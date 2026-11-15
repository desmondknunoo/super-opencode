//! The side pane beside the conversation, opened from the buttons at the top right as in
//! Claude Code: OpenCode's task list, a terminal in the project, the project's changes, the
//! browser, and the project's files, with a menu of project actions.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Frame, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, Stroke, Ui, pos2, vec2,
};

use crate::{
    app::{App, View},
    browser,
    icons::{self, Icon},
    theme::{self, Palette},
    views::{label, one_line, open_path, problem, radius, tilde},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Tasks,
    Terminal,
    Changes,
    Browser,
    Files,
}

impl Pane {
    pub const ALL: [Self; 5] = [
        Self::Tasks,
        Self::Terminal,
        Self::Changes,
        Self::Browser,
        Self::Files,
    ];

    fn icon(self) -> Icon {
        match self {
            Self::Tasks => Icon::List,
            Self::Terminal => Icon::Terminal,
            Self::Changes => Icon::Diff,
            Self::Browser => Icon::Globe,
            Self::Files => Icon::Folder,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Tasks => "Tasks",
            Self::Terminal => "Terminal",
            Self::Changes => "Changes",
            Self::Browser => "Browser",
            Self::Files => "Files",
        }
    }
}

/// A square button with an icon, highlighted when its pane is open.
fn icon_button(
    ui: &mut Ui,
    palette: &Palette,
    rect: Rect,
    icon: Icon,
    on: bool,
    tip: &str,
) -> egui::Response {
    let response = ui
        .interact(rect, ui.id().with(("pane-button", tip)), Sense::click())
        .on_hover_text(tip);
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, radius(7), palette.accent_wash);
    } else if response.hovered() {
        painter.rect_filled(rect, radius(7), palette.hover);
    }
    icons::paint(
        painter,
        Rect::from_center_size(rect.center(), vec2(15.0, 15.0)),
        icon,
        if on {
            palette.accent
        } else {
            palette.ink_muted
        },
    );
    response
}

/// A small button in a pane's bars.
fn tool(ui: &mut Ui, palette: &Palette, icon: Icon, tip: &str, enabled: bool) -> bool {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 28.0), sense);
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, radius(7), palette.hover);
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
        icon,
        if enabled {
            palette.ink_muted
        } else {
            palette.line_strong
        },
    );
    response.on_hover_text(tip).clicked()
}

fn note(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.add(label(text, theme::text(theme::TEXT), palette.ink_faint).wrap());
}

// ---------------------------------------------------------------------------------------
// Tasks: OpenCode's to-do list for the session.

#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// A to-do list read on another thread, with the session it belongs to.
type TodoRead = mpsc::Receiver<(String, Result<Vec<Todo>, String>)>;

pub struct Todos {
    /// The OpenCode session the list belongs to.
    pub session: Option<String>,
    pub items: Result<Vec<Todo>, String>,
    task: Option<TodoRead>,
    pub read_at: Option<Instant>,
    database: Option<Option<PathBuf>>,
}

impl Default for Todos {
    fn default() -> Self {
        Self {
            session: None,
            items: Ok(Vec::new()),
            task: None,
            read_at: None,
            database: None,
        }
    }
}

/// Reads a session's to-do list from OpenCode's database without changing it.
fn read_todos(database: &Path, session: &str) -> Result<Vec<Todo>, String> {
    let db = rusqlite::Connection::open_with_flags(
        database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| error.to_string())?;
    let _ = db.busy_timeout(Duration::from_secs(1));
    let mut statement = db
        .prepare(
            "SELECT content, status, priority FROM todo WHERE session_id = ? ORDER BY position",
        )
        .map_err(|error| error.to_string())?;
    statement
        .query_map([session], |row| {
            Ok(Todo {
                content: row.get(0)?,
                status: row.get(1)?,
                priority: row.get(2)?,
            })
        })
        .and_then(Iterator::collect)
        .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------------------
// Changes: what Git sees changed in the project.

#[derive(Clone, Debug, PartialEq)]
pub struct Changed {
    pub path: String,
    /// Git's status letters, such as `M`, `A`, `D`, `R`, or `??`.
    pub status: String,
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

impl Changed {
    fn untracked(&self) -> bool {
        self.status == "??"
    }

    fn describe(&self) -> &'static str {
        match self.status.trim().chars().next() {
            Some('?') | Some('A') => "Added",
            Some('D') => "Deleted",
            Some('R') => "Renamed",
            Some('U') => "Conflict",
            _ => "Modified",
        }
    }
}

#[derive(Default)]
pub struct Changes {
    pub project: Option<PathBuf>,
    pub list: Option<Result<Vec<Changed>, String>>,
    task: Option<mpsc::Receiver<Result<Vec<Changed>, String>>>,
    pub selected: Option<String>,
    pub diff: Option<Result<String, String>>,
    diff_task: Option<mpsc::Receiver<Result<String, String>>>,
}

fn git(project: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(project)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());
    if let Some(path) = crate::runner::child_path(std::env::var_os("PATH"), dirs::home_dir()) {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

fn run_git(project: &Path, args: &[&str]) -> Result<String, String> {
    let output = git(project)
        .args(args)
        .output()
        .map_err(|_| "Git isn’t installed, so changes can’t be listed.".to_owned())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// `git status --porcelain -z`: status letters and paths. A rename is followed by its old path.
fn parse_status(text: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut parts = text.split('\0').filter(|part| !part.is_empty());
    while let Some(entry) = parts.next() {
        if entry.len() < 4 {
            continue;
        }
        let status = entry[..2].to_owned();
        let path = entry[3..].to_owned();
        if status.starts_with('R') || status.starts_with('C') {
            parts.next();
        }
        entries.push((status.trim().to_owned(), path));
    }
    entries
}

/// `git diff --numstat`: lines added and removed for each path. Binary files show `-`.
fn parse_numstat(text: &str) -> HashMap<String, (Option<u64>, Option<u64>)> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let added = fields.next()?.parse().ok();
            let removed = fields.next()?.parse().ok();
            let path = fields.next()?;
            // A rename reads `old => new` or `dir/{old => new}`; keep the new path.
            let path = match path.split_once(" => ") {
                Some((before, after)) => match (before.split_once('{'), after.split_once('}')) {
                    (Some((prefix, _)), Some((new, suffix))) => format!("{prefix}{new}{suffix}"),
                    _ => after.to_owned(),
                },
                None => path.to_owned(),
            };
            Some((path, (added, removed)))
        })
        .collect()
}

