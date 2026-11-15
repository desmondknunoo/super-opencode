//! The terminal pane: your shell, running in the project folder in a pseudo-terminal, drawn
//! with the app's monospace font. It reads colours, the cursor, and full-screen programs
//! through a VT100 parser, and keeps 5,000 lines of history to scroll back through.

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Event, EventFilter, Key, Modifiers, Rect, Sense, Stroke,
    Ui, pos2, text::LayoutJob, vec2,
};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::theme::{self, Palette};

const SCROLLBACK: usize = 5_000;

pub struct Terminal {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    exited: Arc<AtomicBool>,
    pub directory: PathBuf,
    size: (u16, u16),
    /// Lines scrolled back into history. Zero follows the output.
    scrolled: usize,
    id: egui::Id,
    wants_focus: bool,
}

impl Terminal {
    /// Starts your login shell in `directory`.
    pub fn start(directory: &Path, ctx: &egui::Context) -> Result<Self, String> {
        let size = (24, 80);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: size.0,
                cols: size.1,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("Couldn’t open a terminal: {error}"))?;
        let mut command = CommandBuilder::new_default_prog();
        command.cwd(directory);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "SuperOpenCode");
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("Couldn’t start your shell: {error}"))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("Couldn’t read the terminal: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("Couldn’t write to the terminal: {error}"))?;
        let parser = Arc::new(Mutex::new(vt100::Parser::new(
            std::num::NonZeroU16::new(size.0).unwrap_or(std::num::NonZeroU16::MIN),
            std::num::NonZeroU16::new(size.1).unwrap_or(std::num::NonZeroU16::MIN),
            SCROLLBACK,
        )));
        let exited = Arc::new(AtomicBool::new(false));
        {
            let parser = Arc::clone(&parser);
            let exited = Arc::clone(&exited);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let mut buffer = [0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => {
                            if let Ok(mut parser) = parser.lock() {
                                parser.process(&buffer[..count]);
                            }
                            ctx.request_repaint();
                        }
                    }
                }
                exited.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            });
        }
        Ok(Self {
            parser,
            writer,
            master: pair.master,
            child,
            exited,
            directory: directory.to_path_buf(),
            size,
            scrolled: 0,
            wants_focus: true,
            id: egui::Id::new(("terminal", uuid::Uuid::new_v4())),
        })
    }

    /// The tab's name: the shell and the folder it started in.
    pub fn label(&self) -> String {
        let shell = std::env::var("SHELL")
            .ok()
            .and_then(|shell| shell.rsplit('/').next().map(str::to_owned))
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    "PowerShell".into()
                } else {
                    "shell".into()
                }
            });
        let folder = self
            .directory
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{shell} · {folder}")
    }

    /// Gives the terminal the keyboard on its next frame.
    pub fn focus(&mut self) {
        self.wants_focus = true;
    }

    pub fn exited(&self) -> bool {
        self.exited.load(Ordering::Relaxed)
    }

    fn send(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
        self.scrolled = 0;
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);
        if let Ok(mut parser) = self.parser.lock()
            && let (Some(rows), Some(cols)) = (
                std::num::NonZeroU16::new(rows),
                std::num::NonZeroU16::new(cols),
            )
        {
            parser.screen_mut().set_size(rows, cols);
        }
        let _ = self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    /// Draws the screen in the space left in `ui` and passes typing to the shell while it
    /// has the keyboard. Returns whether it has the keyboard.
    pub fn show(&mut self, ui: &mut Ui, palette: &Palette) -> bool {
        let font = theme::mono(theme::CODE);
        let (cell_width, row_height) =
            ui.fonts_mut(|fonts| (fonts.glyph_width(&font, 'M'), fonts.row_height(&font) + 2.0));
        // The terminal is a widget with its own ID, so it can hold the keyboard like a text box.
        let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        let response = ui.interact(rect, self.id, Sense::click());
        let inner = rect.shrink2(vec2(10.0, 8.0));
        let rows = ((inner.height() / row_height).floor() as u16).max(2);
        let cols = ((inner.width() / cell_width).floor() as u16).max(10);
        self.resize(rows, cols);

        if response.clicked() || self.wants_focus {
            response.request_focus();
            self.wants_focus = false;
        }
        let focused = response.has_focus();
        if focused {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    self.id,
                    EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
            self.take_input(ui);
        }
        // The mouse wheel scrolls back through history.
        if response.hovered() {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll.abs() > 0.5 {
                let lines = (scroll / row_height).round() as isize;
                self.scrolled =
                    (self.scrolled as isize + lines).clamp(0, SCROLLBACK as isize) as usize;
            }
        }

        let painter = ui.painter_at(rect);
        let background = palette.code_ground;
        painter.rect_filled(rect, CornerRadius::ZERO, background);
        let Ok(mut parser) = self.parser.lock() else {
            return focused;
        };
        parser.screen_mut().set_scrollback(self.scrolled);
        self.scrolled = parser.screen().scrollback();
        let screen = parser.screen();
        let dark = ui.visuals().dark_mode;
        let (screen_rows, screen_cols) = screen.size();
        for row in 0..screen_rows.get().min(rows) {
            let top = inner.top() + row as f32 * row_height;
            let mut job = LayoutJob::default();
            let mut col = 0;
            while col < screen_cols.get().min(cols) {
                let Some(cell) = screen.cell(row, col) else {
                    break;
                };
                let (mut foreground, mut back) = (
                    color(cell.fgcolor(), palette, dark, true),
                    color(cell.bgcolor(), palette, dark, false),
                );
                if cell.inverse() {
                    std::mem::swap(&mut foreground, &mut back);
                    if back == Color32::TRANSPARENT {
                        back = palette.ink;
                    }
                    if foreground == Color32::TRANSPARENT {
                        foreground = background;
                    }
                }
                if cell.bold() && !dark {
                    foreground = foreground.gamma_multiply(1.0);
                }
                if cell.dim() {
                    foreground = foreground.gamma_multiply(0.6);
                }
                if back != Color32::TRANSPARENT {
                    let left = inner.left() + col as f32 * cell_width;
                    let width = if cell.is_wide() { 2.0 } else { 1.0 } * cell_width;
                    painter.rect_filled(
                        Rect::from_min_size(pos2(left, top), vec2(width, row_height)),
                        CornerRadius::ZERO,
                        back,
                    );
                }
                let text = if cell.has_contents() {
                    cell.contents()
                } else {
                    " "
                };
                job.append(
                    text,
                    0.0,
                    egui::TextFormat {
                        font_id: font.clone(),
                        color: foreground,
                        underline: if cell.underline() {
                            Stroke::new(1.0, foreground)
                        } else {
                            Stroke::NONE
                        },
                        italics: cell.italic(),
                        ..Default::default()
                    },
                );
                col += if cell.is_wide() { 2 } else { 1 };
            }
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            painter.galley(pos2(inner.left(), top + 1.0), galley, palette.ink);
        }
        // The cursor: a block when the terminal has the keyboard, an outline when not.
        if self.scrolled == 0 && !screen.hide_cursor() {
            let (row, col) = screen.cursor_position();
            let cursor = Rect::from_min_size(
                pos2(
                    inner.left() + col as f32 * cell_width,
                    inner.top() + row as f32 * row_height,
                ),
                vec2(cell_width, row_height),
            );
            if focused {
                painter.rect_filled(
                    cursor,
                    CornerRadius::same(1),
                    palette.accent.gamma_multiply(0.55),
                );
            } else {
                painter.rect_stroke(
                    cursor,
                    CornerRadius::same(1),
                    Stroke::new(1.0, palette.ink_faint),
                    egui::StrokeKind::Inside,
                );
            }
        }
        if self.scrolled > 0 {
            painter.text(
                pos2(rect.right() - 10.0, rect.top() + 8.0),
                Align2::RIGHT_TOP,
                format!("{} lines up · type to return", self.scrolled),
                theme::text(theme::SMALL - 1.0),
                palette.ink_faint,
            );
        }
        if !focused {
            response.on_hover_cursor(egui::CursorIcon::Text);
        }
        focused
    }

    /// Turns key presses, typed text, and pastes into what a terminal sends.
    fn take_input(&mut self, ui: &mut Ui) {
        let (application_cursor, bracketed) = self
            .parser
            .lock()
            .map(|parser| {
                (
                    parser.screen().application_cursor(),
                    parser.screen().bracketed_paste(),
                )
            })
            .unwrap_or((false, false));
        let events = ui.input(|input| input.events.clone());
        let mut out: Vec<u8> = Vec::new();
        for event in events {
            match event {
                Event::Text(text) => out.extend_from_slice(text.as_bytes()),
                Event::Paste(text) => {
                    if bracketed {
                        out.extend_from_slice(b"\x1b[200~");
                        out.extend_from_slice(text.as_bytes());
                        out.extend_from_slice(b"\x1b[201~");
                    } else {
                        out.extend_from_slice(text.as_bytes());
                    }
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(bytes) = key_bytes(key, modifiers, application_cursor) {
                        out.extend_from_slice(&bytes);
                    }
                }
                _ => {}
            }
        }
        if !out.is_empty() {
            self.send(&out);
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// What a key sends. Plain characters arrive as text events instead.
fn key_bytes(key: Key, modifiers: Modifiers, application_cursor: bool) -> Option<Vec<u8>> {
    // Ctrl with a letter sends the control character, as in any terminal. On macOS, Command
    // shortcuts stay with the app.
    if modifiers.ctrl && !modifiers.alt {
        let name = key.name();
        if name.len() == 1 {
            let letter = name.as_bytes()[0].to_ascii_lowercase();
            if letter.is_ascii_lowercase() {
                return Some(vec![letter - b'a' + 1]);
            }
        }
        match key {
            Key::OpenBracket => return Some(vec![0x1b]),
            Key::Backslash => return Some(vec![0x1c]),
            Key::CloseBracket => return Some(vec![0x1d]),
            _ => {}
        }
    }
    if modifiers.mac_cmd || modifiers.command && !cfg!(target_os = "macos") && modifiers.ctrl {
        return None;
    }
    let arrow = |letter: u8| {
        if application_cursor {
            vec![0x1b, b'O', letter]
        } else {
            vec![0x1b, b'[', letter]
        }
    };
    let bytes = match key {
        Key::Enter => vec![b'\r'],
        Key::Backspace if modifiers.alt => vec![0x1b, 0x7f],
        Key::Backspace => vec![0x7f],
        Key::Tab if modifiers.shift => b"\x1b[Z".to_vec(),
        Key::Tab => vec![b'\t'],
        Key::Escape => vec![0x1b],
        Key::ArrowUp => arrow(b'A'),
        Key::ArrowDown => arrow(b'B'),
        Key::ArrowRight if modifiers.alt => b"\x1bf".to_vec(),
        Key::ArrowLeft if modifiers.alt => b"\x1bb".to_vec(),
        Key::ArrowRight => arrow(b'C'),
        Key::ArrowLeft => arrow(b'D'),
        Key::Home => b"\x1b[H".to_vec(),
        Key::End => b"\x1b[F".to_vec(),
        Key::PageUp => b"\x1b[5~".to_vec(),
        Key::PageDown => b"\x1b[6~".to_vec(),
        Key::Delete => b"\x1b[3~".to_vec(),
        Key::Insert => b"\x1b[2~".to_vec(),
        Key::F1 => b"\x1bOP".to_vec(),
        Key::F2 => b"\x1bOQ".to_vec(),
        Key::F3 => b"\x1bOR".to_vec(),
        Key::F4 => b"\x1bOS".to_vec(),
        _ => return None,
    };
    Some(bytes)
}

/// The sixteen standard colours, tuned to read on the app's code background.
fn ansi(index: u8, dark: bool) -> Color32 {
    const LIGHT: [(u8, u8, u8); 16] = [
        (0x2E, 0x34, 0x40),
        (0xC0, 0x30, 0x3A),
        (0x1F, 0x7A, 0x3E),
        (0x8A, 0x63, 0x00),
        (0x2A, 0x5D, 0xC8),
        (0x8E, 0x3B, 0xB0),
        (0x0F, 0x7C, 0x86),
        (0x6B, 0x70, 0x86),
        (0x55, 0x5B, 0x6E),
        (0xD9, 0x3D, 0x48),
        (0x26, 0x96, 0x4C),
        (0xA0, 0x74, 0x00),
        (0x3F, 0x72, 0xE0),
        (0xA4, 0x4E, 0xC6),
        (0x12, 0x92, 0x9E),
        (0x14, 0x16, 0x2B),
    ];
    const DARK: [(u8, u8, u8); 16] = [
        (0x3B, 0x40, 0x5E),
        (0xFF, 0x7A, 0x85),
        (0x6F, 0xD6, 0x8E),
        (0xF0, 0xC2, 0x6A),
        (0x7C, 0xA8, 0xFF),
        (0xD3, 0x8C, 0xF5),
        (0x5F, 0xD3, 0xD9),
        (0xC8, 0xCC, 0xE0),
        (0x6A, 0x70, 0x94),
        (0xFF, 0x9A, 0xA3),
        (0x8F, 0xE6, 0xA8),
        (0xF6, 0xD4, 0x8C),
        (0x9B, 0xBE, 0xFF),
        (0xE0, 0xA8, 0xF8),
        (0x86, 0xE2, 0xE6),
        (0xF3, 0xF4, 0xFB),
    ];
    let (r, g, b) = if dark { DARK } else { LIGHT }[usize::from(index) % 16];
    Color32::from_rgb(r, g, b)
}

/// A cell colour on screen. The default background is transparent, so the pane shows.
fn color(color: vt100::Color, palette: &Palette, dark: bool, foreground: bool) -> Color32 {
    match color {
        vt100::Color::Default if foreground => palette.ink,
        vt100::Color::Default => Color32::TRANSPARENT,
        vt100::Color::Idx(index) if index < 16 => ansi(index, dark),
        vt100::Color::Idx(index) if index < 232 => {
            // The 6×6×6 colour cube.
            let index = index - 16;
            let level = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
            Color32::from_rgb(level(index / 36), level(index / 6 % 6), level(index % 6))
        }
        vt100::Color::Idx(index) => {
            let grey = 8 + (index - 232) * 10;
            Color32::from_rgb(grey, grey, grey)
        }
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_become_terminal_bytes() {
        let none = Modifiers::NONE;
        assert_eq!(key_bytes(Key::Enter, none, false), Some(vec![b'\r']));
        assert_eq!(
            key_bytes(Key::ArrowUp, none, false),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_bytes(Key::ArrowUp, none, true),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(key_bytes(Key::C, Modifiers::CTRL, false), Some(vec![3]));
        assert_eq!(key_bytes(Key::A, none, false), None, "letters come as text");
    }

    #[test]
    fn colours_cover_the_256_palette() {
        let palette = crate::theme::Palette::light();
        assert_eq!(
            color(vt100::Color::Idx(16), &palette, false, true),
            Color32::from_rgb(0, 0, 0)
        );
        assert_eq!(
            color(vt100::Color::Idx(231), &palette, false, true),
            Color32::from_rgb(255, 255, 255)
        );
        assert_eq!(
            color(vt100::Color::Default, &palette, false, false),
            Color32::TRANSPARENT
        );
    }

    /// Types into the terminal widget the way the window does, through egui's key events:
    /// `cargo test typing_reaches_the_shell -- --ignored`
    #[test]
    #[ignore]
    fn typing_reaches_the_shell() {
        let folder = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut terminal = Terminal::start(folder.path(), &ctx).unwrap();
        let palette = crate::theme::Palette::light();
        let frame = |events: Vec<Event>, terminal: &mut Terminal| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 400.0))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                terminal.show(ui, &palette);
            });
            output.textures_delta.clear();
        };
        // The first frames give it the keyboard, as opening the dock does.
        frame(Vec::new(), &mut terminal);
        frame(Vec::new(), &mut terminal);
        frame(
            vec![
                Event::Text("echo TYPED_$((6*7))".into()),
                Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                },
            ],
            &mut terminal,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let contents = terminal.parser.lock().unwrap().screen().contents();
            if contents.contains("TYPED_42") {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "typing didn't arrive: {contents}"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Starts the shell in a temporary folder and runs a command:
    /// `cargo test terminal_runs_a_command -- --ignored`
    #[test]
    #[ignore]
    fn terminal_runs_a_command() {
        let folder = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut terminal = Terminal::start(folder.path(), &ctx).unwrap();
        terminal.send(b"echo SUPER_$((40+2))\r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let contents = terminal.parser.lock().unwrap().screen().contents();
            if contents.contains("SUPER_42") {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no output: {contents}"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        terminal.send(b"exit\r");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !terminal.exited() {
            assert!(
                std::time::Instant::now() < deadline,
                "the shell didn't exit"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}