fn list_changes(project: &Path) -> Result<Vec<Changed>, String> {
    match run_git(project, &["rev-parse", "--is-inside-work-tree"]) {
        Ok(answer) if answer.trim() == "true" => {}
        Err(message) if message.starts_with("Git isn’t") => return Err(message),
        _ => {
            return Err("Not a Git repository.".into());
        }
    }
    let status = run_git(
        project,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    // A repository with no commits yet has no HEAD to compare with.
    let numbers = run_git(project, &["diff", "HEAD", "--numstat"])
        .or_else(|_| run_git(project, &["diff", "--cached", "--numstat"]))
        .unwrap_or_default();
    let counts = parse_numstat(&numbers);
    let mut changed: Vec<Changed> = parse_status(&status)
        .into_iter()
        .map(|(status, path)| {
            let (added, removed) = if status == "??" {
                let lines = std::fs::read(project.join(&path))
                    .ok()
                    .filter(|bytes| bytes.len() < 4 << 20 && !bytes.contains(&0))
                    .map(|bytes| bytes.iter().filter(|&&byte| byte == b'\n').count() as u64);
                (lines, lines.map(|_| 0))
            } else {
                counts.get(&path).copied().unwrap_or((None, None))
            };
            Changed {
                path,
                status,
                added,
                removed,
            }
        })
        .collect();
    changed.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changed)
}

/// The diff of one file, or a new file's contents as added lines.
fn file_diff(project: &Path, changed: &Changed) -> Result<String, String> {
    if changed.untracked() {
        let bytes =
            std::fs::read(project.join(&changed.path)).map_err(|error| error.to_string())?;
        if bytes.contains(&0) {
            return Ok("Binary file".into());
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut out = format!("@@ new file: {} @@\n", changed.path);
        for line in text.lines().take(4000) {
            out.push('+');
            out.push_str(line);
            out.push('\n');
        }
        return Ok(out);
    }
    run_git(project, &["diff", "HEAD", "--", &changed.path])
        .or_else(|_| run_git(project, &["diff", "--cached", "--", &changed.path]))
        .map(|diff| {
            if diff.trim().is_empty() {
                "No line changes, only the file’s mode or name.".into()
            } else {
                diff
            }
        })
}

// ---------------------------------------------------------------------------------------
// Files: the project's folders and files, and a preview of one.

#[derive(Clone, Debug)]
struct Entry {
    name: String,
    path: PathBuf,
    folder: bool,
}

pub enum Preview {
    Text { text: String, cut: bool },
    Image,
    Other(String),
}

#[derive(Default)]
pub struct Files {
    pub root: Option<PathBuf>,
    pub expanded: HashSet<PathBuf>,
    listed: HashMap<PathBuf, Result<Vec<Entry>, String>>,
    pub open: Option<(PathBuf, Preview)>,
}

/// Names the tree leaves out: version control internals and Finder's files.
const HIDDEN: &[&str] = &[".git", ".DS_Store", ".hg", ".svn"];
const PREVIEW_LIMIT: usize = 256 * 1024;

fn list_folder(folder: &Path) -> Result<Vec<Entry>, String> {
    let mut entries: Vec<Entry> = std::fs::read_dir(folder)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if HIDDEN.contains(&name.as_str()) {
                return None;
            }
            let folder = entry.file_type().ok()?.is_dir();
            Some(Entry {
                name,
                path: entry.path(),
                folder,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.folder
            .cmp(&a.folder)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(entries)
}

fn preview(path: &Path) -> Preview {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tiff", "tif"].contains(&extension.as_str()) {
        return Preview::Image;
    }
    match std::fs::read(path) {
        Err(error) => Preview::Other(format!("Couldn’t read it: {error}")),
        Ok(bytes) if bytes.iter().take(8192).any(|&byte| byte == 0) => Preview::Other(format!(
            "No preview for this kind of file ({}).",
            size_label(bytes.len() as u64)
        )),
        Ok(bytes) => {
            let cut = bytes.len() > PREVIEW_LIMIT;
            let shown = &bytes[..bytes.len().min(PREVIEW_LIMIT)];
            Preview::Text {
                text: String::from_utf8_lossy(shown).into_owned(),
                cut,
            }
        }
    }
}

fn size_label(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} bytes"),
        1024..1_048_576 => format!("{:.0} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

// ---------------------------------------------------------------------------------------
// Opening the project elsewhere.

fn open_in_terminal_app(folder: &Path) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        Command::new("open")
            .args(["-a", "Terminal"])
            .arg(folder)
            .spawn()
    } else if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "start", "", "cmd", "/K", "cd", "/d"])
            .arg(folder)
            .spawn()
    } else {
        Command::new("x-terminal-emulator")
            .current_dir(folder)
            .spawn()
    };
    result
        .map(|_| ())
        .map_err(|error| format!("Couldn’t open a terminal: {error}"))
}

/// The VS Code command, when it is installed.
fn vs_code() -> Option<PathBuf> {
    let path = crate::runner::child_path(std::env::var_os("PATH"), dirs::home_dir())?;
    let name = if cfg!(windows) { "code.cmd" } else { "code" };
    std::env::split_paths(&path)
        .map(|folder| folder.join(name))
        .chain(cfg!(target_os = "macos").then(|| {
            PathBuf::from("/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code")
        }))
        .find(|candidate| candidate.is_file())
}

impl App {
    /// The pane buttons at the top right of `rect`. Returns where they start.
    pub(crate) fn pane_buttons(&mut self, ui: &mut Ui, palette: &Palette, rect: Rect) -> f32 {
        let size = 30.0;
        let gap = 2.0;
        let count = Pane::ALL.len() as f32 + 1.0;
        let mut left = rect.right() - 14.0 - count * (size + gap);
        let start = left;
        let mut clicked = None;
        for pane in Pane::ALL {
            let button =
                Rect::from_min_size(pos2(left, rect.center().y - size / 2.0), vec2(size, size));
            let on = if pane == Pane::Terminal {
                self.terminal_open
            } else {
                self.pane == Some(pane)
            };
            if icon_button(ui, palette, button, pane.icon(), on, pane.name()).clicked() {
                clicked = Some(pane);
            }
            left += size + gap;
        }
        let more = Rect::from_min_size(pos2(left, rect.center().y - size / 2.0), vec2(size, size));
        let response = icon_button(ui, palette, more, Icon::More, false, "More");
        egui::Popup::from_toggle_button_response(&response)
            .align(egui::RectAlign::BOTTOM_END)
            .gap(6.0)
            .width(250.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .show(|ui| self.more_menu(ui, palette));
        if let Some(pane) = clicked {
            self.toggle_pane(ui.ctx(), pane);
        }
        start
    }

    /// Opens `pane`, or closes it when it is already open.
    pub(crate) fn toggle_pane(&mut self, ctx: &egui::Context, pane: Pane) {
        // The terminal has its own dock, at the bottom or the side.
        if pane == Pane::Terminal {
            self.terminal_open = !self.terminal_open;
            if !self.terminal_open {
                self.terminal_focused = false;
            } else if let Some(terminal) = self.terminals.get_mut(self.terminal_tab) {
                terminal.focus();
            }
            ctx.request_repaint();
            return;
        }
        if self.pane == Some(pane) {
            self.pane = None;
            self.pane_wide = false;
            return;
        }
        self.show_pane(ctx, pane);
    }

    pub(crate) fn show_pane(&mut self, ctx: &egui::Context, pane: Pane) {
        self.pane = Some(pane);
        match pane {
            Pane::Changes => self.refresh_changes(),
            Pane::Tasks => self.todos.read_at = None,
            Pane::Browser => {
                if self.browser.is_none() {
                    self.browser = Some(browser::Browser::new(None));
                }
            }
            Pane::Files => {
                if self.files.root != self.settings.project {
                    self.files = Files {
                        root: self.settings.project.clone(),
                        ..Default::default()
                    };
                }
            }
            Pane::Terminal => {}
        }
        ctx.request_repaint();
    }

    pub(crate) fn more_menu(&mut self, ui: &mut Ui, palette: &Palette) {
        ui.set_min_width(230.0);
        let project = self.settings.project.clone();
        let item = |ui: &mut Ui, text: &str, enabled: bool| {
            ui.add_enabled(
                enabled,
                egui::Button::new(text)
                    .frame(false)
                    .min_size(vec2(220.0, 26.0)),
            )
            .clicked()
        };
        let file_manager = if cfg!(target_os = "macos") {
            "Show Project in Finder"
        } else if cfg!(windows) {
            "Show Project in Explorer"
        } else {
            "Show Project in Files"
        };
        if item(ui, file_manager, project.is_some())
            && let Some(folder) = &project
            && let Err(message) = open_path(folder)
        {
            self.inputs.notice = Some(message);
        }
        if item(ui, "Open Project in Terminal", project.is_some())
            && let Some(folder) = &project
            && let Err(message) = open_in_terminal_app(folder)
        {
            self.inputs.notice = Some(message);
        }
        let code = vs_code();
        if code.is_some()
            && item(ui, "Open Project in VS Code", project.is_some())
            && let (Some(code), Some(folder)) = (code, &project)
        {
            let _ = Command::new(code).arg(folder).spawn();
        }
        ui.separator();
        if item(ui, "Copy Project Path", project.is_some())
            && let Some(folder) = &project
        {
            ui.ctx().copy_text(folder.display().to_string());
        }
        let session = self
            .active
            .as_ref()
            .and_then(|session| session.server_session.clone());
        if item(ui, "Copy OpenCode Session ID", session.is_some())
            && let Some(id) = session
        {
            ui.ctx().copy_text(id);
        }
        ui.separator();
        if item(ui, "Usage", true) {
            self.open(View::Usage);
        }
        if item(ui, "Settings", true) {
            self.open(View::Settings);
        }
        let _ = palette;
    }

    /// The pane: its bar with tabs or a title, expand and close, then its contents.
    pub(crate) fn pane_ui(&mut self, ui: &mut Ui, palette: &Palette, pane: Pane) {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let mut close = false;
        let bar = Frame::new()
            .inner_margin(Margin {
                left: 10,
                right: 8,
                top: 8,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.set_min_height(30.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let controls = 2.0 * 30.0 + 6.0;
                    let room = (ui.available_width() - controls).max(60.0);
                    ui.allocate_ui_with_layout(
                        vec2(room, 30.0),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            if pane == Pane::Browser {
                                self.browser_tabs(ui, palette);
                            } else {
                                ui.add(
                                    label(pane.name(), theme::strong(theme::TEXT), palette.ink)
                                        .selectable(false),
                                );
                            }
                        },
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if tool(ui, palette, Icon::Close, "Close the pane", true) {
                            close = true;
                        }
                        let (icon, tip) = if self.pane_wide {
                            (Icon::Collapse, "Show the conversation")
                        } else {
                            (Icon::Expand, "Fill the window")
                        };
                        if tool(ui, palette, icon, tip, true) {
                            self.pane_wide = !self.pane_wide;
                        }
                    });
                });
            })
            .response
            .rect;
        ui.painter()
            .hline(bar.x_range(), bar.bottom(), Stroke::new(1.0, palette.line));
        match pane {
            Pane::Tasks => self.tasks_pane(ui, palette),
            Pane::Terminal => self.terminal_dock(ui, palette),
            Pane::Changes => self.changes_pane(ui, palette),
            Pane::Browser => self.browser_pane(ui, palette),
            Pane::Files => self.files_pane(ui, palette),
        }
        if close {
            self.pane = None;
            self.pane_wide = false;
        }
    }

    // ---- Browser ------------------------------------------------------------------------

    fn browser_tabs(&mut self, ui: &mut Ui, palette: &Palette) {
        let Some(browser) = &mut self.browser else {
            return;
        };
        let mut select = None;
        let mut close = None;
        ScrollArea::horizontal()
            .id_salt("browser-tabs")
            .max_width(ui.available_width() - 32.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (index, tab) in browser.tabs.iter().enumerate() {
                        let active = index == browser.active;
                        let text = tab.label();
                        let galley = one_line(
                            ui,
                            &text,
                            theme::text(theme::SMALL + 0.5),
                            if active {
                                palette.ink
                            } else {
                                palette.ink_muted
                            },
                            150.0,
                        );
                        let width = galley.size().x + 12.0 + 22.0;
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
                        let hovered = response.hovered();
                        let painter = ui.painter();
                        if active {
                            painter.rect_filled(rect, radius(7), palette.hover);
                        } else if hovered {
                            painter.rect_filled(rect, radius(7), palette.hover.gamma_multiply(0.6));
                        }
                        painter.galley(
                            pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
                            galley,
                            palette.ink,
                        );
                        let cross = Rect::from_center_size(
                            pos2(rect.right() - 12.0, rect.center().y),
                            vec2(16.0, 16.0),
                        );
                        let over_cross = ui
                            .ctx()
                            .pointer_hover_pos()
                            .is_some_and(|pointer| cross.contains(pointer));
                        if hovered || active {
                            if over_cross {
                                ui.painter().rect_filled(cross, radius(4), palette.line);
                            }
                            icons::paint(
                                ui.painter(),
                                cross.shrink(3.5),
                                Icon::Close,
                                palette.ink_faint,
                            );
                        }
                        if response.clicked() {
                            if over_cross {
                                close = Some(index);
                            } else {
                                select = Some(index);
                            }
                        }
                        let response = response.on_hover_text(tab.url());
                        if response.middle_clicked() {
                            close = Some(index);
                        }
                    }
                });
            });
        if tool(ui, palette, Icon::Plus, "New tab", true) {
            browser.new_tab();
        }
        if let Some(index) = select {
            browser.active = index;
        }
        if let Some(index) = close {
            browser.close_tab(index);
        }
    }

    fn browser_pane(&mut self, ui: &mut Ui, palette: &Palette) {
        if self.browser.is_none() {
            self.browser = Some(browser::Browser::new(None));
        }
        let links_in_app = self.settings.links_in_app;
        let auto_open = self.settings.auto_open_html;
        let mut toggles = (None, None);
        let mut open_html = false;
        let Some(browser) = &mut self.browser else {
            return;
        };
        let mut load = None;
        let mut external = None;
        let bar = Frame::new()
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let has_page = !browser.tab().is_blank();
                    if tool(ui, palette, Icon::Back, "Back", has_page) {
                        browser.tab().back();
                    }
                    if tool(ui, palette, Icon::Forward, "Forward", has_page) {
                        browser.tab().forward();
                    }
                    if tool(ui, palette, Icon::Reload, "Reload", has_page) {
                        browser.tab().reload();
                    }
                    ui.add_space(4.0);
                    let title = browser.tab().title();
                    let focus = std::mem::take(&mut browser.focus_address);
                    let tab = browser.tab_mut();
                    let field = egui::TextEdit::singleline(&mut tab.address)
                        .hint_text("Type an address")
                        .font(theme::text(theme::SMALL + 0.5))
                        .desired_width((ui.available_width() - 34.0).max(80.0))
                        .margin(Margin::symmetric(8, 6));
                    let response = ui.add(field);
                    if focus {
                        response.request_focus();
                    }
                    let response = if title.is_empty() {
                        response
                    } else {
                        response.on_hover_text(title)
                    };
                    if response.gained_focus() {
                        tab.editing = true;
                    }
                    if response.lost_focus() {
                        tab.editing = false;
                        if ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                            load = browser::address_to_url(&tab.address);
                        }
                    }
                    ui.add_space(2.0);
                    let (rect, more) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());
                    if more.hovered() {
                        ui.painter().rect_filled(rect, radius(7), palette.hover);
                    }
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
                        Icon::More,
                        palette.ink_muted,
                    );
                    let more = more.on_hover_text("Page and browser options");
                    egui::Popup::from_toggle_button_response(&more)
                        .align(egui::RectAlign::BOTTOM_END)
                        .gap(6.0)
                        .width(260.0)
                        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                        .show(|ui| {
                            ui.set_min_width(250.0);
                            let url = browser.tab().url();
                            let page = !browser.tab().is_blank();
                            let item = |ui: &mut Ui, text: &str, enabled: bool| {
                                ui.add_enabled(
                                    enabled,
                                    egui::Button::new(text).frame(false).min_size(vec2(240.0, 26.0)),
                                )
                                .clicked()
                            };
                            if item(ui, "Open in Your Browser", page) {
                                external = Some(url.clone());
                                ui.close();
                            }
                            if item(ui, "Copy Address", page) {
                                ui.ctx().copy_text(url.clone());
                                ui.close();
                            }
                            if item(ui, "Print or Save as PDF…", page) {
                                browser.tab().print();
                                ui.close();
                            }
                            if item(ui, "Open HTML File…", true) {
                                open_html = true;
                                ui.close();
                            }
                            ui.separator();
                            ui.add(
                                label("Viewport", theme::text(theme::SMALL), palette.ink_faint)
                                    .selectable(false),
                            );
                            for viewport in browser::Viewport::ALL {
                                if ui
                                    .selectable_label(browser.viewport == viewport, viewport.name())
                                    .clicked()
                                {
                                    browser.viewport = viewport;
                                }
                            }
                            ui.separator();
                            let mut links = links_in_app;
                            if ui.checkbox(&mut links, "Open links in Super OpenCode").changed() {
                                toggles.0 = Some(links);
                            }
                            let mut html = auto_open;
                            if ui
                                .checkbox(&mut html, "Open HTML files OpenCode writes")
                                .changed()
                            {
                                toggles.1 = Some(html);
                            }
                            ui.separator();
                            let mut keep = browser.keep_cookies;
                            if ui
                                .checkbox(&mut keep, "Keep cookies between pages")
                                .on_hover_text("Off opens new pages privately, without saved cookies or site data.")
                                .changed()
                            {
                                browser.keep_cookies = keep;
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Clear Browsing Data").color(palette.danger),
                                    )
                                    .frame(false)
                                    .min_size(vec2(240.0, 26.0)),
                                )
                                .clicked()
                            {
                                browser.clear_browsing_data();
                                ui.close();
                            }
                        });
                });
            })
            .response
            .rect;
        ui.painter()
            .hline(bar.x_range(), bar.bottom(), Stroke::new(1.0, palette.line));
        let loading =
            browser.tab().loading() && browser.tab().failed.is_none() && !browser.tab().is_blank();
        if loading && !self.settings.reduce_motion() {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
            let time = ui.input(|input| input.time) as f32;
            let travel = bar.width() + 120.0;
            let start = bar.left() - 120.0 + (time * 260.0) % travel;
            ui.painter().hline(
                start.max(bar.left())..=(start + 120.0).min(bar.right()),
                bar.bottom(),
                Stroke::new(2.0, palette.accent),
            );
        }
        let (area, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
        ui.painter().rect_filled(area, 0.0, palette.ground);
        let page = browser.viewport.within(area);
        if page != area {
            ui.painter().rect_stroke(
                page.expand(1.0),
                CornerRadius::ZERO,
                Stroke::new(1.0, palette.line),
                egui::StrokeKind::Outside,
            );
        }
        if browser.tab().is_blank() {
            browser.area = None;
            let mut detect = false;
            ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
                ui.add_space((area.height() / 2.0 - 70.0).max(20.0));
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    let (icon, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                    icons::paint(ui.painter(), icon, Icon::Globe, palette.ink_faint);
                    ui.add_space(4.0);
                    ui.add(
                        label("Open a page", theme::strong(theme::TEXT), palette.ink)
                            .selectable(false),
                    );
                    ui.add(
                        label(
                            "Type an address above, or open a link from a reply.",
                            theme::text(theme::SMALL),
                            palette.ink_muted,
                        )
                        .selectable(false),
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let text = "Preview your app?";
                        let width = 260.0;
                        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                        ui.add(
                            label(text, theme::text(theme::SMALL), palette.ink_faint)
                                .selectable(false),
                        );
                        ui.add_space(4.0);
                        if ui
                            .add_enabled(
                                !browser.detecting(),
                                egui::Button::new("Detect Dev Server").small(),
                            )
                            .clicked()
                        {
                            detect = true;
                        }
                    });
                    if let Some(message) = &browser.detect_message {
                        ui.add_space(6.0);
                        ui.add(
                            label(
                                message.clone(),
                                theme::text(theme::SMALL),
                                palette.ink_faint,
                            )
                            .wrap(),
                        );
                    }
                });
            });
            if detect {
                browser.detect_dev_server(ui.ctx());
            }
        } else {
            let note = match &browser.tab().failed {
                Some(problem) => problem.clone(),
                None if !browser::SUPPORTED => {
                    "This build opens pages in your default browser.".to_owned()
                }
                None => format!("Loading {}…", browser.tab().url()),
            };
            ui.painter().text(
                page.center(),
                Align2::CENTER_CENTER,
                note,
                theme::text(theme::SMALL),
                palette.ink_faint,
            );
            browser.area = Some(area);
        }
        if let Some(url) = load {
            browser.tab_mut().load(url);
        }
        if let Some(url) = external
            && open_path(Path::new(&url)).is_err()
        {
            self.inputs.notice = Some("Couldn’t open your browser.".into());
        }
        if let Some(links) = toggles.0 {
            self.settings.links_in_app = links;
            self.save_settings();
        }
        if let Some(html) = toggles.1 {
            self.settings.auto_open_html = html;
            self.save_settings();
        }
        if open_html {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Open an HTML file")
                .add_filter("Web page", &["html", "htm"]);
            if let Some(project) = &self.settings.project {
                dialog = dialog.set_directory(project);
            }
            if let Some(path) = dialog.pick_file() {
                self.open_file_in_browser(&path);
            }
        }
    }

    /// Shows a file from this computer in a browser tab.
    pub(crate) fn open_file_in_browser(&mut self, path: &Path) {
        let url = browser::file_url(path);
        self.browser
            .get_or_insert_with(|| browser::Browser::new(None))
            .open(url);
        self.pane = Some(Pane::Browser);
    }

    // ---- Tasks --------------------------------------------------------------------------

    /// Reads the session's to-do list again when it is stale: every two seconds while a task
    /// runs, and whenever the session changes.
    pub(crate) fn poll_todos(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.todos.task {
            match receiver.try_recv() {
                Ok((session, result)) => {
                    self.todos.task = None;
                    if Some(&session) == self.todos.session.as_ref() {
                        self.todos.items = result;
                    }
                    ctx.request_repaint();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.todos.task = None,
            }
        }
        if self.pane != Some(Pane::Tasks) {
            return;
        }
        let session = self
            .active
            .as_ref()
            .and_then(|session| session.server_session.clone());
        if session != self.todos.session {
            self.todos.session = session.clone();
            self.todos.items = Ok(Vec::new());
            self.todos.read_at = None;
        }
        let Some(session) = session else { return };
        let due = match self.todos.read_at {
            None => true,
            Some(at) => self.is_running() && at.elapsed() > Duration::from_secs(2),
        };
        if self.is_running() {
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        if !due || self.todos.task.is_some() {
            return;
        }
        self.todos.read_at = Some(Instant::now());
        let known = self.todos.database.clone();
        let binary = self.opencode().cloned();
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let database = known
                .flatten()
                .or_else(|| crate::history::opencode_database(binary.as_deref()));
            let result = match &database {
                Some(path) => read_todos(path, &session),
                None => Err("OpenCode’s database wasn’t found on this computer.".into()),
            };
            let _ = sender.send((session, result));
            ctx.request_repaint();
        });
        self.todos.task = Some(receiver);
        if self.todos.database.is_none() {
            // Found once; later reads reuse the same file.
            self.todos.database = Some(crate::history::opencode_database(
                self.opencode().map(PathBuf::as_path),
            ));
        }
    }

    fn tasks_pane(&mut self, ui: &mut Ui, palette: &Palette) {
        Frame::new()
            .inner_margin(Margin::symmetric(16, 14))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.set_min_width(ui.available_width());
                if self.is_running()
                    && let Some((doing, since)) = &self.doing
                {
                    ui.add(
                        label("Now", theme::strong(theme::SMALL), palette.ink_faint)
                            .selectable(false),
                    );
                    ui.add_space(6.0);
                    let seconds = (crate::runner::now_millis() - since) / 1000;
                    ui.add(
                        label(
                            format!("{doing} · {}", crate::activity::elapsed(seconds)),
                            theme::text(theme::TEXT),
                            palette.ink,
                        )
                        .wrap(),
                    );
                    ui.ctx().request_repaint_after(Duration::from_millis(500));
                    ui.add_space(18.0);
                }
                ui.add(
                    label("To-do list", theme::strong(theme::SMALL), palette.ink_faint)
                        .selectable(false),
                );
                ui.add_space(8.0);
                if self.todos.session.is_none() {
                    note(ui, palette, "No tasks yet.");
                    return;
                }
                match &self.todos.items {
                    Err(error) => problem(ui, palette, error),
                    Ok(items) if items.is_empty() => {
                        note(ui, palette, "No to-do list in this session.")
                    }
                    Ok(items) => {
                        let done = items
                            .iter()
                            .filter(|item| item.status == "completed")
                            .count();
                        ui.add(
                            label(
                                format!("{done} of {} done", items.len()),
                                theme::text(theme::SMALL),
                                palette.ink_muted,
                            )
                            .selectable(false),
                        );
                        ui.add_space(8.0);
                        for item in items {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(16.0, 20.0), Sense::hover());
                                let center = rect.center();
                                let painter = ui.painter();
                                match item.status.as_str() {
                                    "completed" => {
                                        painter.circle_filled(center, 7.0, palette.ok);
                                        icons::paint(
                                            painter,
                                            Rect::from_center_size(center, vec2(11.0, 11.0)),
                                            Icon::Check,
                                            palette.raised,
                                        );
                                    }
                                    "in_progress" => {
                                        painter.circle_stroke(
                                            center,
                                            6.5,
                                            Stroke::new(1.5, palette.accent),
                                        );
                                        painter.circle_filled(center, 3.0, palette.accent);
                                    }
                                    "cancelled" => {
                                        painter.circle_stroke(
                                            center,
                                            6.5,
                                            Stroke::new(1.5, palette.line_strong),
                                        );
                                        painter.line_segment(
                                            [center - vec2(3.5, 0.0), center + vec2(3.5, 0.0)],
                                            Stroke::new(1.5, palette.line_strong),
                                        );
                                    }
                                    _ => {
                                        painter.circle_stroke(
                                            center,
                                            6.5,
                                            Stroke::new(1.5, palette.line_strong),
                                        );
                                    }
                                }
                                let color = match item.status.as_str() {
                                    "completed" | "cancelled" => palette.ink_faint,
                                    _ => palette.ink,
                                };
                                let mut text = RichText::new(&item.content)
                                    .font(theme::text(theme::TEXT))
                                    .color(color);
                                if item.status == "cancelled" {
                                    text = text.strikethrough();
                                }
                                ui.add(egui::Label::new(text).wrap());
                                if item.priority == "high" && item.status != "completed" {
                                    ui.add(
                                        label(
                                            "High",
                                            theme::text(theme::SMALL - 1.0),
                                            palette.warn,
                                        )
                                        .selectable(false),
                                    );
                                }
                            });
                            ui.add_space(8.0);
                        }
                    }
                }
            });
    }

    // ---- Terminal -----------------------------------------------------------------------

    /// The terminal dock: a tab for each shell, + for another, a button that moves the dock
    /// between the bottom and the side, and close. Closing keeps the shells running.
    pub(crate) fn terminal_dock(&mut self, ui: &mut Ui, palette: &Palette) {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        let project = self.settings.project.clone();
        let mut select = None;
        let mut close_tab = None;
        let mut add = false;
        let mut hide = false;
        let mut flip = false;
        let bar = Frame::new()
            .inner_margin(Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (index, terminal) in self.terminals.iter().enumerate() {
                        let active = index == self.terminal_tab;
                        let mut text = terminal.label();
                        if terminal.exited() {
                            text.push_str(" · ended");
                        }
                        let galley = one_line(
                            ui,
                            &text,
                            theme::text(theme::SMALL + 0.5),
                            if active {
                                palette.ink
                            } else {
                                palette.ink_muted
                            },
                            170.0,
                        );
                        let width = 26.0 + galley.size().x + 24.0;
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
                        if active {
                            ui.painter().rect_filled(rect, radius(7), palette.hover);
                        } else if response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                radius(7),
                                palette.hover.gamma_multiply(0.6),
                            );
                        }
                        icons::paint(
                            ui.painter(),
                            Rect::from_center_size(
                                pos2(rect.left() + 14.0, rect.center().y),
                                vec2(13.0, 13.0),
                            ),
                            Icon::Terminal,
                            palette.ink_muted,
                        );
                        ui.painter().galley(
                            pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0),
                            galley,
                            palette.ink,
                        );
                        let cross = Rect::from_center_size(
                            pos2(rect.right() - 12.0, rect.center().y),
                            vec2(16.0, 16.0),
                        );
                        let over = ui
                            .ctx()
                            .pointer_hover_pos()
                            .is_some_and(|pointer| cross.contains(pointer));
                        if response.hovered() || active {
                            if over {
                                ui.painter().rect_filled(cross, radius(4), palette.line);
                            }
                            icons::paint(
                                ui.painter(),
                                cross.shrink(3.5),
                                Icon::Close,
                                palette.ink_faint,
                            );
                        }
                        if response.clicked() {
                            if over {
                                close_tab = Some(index);
                            } else {
                                select = Some(index);
                            }
                        }
                    }
                    if tool(ui, palette, Icon::Plus, "New terminal", project.is_some()) {
                        add = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if tool(ui, palette, Icon::Close, "Hide the terminal", true) {
                            hide = true;
                        }
                        let (icon, tip) = if self.settings.terminal_side {
                            (Icon::PanelBottom, "Move to the bottom")
                        } else {
                            (Icon::PanelSide, "Move to the side")
                        };
                        if tool(ui, palette, icon, tip, true) {
                            flip = true;
                        }
                    });
                });
            })
            .response
            .rect;
        ui.painter()
            .hline(bar.x_range(), bar.bottom(), Stroke::new(1.0, palette.line));
        if let Some(index) = select {
            self.terminal_tab = index;
            if let Some(terminal) = self.terminals.get_mut(index) {
                terminal.focus();
            }
        }
        if let Some(index) = close_tab {
            self.terminals.remove(index);
            self.terminal_tab = self
                .terminal_tab
                .min(self.terminals.len().saturating_sub(1));
            if self.terminals.is_empty() {
                self.terminal_open = false;
                self.terminal_focused = false;
                return;
            }
        }
        if hide {
            self.terminal_open = false;
            self.terminal_focused = false;
            return;
        }
        if flip {
            self.settings.terminal_side = !self.settings.terminal_side;
            self.save_settings();
        }
        let Some(project) = project else {
            Frame::new()
                .inner_margin(Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    note(ui, palette, "Choose a project folder first.");
                });
            self.terminal_focused = false;
            return;
        };
        if (add || self.terminals.is_empty()) && self.terminal_error.is_none() {
            match crate::terminal::Terminal::start(&project, ui.ctx()) {
                Ok(terminal) => {
                    self.terminals.push(terminal);
                    self.terminal_tab = self.terminals.len() - 1;
                }
                Err(error) => self.terminal_error = Some(error),
            }
        }
        if let Some(error) = self.terminal_error.clone() {
            Frame::new()
                .inner_margin(Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    problem(ui, palette, &error);
                    if ui.button("Try Again").clicked() {
                        self.terminal_error = None;
                    }
                });
            self.terminal_focused = false;
            return;
        }
        let index = self
            .terminal_tab
            .min(self.terminals.len().saturating_sub(1));
        if let Some(terminal) = self.terminals.get_mut(index) {
            self.terminal_focused = terminal.show(ui, palette);
        }
    }

    // ---- Changes ------------------------------------------------------------------------

    pub(crate) fn refresh_changes(&mut self) {
        let Some(project) = self.settings.project.clone() else {
            self.changes.list = Some(Err("Choose a project folder to see its changes.".into()));
            return;
        };
        if self.changes.project.as_ref() != Some(&project) {
            self.changes = Changes {
                project: Some(project.clone()),
                ..Default::default()
            };
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(list_changes(&project));
        });
        self.changes.task = Some(receiver);
    }

    pub(crate) fn poll_changes(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.changes.task {
            match receiver.try_recv() {
                Ok(result) => {
                    self.changes.task = None;
                    // A diff that is open is read again, since the file may have changed.
                    if let (Ok(list), Some(selected)) = (&result, self.changes.selected.clone()) {
                        match list.iter().find(|changed| changed.path == selected) {
                            Some(changed) => self.load_diff(changed.clone()),
                            None => {
                                self.changes.selected = None;
                                self.changes.diff = None;
                            }
                        }
                    }
                    self.changes.list = Some(result);
                    ctx.request_repaint();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(150))
                }
                Err(mpsc::TryRecvError::Disconnected) => self.changes.task = None,
            }
        }
        if let Some(receiver) = &self.changes.diff_task {
            match receiver.try_recv() {
                Ok(result) => {
                    self.changes.diff_task = None;
                    self.changes.diff = Some(result);
                    ctx.request_repaint();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(150))
                }
                Err(mpsc::TryRecvError::Disconnected) => self.changes.diff_task = None,
            }
        }
    }

    fn load_diff(&mut self, changed: Changed) {
        let Some(project) = self.changes.project.clone() else {
            return;
        };
        self.changes.selected = Some(changed.path.clone());
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(file_diff(&project, &changed));
        });
        self.changes.diff_task = Some(receiver);
    }

    fn changes_pane(&mut self, ui: &mut Ui, palette: &Palette) {
        let mut open = None;
        let mut back = false;
        let mut refresh = false;
        Frame::new()
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.set_min_width(ui.available_width());
                if let Some(selected) = self.changes.selected.clone() {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if tool(ui, palette, Icon::Back, "All changes", true) {
                            back = true;
                        }
                        ui.add(
                            label(selected, theme::mono(theme::CODE), palette.ink)
                                .selectable(true)
                                .truncate(),
                        );
                    });
                    ui.add_space(8.0);
                    match &self.changes.diff {
                        None => note(ui, palette, "Reading the diff…"),
                        Some(Err(error)) => problem(ui, palette, error),
                        Some(Ok(diff)) => diff_view(ui, palette, diff),
                    }
                    return;
                }
                let summary = match &self.changes.list {
                    Some(Ok(list)) if !list.is_empty() => {
                        let added: u64 = list.iter().filter_map(|changed| changed.added).sum();
                        let removed: u64 = list.iter().filter_map(|changed| changed.removed).sum();
                        format!(
                            "{} {} · +{added} −{removed}",
                            list.len(),
                            if list.len() == 1 { "file" } else { "files" }
                        )
                    }
                    Some(Ok(_)) => "No changes".to_owned(),
                    _ => String::new(),
                };
                ui.horizontal(|ui| {
                    ui.add(
                        label(summary, theme::text(theme::SMALL), palette.ink_muted)
                            .selectable(false),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if tool(
                            ui,
                            palette,
                            Icon::Reload,
                            "Check again",
                            self.changes.task.is_none(),
                        ) {
                            refresh = true;
                        }
                    });
                });
                ui.add_space(6.0);
                match &self.changes.list {
                    None => note(ui, palette, "Reading the project’s changes…"),
                    Some(Err(error)) => note(ui, palette, error),
                    Some(Ok(list)) if list.is_empty() => {
                        note(ui, palette, "Nothing has changed since the last commit.")
                    }
                    Some(Ok(list)) => {
                        ScrollArea::vertical().id_salt("changes").show(ui, |ui| {
                            for changed in list {
                                let (rect, response) = ui.allocate_exact_size(
                                    vec2(ui.available_width(), 30.0),
                                    Sense::click(),
                                );
                                if response.hovered() {
                                    ui.painter().rect_filled(rect, radius(6), palette.hover);
                                }
                                let numbers = match (changed.added, changed.removed) {
                                    (Some(added), Some(removed)) => format!("+{added} −{removed}"),
                                    _ => "binary".to_owned(),
                                };
                                let numbers_galley = ui.painter().layout_no_wrap(
                                    numbers,
                                    theme::mono(theme::CODE - 1.0),
                                    palette.ink_faint,
                                );
                                let kind_color = match changed.describe() {
                                    "Added" => palette.ok,
                                    "Deleted" => palette.danger,
                                    _ => palette.warn,
                                };
                                let letter =
                                    changed.describe().chars().next().unwrap_or('M').to_string();
                                ui.painter().text(
                                    pos2(rect.left() + 10.0, rect.center().y),
                                    Align2::LEFT_CENTER,
                                    letter,
                                    theme::strong(theme::SMALL),
                                    kind_color,
                                );
                                let room = rect.width() - 34.0 - numbers_galley.size().x - 16.0;
                                let path = one_line(
                                    ui,
                                    &changed.path,
                                    theme::mono(theme::CODE),
                                    palette.ink,
                                    room,
                                );
                                ui.painter().galley(
                                    pos2(rect.left() + 28.0, rect.center().y - path.size().y / 2.0),
                                    path,
                                    palette.ink,
                                );
                                let numbers_x = rect.right() - 8.0 - numbers_galley.size().x;
                                ui.painter().galley(
                                    pos2(
                                        numbers_x,
                                        rect.center().y - numbers_galley.size().y / 2.0,
                                    ),
                                    numbers_galley,
                                    palette.ink_faint,
                                );
                                if response.on_hover_text(changed.describe()).clicked() {
                                    open = Some(changed.clone());
                                }
                            }
                        });
                    }
                }
            });
        if back {
            self.changes.selected = None;
            self.changes.diff = None;
        }
        if refresh {
            self.refresh_changes();
        }
        if let Some(changed) = open {
            self.changes.diff = None;
            self.load_diff(changed);
        }
    }

    // ---- Files --------------------------------------------------------------------------

    fn files_pane(&mut self, ui: &mut Ui, palette: &Palette) {
        let Some(root) = self.settings.project.clone() else {
            Frame::new()
                .inner_margin(Margin::symmetric(16, 14))
                .show(ui, |ui| {
                    note(ui, palette, "Choose a project folder to see its files.");
                });
            return;
        };
        if self.files.root.as_ref() != Some(&root) {
            self.files = Files {
                root: Some(root.clone()),
                ..Default::default()
            };
        }
        let mut attach = None;
        let mut close_preview = false;
        let mut refresh = false;
        let mut open_in_browser = None;
        Frame::new()
            .inner_margin(Margin::symmetric(10, 10))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.set_min_width(ui.available_width());
                if let Some((path, shown)) = &self.files.open {
                    let name = path.strip_prefix(&root).map_or_else(
                        |_| path.display().to_string(),
                        |inside| inside.display().to_string(),
                    );
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if tool(ui, palette, Icon::Back, "All files", true) {
                            close_preview = true;
                        }
                        ui.add(label(name, theme::mono(theme::CODE), palette.ink).truncate());
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if ui.small_button("Attach to Prompt").clicked() {
                            attach = Some(path.clone());
                        }
                        if ui.small_button("Open").clicked() {
                            let _ = open_path(path);
                        }
                        let html = path
                            .extension()
                            .is_some_and(|extension| extension == "html" || extension == "htm");
                        if html
                            && browser::SUPPORTED
                            && ui.small_button("View in Browser").clicked()
                        {
                            open_in_browser = Some(path.clone());
                        }
                    });
                    ui.add_space(10.0);
                    match shown {
                        Preview::Text { text, cut } => {
                            code_view(ui, palette, text, true);
                            if *cut {
                                ui.add_space(6.0);
                                note(ui, palette, "Showing the first 256 KB.");
                            }
                        }
                        Preview::Image => {
                            let ctx = ui.ctx().clone();
                            match self.inputs.thumbnail(&ctx, path) {
                                Some(texture) => {
                                    let size = texture.size_vec2();
                                    let scale = (ui.available_width() / size.x).min(1.0) * 2.0;
                                    ui.image((texture.id(), size * scale.min(2.0)));
                                }
                                None => note(ui, palette, "Loading the picture…"),
                            }
                        }
                        Preview::Other(message) => note(ui, palette, message),
                    }
                    return;
                }
                ui.horizontal(|ui| {
                    ui.add(
                        label(tilde(&root), theme::text(theme::SMALL), palette.ink_muted)
                            .truncate(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if tool(ui, palette, Icon::Reload, "Read the folders again", true) {
                            refresh = true;
                        }
                    });
                });
                ui.add_space(4.0);
                let mut toggle = None;
                let mut choose = None;
                ScrollArea::vertical().id_salt("files").show(ui, |ui| {
                    self.file_rows(ui, palette, &root, 0, &mut toggle, &mut choose);
                });
                if let Some(folder) = toggle
                    && !self.files.expanded.remove(&folder)
                {
                    self.files.expanded.insert(folder);
                }
                if let Some(file) = choose {
                    let shown = preview(&file);
                    self.files.open = Some((file, shown));
                }
            });
        if close_preview {
            self.files.open = None;
        }
        if refresh {
            self.files.listed.clear();
        }
        if let Some(path) = attach {
            let ctx = ui.ctx().clone();
            self.attach(vec![path], &ctx);
        }
        if let Some(path) = open_in_browser {
            self.open_file_in_browser(&path);
        }
    }

    fn file_rows(
        &mut self,
        ui: &mut Ui,
        palette: &Palette,
        folder: &Path,
        depth: usize,
        toggle: &mut Option<PathBuf>,
        choose: &mut Option<PathBuf>,
    ) {
        if depth > 24 {
            return;
        }
        let entries = self
            .files
            .listed
            .entry(folder.to_path_buf())
            .or_insert_with(|| list_folder(folder))
            .clone();
        let entries = match entries {
            Ok(entries) => entries,
            Err(error) => {
                ui.add(label(error, theme::text(theme::SMALL), palette.danger).wrap());
                return;
            }
        };
        if entries.is_empty() && depth > 0 {
            ui.horizontal(|ui| {
                ui.add_space(12.0 + depth as f32 * 14.0 + 20.0);
                ui.add(
                    label("Empty", theme::text(theme::SMALL), palette.ink_faint).selectable(false),
                );
            });
        }
        for entry in entries {
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, radius(5), palette.hover);
            }
            let indent = rect.left() + 6.0 + depth as f32 * 14.0;
            let expanded = self.files.expanded.contains(&entry.path);
            if entry.folder {
                icons::paint(
                    ui.painter(),
                    Rect::from_center_size(pos2(indent + 6.0, rect.center().y), vec2(10.0, 10.0)),
                    if expanded {
                        Icon::ChevronDown
                    } else {
                        Icon::Forward
                    },
                    palette.ink_faint,
                );
            }
            let icon = if entry.folder {
                Icon::Folder
            } else if matches!(
                entry
                    .path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .as_deref(),
                Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp")
            ) {
                Icon::Image
            } else {
                Icon::File
            };
            icons::paint(
                ui.painter(),
                Rect::from_center_size(pos2(indent + 22.0, rect.center().y), vec2(14.0, 14.0)),
                icon,
                if entry.folder {
                    palette.accent
                } else {
                    palette.ink_muted
                },
            );
            let name = one_line(
                ui,
                &entry.name,
                theme::text(theme::TEXT - 0.5),
                palette.ink,
                rect.right() - indent - 40.0,
            );
            ui.painter().galley(
                pos2(indent + 34.0, rect.center().y - name.size().y / 2.0),
                name,
                palette.ink,
            );
            if response.clicked() {
                if entry.folder {
                    *toggle = Some(entry.path.clone());
                } else {
                    *choose = Some(entry.path.clone());
                }
            }
            if entry.folder && expanded {
                self.file_rows(ui, palette, &entry.path, depth + 1, toggle, choose);
            }
        }
    }
}

/// A diff with added lines on green, removed lines on red, and hunk headers quieter.
fn diff_view(ui: &mut Ui, palette: &Palette, diff: &str) {
    let lines: Vec<&str> = diff.lines().collect();
    let row = 18.0;
    let font = theme::mono(theme::CODE - 0.5);
    let added = palette.ok.gamma_multiply(0.14);
    let removed = palette.danger.gamma_multiply(0.14);
    ScrollArea::both()
        .id_salt("diff")
        .auto_shrink([false, false])
        .show_rows(ui, row, lines.len(), |ui, range| {
            for line in &lines[range] {
                let (background, color) = if line.starts_with("+++") || line.starts_with("---") {
                    (Color32::TRANSPARENT, palette.ink_faint)
                } else if line.starts_with('+') {
                    (added, palette.ink)
                } else if line.starts_with('-') {
                    (removed, palette.ink)
                } else if line.starts_with("@@") {
                    (palette.accent_wash, palette.accent)
                } else if line.starts_with("diff ") || line.starts_with("index ") {
                    (Color32::TRANSPARENT, palette.ink_faint)
                } else {
                    (Color32::TRANSPARENT, palette.ink_muted)
                };
                let galley =
                    ui.painter()
                        .layout_no_wrap(line.replace('\t', "    "), font.clone(), color);
                let width = (galley.size().x + 16.0).max(ui.available_width());
                let (rect, _) = ui.allocate_exact_size(vec2(width, row), Sense::hover());
                if background != Color32::TRANSPARENT {
                    ui.painter().rect_filled(rect, 0.0, background);
                }
                ui.painter().galley(
                    pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0),
                    galley,
                    color,
                );
            }
        });
}

/// A file's text with line numbers, scrolling both ways.
fn code_view(ui: &mut Ui, palette: &Palette, text: &str, numbers: bool) {
    let lines: Vec<&str> = text.lines().collect();
    let row = 18.0;
    let font = theme::mono(theme::CODE - 0.5);
    let gutter = if numbers {
        8.0 + lines.len().to_string().len() as f32 * 7.5 + 10.0
    } else {
        6.0
    };
    ScrollArea::both()
        .id_salt("file-preview")
        .auto_shrink([false, false])
        .show_rows(ui, row, lines.len(), |ui, range| {
            for index in range {
                let line = lines[index].replace('\t', "    ");
                let galley = ui.painter().layout_no_wrap(line, font.clone(), palette.ink);
                let width = (gutter + galley.size().x + 16.0).max(ui.available_width());
                let (rect, _) = ui.allocate_exact_size(vec2(width, row), Sense::hover());
                if numbers {
                    ui.painter().text(
                        pos2(rect.left() + gutter - 10.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        (index + 1).to_string(),
                        font.clone(),
                        palette.ink_faint,
                    );
                }
                ui.painter().galley(
                    pos2(
                        rect.left() + gutter,
                        rect.center().y - galley.size().y / 2.0,
                    ),
                    galley,
                    palette.ink,
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_status_and_numbers_are_read() {
        let status = " M src/main.rs\0?? notes.md\0R  new.rs\0old.rs\0D  gone.rs\0";
        assert_eq!(
            parse_status(status),
            vec![
                ("M".to_owned(), "src/main.rs".to_owned()),
                ("??".to_owned(), "notes.md".to_owned()),
                ("R".to_owned(), "new.rs".to_owned()),
                ("D".to_owned(), "gone.rs".to_owned()),
            ]
        );
        let counts =
            parse_numstat("3\t1\tsrc/main.rs\n-\t-\tlogo.png\n2\t0\tsrc/{old => new}.rs\n");
        assert_eq!(counts["src/main.rs"], (Some(3), Some(1)));
        assert_eq!(counts["logo.png"], (None, None));
        assert_eq!(counts["src/new.rs"], (Some(2), Some(0)));
    }

    #[test]
    fn a_real_repository_lists_its_changes() {
        let folder = tempfile::tempdir().unwrap();
        let project = folder.path();
        if run_git(project, &["init", "-q"]).is_err() {
            return; // Git isn't installed here.
        }
        std::fs::write(project.join("kept.txt"), "one\ntwo\n").unwrap();
        let _ = run_git(project, &["add", "."]);
        let _ = run_git(
            project,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "start",
            ],
        );
        std::fs::write(project.join("kept.txt"), "one\nthree\nfour\n").unwrap();
        std::fs::write(project.join("new.txt"), "a\nb\n").unwrap();
        let changes = list_changes(project).unwrap();
        assert_eq!(changes.len(), 2);
        let kept = changes
            .iter()
            .find(|changed| changed.path == "kept.txt")
            .unwrap();
        assert_eq!((kept.added, kept.removed), (Some(2), Some(1)));
        let new = changes
            .iter()
            .find(|changed| changed.path == "new.txt")
            .unwrap();
        assert_eq!((new.status.as_str(), new.added), ("??", Some(2)));
        assert!(file_diff(project, kept).unwrap().contains("+three"));
        assert!(file_diff(project, new).unwrap().contains("+a"));
        let plain = tempfile::tempdir().unwrap();
        assert!(
            list_changes(plain.path())
                .unwrap_err()
                .contains("Not a Git repository")
        );
    }

    #[test]
    fn folders_list_first_without_git_internals() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::create_dir(folder.path().join(".git")).unwrap();
        std::fs::create_dir(folder.path().join("src")).unwrap();
        std::fs::write(folder.path().join("README.md"), "hi").unwrap();
        let names: Vec<String> = list_folder(folder.path())
            .unwrap()
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names, ["src", "README.md"]);
        assert!(matches!(
            preview(&folder.path().join("README.md")),
            Preview::Text { .. }
        ));
    }
}
