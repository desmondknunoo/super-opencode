//! The box where a task is written, after Codex's and Claude Code's: the project above it,
//! attachments as cards, then the text, and a toolbar with Add, access, the model and its
//! effort, dictation, and Run. Under it sit the rate-limit fallback and the context ring.

use std::time::Duration;

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Frame, Layout, Margin, Rect, RectAlign, Response,
    RichText, ScrollArea, Sense, Shadow, Stroke, StrokeKind, TextEdit, Ui, WidgetInfo, WidgetType,
    text::{LayoutJob, TextWrapping},
    vec2,
};
use uuid::Uuid;

use crate::{
    app::{App, View},
    attach::{self, Kind},
    icons::{self, Icon},
    inputs::{Dictation, ModelState, Prep},
    models, providers, speech,
    theme::{self, Palette},
    views::{breath, checkbox, label, one_line, open_path, radius, round_button},
};

const RETURN: &str = if cfg!(target_os = "macos") {
    "Return"
} else {
    "Enter"
};
const MODIFIER: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl+"
};

/// Side of an attachment card in the composer, and under a sent prompt.
pub(crate) const CARD: f32 = 104.0;
pub(crate) const SENT_CARD: f32 = 84.0;

pub(crate) fn kind_icon(kind: Kind) -> Icon {
    match kind {
        Kind::Text | Kind::Document => Icon::File,
        Kind::Image => Icon::Image,
        Kind::Video => Icon::Film,
        Kind::Audio => Icon::Wave,
        Kind::Folder => Icon::Folder,
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tone {
    Quiet,
    Busy,
    Warn,
    Danger,
}

pub(crate) struct Card<'a> {
    pub kind: Kind,
    pub name: &'a str,
    /// "PDF", "PNG", "Folder".
    pub type_label: &'a str,
    pub note: &'a str,
    pub tone: Tone,
    pub preview: Option<&'a egui::TextureHandle>,
}

#[derive(Default)]
pub(crate) struct CardClicks {
    pub open: bool,
    pub remove: bool,
}

/// A square attachment card: the image itself for pictures, otherwise its icon, name, and what
/// was done with it. Clicking opens it; the remove button appears on hover when `removable`.
pub(crate) fn attachment_card(
    ui: &mut Ui,
    palette: &Palette,
    side: f32,
    card: Card,
    ground: Color32,
    removable: bool,
) -> CardClicks {
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Button, true, format!("Open {}", card.name))
    });
    let mut clicks = CardClicks::default();
    if !ui.is_rect_visible(rect) {
        return clicks;
    }
    let corner = radius(10);
    let painter = ui.painter();
    let note_color = match card.tone {
        Tone::Quiet => palette.ink_faint,
        Tone::Busy => palette.accent,
        Tone::Warn => palette.warn,
        Tone::Danger => palette.danger,
    };
    match card.preview {
        Some(texture) => {
            // Crop the picture to a square from its middle, as a photo grid does.
            let [width, height] = texture.size().map(|side| side as f32);
            let uv = if width > height {
                let inset = (1.0 - height / width) / 2.0;
                Rect::from_min_max(egui::pos2(inset, 0.0), egui::pos2(1.0 - inset, 1.0))
            } else {
                let inset = (1.0 - width / height) / 2.0;
                Rect::from_min_max(egui::pos2(0.0, inset), egui::pos2(1.0, 1.0 - inset))
            };
            painter.rect_filled(rect, corner, palette.code_ground);
            egui::Image::new((texture.id(), rect.size()))
                .uv(uv)
                .corner_radius(corner)
                .paint_at(ui, rect);
            if card.tone != Tone::Quiet {
                // A problem with a picture is said on a strip across its foot.
                let strip =
                    Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - 24.0), rect.max);
                ui.painter().rect_filled(
                    strip,
                    CornerRadius {
                        nw: 0,
                        ne: 0,
                        sw: 10,
                        se: 10,
                    },
                    palette.raised.gamma_multiply(0.94),
                );
                let galley = one_line(
                    ui,
                    card.note,
                    theme::text(theme::SMALL),
                    note_color,
                    side - 16.0,
                );
                ui.painter().galley(
                    egui::pos2(strip.left() + 8.0, strip.center().y - galley.size().y / 2.0),
                    galley,
                    note_color,
                );
            }
        }
        None => {
            painter.rect_filled(rect, corner, ground);
            icons::paint(
                painter,
                Rect::from_min_size(rect.min + vec2(11.0, 11.0), vec2(17.0, 17.0)),
                kind_icon(card.kind),
                palette.ink_muted,
            );
            let kind_label = painter.layout_no_wrap(
                card.type_label.to_owned(),
                theme::strong(theme::SMALL - 1.5),
                palette.ink_faint,
            );
            painter.galley(
                egui::pos2(rect.right() - 10.0 - kind_label.size().x, rect.top() + 12.0),
                kind_label,
                palette.ink_faint,
            );
            let mut job = LayoutJob::simple(
                card.name.to_owned(),
                theme::strong(theme::SMALL),
                palette.ink,
                side - 20.0,
            );
            job.wrap = TextWrapping {
                max_width: side - 20.0,
                max_rows: 2,
                // Prefer breaking a file name after a dash or dot, as in "webhook-" "suite.log".
                break_anywhere: false,
                overflow_character: Some('…'),
            };
            let name = ui.painter().layout_job(job);
            let note = one_line(
                ui,
                card.note,
                theme::text(theme::SMALL - 0.5),
                note_color,
                side - 20.0,
            );
            let note_top = rect.bottom() - 10.0 - note.size().y;
            ui.painter().galley(
                egui::pos2(rect.left() + 10.0, note_top - 3.0 - name.size().y),
                name,
                palette.ink,
            );
            ui.painter()
                .galley(egui::pos2(rect.left() + 10.0, note_top), note, note_color);
        }
    }
    let border = match card.tone {
        Tone::Warn => Stroke::new(1.5, palette.warn),
        Tone::Danger => Stroke::new(1.5, palette.danger),
        _ if response.hovered() => Stroke::new(1.0, palette.line_strong),
        _ => Stroke::new(1.0, palette.line),
    };
    ui.painter()
        .rect_stroke(rect, corner, border, StrokeKind::Inside);
    if card.tone == Tone::Busy {
        ui.painter()
            .rect_filled(rect, corner, palette.raised.gamma_multiply(0.55));
        egui::Spinner::new()
            .size(18.0)
            .color(palette.accent)
            .paint_at(ui, Rect::from_center_size(rect.center(), vec2(18.0, 18.0)));
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }
    if response.has_focus() {
        icons::focus_ring(ui.painter(), rect, 10, palette.accent);
    }

    if removable {
        let close = Rect::from_center_size(rect.right_top() + vec2(-14.0, 14.0), vec2(22.0, 22.0));
        let remove = ui.interact(close, response.id.with("remove"), Sense::click());
        remove.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, true, format!("Remove {}", card.name))
        });
        if response.hovered() || remove.hovered() || remove.has_focus() {
            let painter = ui.painter();
            painter.circle_filled(close.center(), 10.0, palette.ink.gamma_multiply(0.85));
            if remove.has_focus() {
                painter.circle_stroke(close.center(), 11.5, Stroke::new(1.5, palette.accent));
            }
            icons::paint(
                painter,
                Rect::from_center_size(close.center(), vec2(9.0, 9.0)),
                Icon::Close,
                palette.ground,
            );
        }
        clicks.remove = remove.on_hover_text("Remove").clicked();
    }
    let tip = if card.note.is_empty() {
        card.name.to_owned()
    } else {
        format!("{}\n{}", card.name, card.note)
    };
    clicks.open = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tip)
        .clicked()
        && !clicks.remove;
    clicks
}

/// A square icon button for the composer's tools.
fn tool_button(ui: &mut Ui, palette: &Palette, icon: Icon, name: &str, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name));
    let painter = ui.painter();
    if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
        painter.rect_filled(rect, radius(theme::RADIUS_CONTROL), palette.hover);
    }
    if response.has_focus() {
        icons::focus_ring(painter, rect, theme::RADIUS_CONTROL, palette.accent);
    }
    let ink = if enabled {
        palette.ink_muted
    } else {
        palette.ink_faint.gamma_multiply(0.6)
    };
    icons::paint(
        painter,
        Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
        icon,
        ink,
    );
    response.on_hover_text(name)
}

/// A text button in the toolbar: optional icon, words, optional quieter words, a chevron when
/// it opens a menu.
#[allow(clippy::too_many_arguments)]
fn toolbar_chip(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    text: &str,
    detail: &str,
    ink: Color32,
    menu: bool,
    enabled: bool,
) -> Response {
    let font = theme::text(theme::TEXT - 0.5);
    let main = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), ink);
    let extra = (!detail.is_empty()).then(|| {
        ui.painter()
            .layout_no_wrap(detail.to_owned(), font, palette.ink_faint)
    });
    let width = 10.0
        + icon.map_or(0.0, |_| 21.0)
        + main.size().x
        + extra.as_ref().map_or(0.0, |galley| 6.0 + galley.size().x)
        + if menu { 18.0 } else { 0.0 }
        + 10.0;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, text));
    let painter = ui.painter();
    if enabled && (response.hovered() || response.is_pointer_button_down_on()) {
        painter.rect_filled(rect, radius(theme::RADIUS_CONTROL + 2), palette.hover);
    }
    if response.has_focus() {
        icons::focus_ring(painter, rect, theme::RADIUS_CONTROL + 2, palette.accent);
    }
    let fade = if enabled { 1.0 } else { 0.55 };
    let mut x = rect.left() + 10.0;
    if let Some(icon) = icon {
        icons::paint(
            painter,
            Rect::from_center_size(egui::pos2(x + 7.5, rect.center().y), vec2(15.0, 15.0)),
            icon,
            ink.gamma_multiply(fade),
        );
        x += 21.0;
    }
    let main_width = main.size().x;
    painter.galley(
        egui::pos2(x, rect.center().y - main.size().y / 2.0),
        main,
        ink.gamma_multiply(fade),
    );
    x += main_width;
    if let Some(galley) = extra {
        x += 6.0;
        let extra_width = galley.size().x;
        painter.galley(
            egui::pos2(x, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.ink_faint,
        );
        x += extra_width;
    }
    if menu {
        icons::paint(
            painter,
            Rect::from_center_size(egui::pos2(x + 10.0, rect.center().y), vec2(11.0, 11.0)),
            Icon::ChevronDown,
            palette.ink_faint.gamma_multiply(fade),
        );
    }
    response
}

/// One choice in a menu: icon, title, a line about it, and a check when it is the current one.
fn menu_row(
    ui: &mut Ui,
    palette: &Palette,
    icon: Icon,
    title: &str,
    detail: &str,
    selected: bool,
    danger: bool,
) -> Response {
    let width = ui.available_width();
    let title_ink = if danger { palette.danger } else { palette.ink };
    let detail_ink = if danger {
        palette.danger.gamma_multiply(0.85)
    } else {
        palette.ink_faint
    };
    let mut job = LayoutJob::simple(
        detail.to_owned(),
        theme::text(theme::SMALL),
        detail_ink,
        width - 66.0,
    );
    job.wrap.max_rows = 3;
    let detail_galley = (!detail.is_empty()).then(|| ui.painter().layout_job(job));
    let height = 12.0 + 18.0 + detail_galley.as_ref().map_or(0.0, |g| g.size().y + 1.0) + 10.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, title));
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, radius(theme::RADIUS_CONTROL), palette.hover);
    }
    if response.has_focus() {
        icons::focus_ring(painter, rect, theme::RADIUS_CONTROL, palette.accent);
    }
    icons::paint(
        painter,
        Rect::from_min_size(rect.min + vec2(10.0, 13.0), vec2(15.0, 15.0)),
        icon,
        if danger {
            palette.danger
        } else {
            palette.ink_muted
        },
    );
    painter.text(
        rect.min + vec2(34.0, 12.0),
        Align2::LEFT_TOP,
        title,
        theme::text(theme::TEXT),
        title_ink,
    );
    if let Some(galley) = detail_galley {
        painter.galley(rect.min + vec2(34.0, 31.0), galley, detail_ink);
    }
    if selected {
        icons::paint(
            painter,
            Rect::from_center_size(
                egui::pos2(rect.right() - 18.0, rect.top() + 20.0),
                vec2(14.0, 14.0),
            ),
            Icon::Check,
            palette.accent,
        );
    }
    response
}

fn menu_heading(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.add(label(text, theme::text(theme::SMALL), palette.ink_faint).selectable(false));
    });
    ui.add_space(4.0);
}

/// 48213 as 48.2K, and 200000 as 200K.
pub(crate) fn tokens_short(count: u64) -> String {
    let (value, unit) = match count {
        0..=999 => return count.to_string(),
        1_000..=999_999 => (count as f64 / 1_000.0, "K"),
        _ => (count as f64 / 1_000_000.0, "M"),
    };
    let text = format!("{value:.1}");
    format!("{}{unit}", text.strip_suffix(".0").unwrap_or(&text))
}

/// How full a context window is: green-free accent until 80%, then amber, then red past 95%.
fn fill_color(palette: &Palette, fraction: f32) -> Color32 {
    if fraction >= 0.95 {
        palette.danger
    } else if fraction >= 0.8 {
        palette.warn
    } else {
        palette.accent
    }
}

fn minutes(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// What the context ring reports.
pub(crate) struct Context {
    pub used: u64,
    pub limit: u64,
    pub model: String,
}

impl Context {
    pub fn fraction(&self) -> Option<f32> {
        (self.limit > 0).then(|| (self.used as f32 / self.limit as f32).min(1.0))
    }

    pub fn left(&self) -> u64 {
        self.limit.saturating_sub(self.used)
    }
}

enum Add {
    Files,
    Folder,
    Project,
    Plan,
}

impl App {
    /// The current session's context against the selected model's window.
    pub(crate) fn context(&self) -> Context {
        let model = models::at(self.route);
        Context {
            used: self
                .active
                .as_ref()
                .map_or(0, |session| session.context_tokens),
            limit: u64::from(model.context),
            model: model.name,
        }
    }

    pub(crate) fn composer(&mut self, ui: &mut Ui, palette: &Palette) {
        ui.spacing_mut().item_spacing.y = 0.0;
        // A narrow window, or one sharing its width with the browser, drops the controls'
        // labels so they don't overlap.
        self.narrow = ui.available_width() < 600.0;
        let has_project = self.settings.project.is_some();
        let running = self.is_running();
        let with_modifier = self.settings.send_with_modifier;

        self.project_bar(ui, palette, running);

        let frame = Frame::new()
            .fill(palette.raised)
            .stroke(Stroke::new(1.0, palette.line_strong))
            .corner_radius(radius(16))
            .shadow(Shadow {
                offset: [0, 3],
                blur: 14,
                spread: 0,
                color: palette.shadow.gamma_multiply(0.55),
            })
            .inner_margin(Margin {
                left: 12,
                right: 10,
                top: 12,
                bottom: 8,
            });
        let mut submit = false;
        let shown = frame.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            self.queued_rows(ui, palette);
            self.speech_model_panel(ui, palette);
            self.attachment_cards(ui, palette);
            self.attachment_notes(ui, palette);

            // By default Return sends and Shift+Return adds a new line. With the setting on,
            // Command+Return (Ctrl+Enter) sends and Return adds a new line.
            // While a task runs, the send keys add to it instead: the send key gives the
            // message to the task now, and the queue key (⌘Return, or ⇧⌘Return when ⌘Return
            // sends) keeps it for when the task finishes.
            let task = egui::Id::new("composer-task");
            let (send_key, queue_key) = if ui.memory(|memory| memory.has_focus(task)) {
                ui.input_mut(|input| {
                    let (mut send, mut queue) = (false, false);
                    input.events.retain(|event| match event {
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            pressed: true,
                            modifiers,
                            ..
                        } => {
                            let sends = if with_modifier {
                                modifiers.command && !modifiers.shift
                            } else {
                                !modifiers.shift && !modifiers.alt && !modifiers.command
                            };
                            let queues = if with_modifier {
                                modifiers.command && modifiers.shift
                            } else {
                                modifiers.command && !modifiers.shift
                            };
                            send |= sends;
                            queue |= queues;
                            !(sends || queues)
                        }
                        _ => true,
                    });
                    (send, queue)
                })
            } else {
                (false, false)
            };
            let has_text = !self.prompt.trim().is_empty();
            if running && has_text {
                if send_key {
                    self.send_now(ui.ctx());
                } else if queue_key {
                    self.queue_prompt();
                }
            }
            let newline = if with_modifier {
                egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::Enter)
            } else {
                egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, egui::Key::Enter)
            };
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                let editor = ScrollArea::vertical()
                    .id_salt("composer-text")
                    .max_height(168.0)
                    .show(ui, |ui| {
                        ui.add_enabled(
                            has_project,
                            TextEdit::multiline(&mut self.prompt)
                                .id(task)
                                .return_key(newline)
                                .hint_text(
                                    RichText::new(if !has_project {
                                        "Choose a project folder to start".to_owned()
                                    } else if running {
                                        let (now, later) = if with_modifier {
                                            (
                                                format!("{MODIFIER}{RETURN}"),
                                                format!("⇧{MODIFIER}{RETURN}"),
                                            )
                                        } else {
                                            (RETURN.to_owned(), format!("{MODIFIER}{RETURN}"))
                                        };
                                        format!("Add to the task: {now} now, {later} after")
                                    } else {
                                        "Describe a coding task".to_owned()
                                    })
                                    .color(palette.ink_faint),
                                )
                                .font(theme::text(theme::READING))
                                .text_color(palette.ink)
                                .frame(Frame::NONE)
                                .margin(Margin::ZERO)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        )
                    })
                    .inner;
                if self.focus_composer && has_project {
                    editor.request_focus();
                    self.focus_composer = false;
                }
            });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                self.left_tools(ui, palette, has_project && !running);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if running {
                        let stop = round_button(
                            ui,
                            palette,
                            Icon::Stop,
                            "Stop (Esc)",
                            palette.ink,
                            palette.ground,
                            true,
                        );
                        if stop.clicked() {
                            self.stop();
                        }
                        if has_text {
                            let now = round_button(
                                ui,
                                palette,
                                Icon::ArrowUp,
                                "Send now: the task reads it at its next step",
                                palette.accent,
                                palette.on_accent,
                                true,
                            );
                            if now.clicked() {
                                self.send_now(ui.ctx());
                            }
                            let queue = ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Queue")
                                            .font(theme::text(theme::SMALL))
                                            .color(palette.ink_muted),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text("Run it when the task finishes");
                            if queue.clicked() {
                                self.queue_prompt();
                            }
                        }
                    } else {
                        let (run, newline) = if with_modifier {
                            (
                                format!("{MODIFIER}{RETURN}"),
                                format!("{RETURN} adds a new line"),
                            )
                        } else {
                            (RETURN.to_owned(), format!("Shift+{RETURN} adds a new line"))
                        };
                        let send = round_button(
                            ui,
                            palette,
                            Icon::ArrowUp,
                            &format!("Run task ({run}). {newline}."),
                            palette.accent,
                            palette.on_accent,
                            self.can_submit(),
                        );
                        submit = send.clicked() || ((send_key || queue_key) && self.can_submit());
                    }
                    ui.add_space(2.0);
                    self.mic_button(ui, palette, has_project && !running);
                    self.model_chip(ui, palette, !running);
                });
            });
        });

        // Files dragged over the window land in the composer.
        if self.inputs.dropping {
            let rect = shown.response.rect;
            let painter = ui.painter();
            painter.rect_filled(rect, radius(16), palette.accent_wash.gamma_multiply(0.92));
            painter.rect_stroke(
                rect,
                radius(16),
                Stroke::new(1.5, palette.accent),
                StrokeKind::Inside,
            );
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                if has_project {
                    "Drop to attach"
                } else {
                    "Choose a project folder first"
                },
                theme::strong(theme::READING),
                palette.accent,
            );
        }

        // Under the box: where a rate-limited task may continue, and how full the context is.
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let ring_room = 132.0;
            let left = (ui.available_width() - ring_room).max(0.0);
            ui.allocate_ui_with_layout(
                vec2(left, 26.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.add_space(4.0);
                    self.fallback_line(ui, palette, running);
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                self.context_ring(ui, palette);
            });
        });

        if submit {
            self.submit(ui.ctx());
        }
    }

    /// Messages waiting for this session's task, each with Send now and remove.
    fn queued_rows(&mut self, ui: &mut Ui, palette: &Palette) {
        let Some(id) = self.active.as_ref().map(|session| session.id) else {
            return;
        };
        let Some(items) = self.queued.get(&id).filter(|items| !items.is_empty()) else {
            return;
        };
        let held = self.held.contains(&id);
        let rows: Vec<(String, usize)> = items
            .iter()
            .map(|item| (item.prompt.clone(), item.attachments.len()))
            .collect();
        let mut send = None;
        let mut remove = None;
        for (index, (prompt, files)) in rows.iter().enumerate() {
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
            ui.painter()
                .rect_filled(rect, radius(8), palette.hover.gamma_multiply(0.7));
            let tag = if held { "Waiting" } else { "Queued" };
            let tag_galley = ui.painter().layout_no_wrap(
                tag.to_owned(),
                theme::strong(theme::SMALL),
                palette.ink_muted,
            );
            let tag_width = tag_galley.size().x;
            ui.painter().galley(
                egui::pos2(
                    rect.left() + 10.0,
                    rect.center().y - tag_galley.size().y / 2.0,
                ),
                tag_galley,
                palette.ink_muted,
            );
            let mut text = prompt.replace('\n', " ");
            if *files > 0 {
                text.push_str(&format!("  · {files} attached"));
            }
            let room = rect.width() - tag_width - 150.0;
            let line =
                crate::views::one_line(ui, &text, theme::text(theme::TEXT), palette.ink, room);
            ui.painter().galley(
                egui::pos2(
                    rect.left() + 20.0 + tag_width,
                    rect.center().y - line.size().y / 2.0,
                ),
                line,
                palette.ink,
            );
            let mut buttons = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(vec2(6.0, 3.0)))
                    .layout(Layout::right_to_left(Align::Center)),
            );
            buttons.spacing_mut().item_spacing.x = 6.0;
            if buttons
                .add(
                    egui::Button::new("×")
                        .frame(false)
                        .min_size(vec2(22.0, 22.0)),
                )
                .on_hover_text("Remove from the queue")
                .clicked()
            {
                remove = Some(index);
            }
            let label = if self.is_running() {
                "Send Now"
            } else {
                "Send"
            };
            if buttons.small_button(label).clicked() {
                send = Some(index);
            }
            ui.add_space(4.0);
        }
        ui.add_space(4.0);
        if let Some(index) = remove
            && let Some(items) = self.queued.get_mut(&id)
            && index < items.len()
        {
            let item = items.remove(index);
            // Its text comes back to the box if the box is empty, so nothing is lost by accident.
            if self.prompt.trim().is_empty() {
                self.prompt = item.prompt;
            }
        }
        if let Some(index) = send {
            if self.is_running() {
                // Into the running task, as if typed now.
                if let Some(items) = self.queued.get_mut(&id)
                    && index < items.len()
                {
                    let item = items.remove(index);
                    // Sent as if typed now; when it can't go yet it returns to the queue.
                    let typed = std::mem::replace(&mut self.prompt, item.prompt);
                    self.send_now(ui.ctx());
                    self.prompt = typed;
                }
            } else {
                let ctx = ui.ctx().clone();
                self.send_queued(&ctx, id, index);
            }
        }
    }

    /// The project OpenCode works in, on a tab above the box, as in Codex.
    fn project_bar(&mut self, ui: &mut Ui, palette: &Palette, running: bool) {
        let width = ui.available_width() - 28.0;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        let tab = Rect::from_min_size(rect.min + vec2(14.0, 0.0), vec2(width, 34.0 + 8.0));
        let painter = ui.painter();
        painter.rect_filled(
            tab,
            CornerRadius {
                nw: 12,
                ne: 12,
                sw: 0,
                se: 0,
            },
            palette.rail,
        );
        painter.rect_stroke(
            tab,
            CornerRadius {
                nw: 12,
                ne: 12,
                sw: 0,
                se: 0,
            },
            Stroke::new(1.0, palette.line),
            StrokeKind::Inside,
        );
        let inner = Rect::from_min_size(tab.min + vec2(8.0, 3.0), vec2(width - 16.0, 28.0));
        let mut choose = false;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let name = self
                    .settings
                    .project
                    .as_deref()
                    .and_then(std::path::Path::file_name)
                    .map(|name| name.to_string_lossy().into_owned());
                let (text, ink) = match &name {
                    Some(name) => (name.clone(), palette.ink),
                    None => ("Choose project".to_owned(), palette.accent),
                };
                let chip = toolbar_chip(
                    ui,
                    palette,
                    Some(Icon::Folder),
                    &text,
                    "",
                    ink,
                    true,
                    !running,
                );
                let tip = self.settings.project.as_ref().map_or_else(
                    || "Choose the folder OpenCode works in".to_owned(),
                    |path| format!("{}\nChoose another folder", crate::views::tilde(path)),
                );
                choose = chip.on_hover_text(tip).clicked();
                if let Some(branch) = self.settings.project.as_deref().and_then(git_branch) {
                    ui.add(
                        label(
                            format!("on {branch}"),
                            theme::text(theme::SMALL),
                            palette.ink_faint,
                        )
                        .selectable(false),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(6.0);
                    ui.add(
                        label(
                            "Runs on this computer",
                            theme::text(theme::SMALL),
                            palette.ink_faint,
                        )
                        .selectable(false),
                    );
                });
            },
        );
        // The box below covers the tab's lower edge.
        ui.add_space(-8.0);
        if choose {
            self.pick_project();
        }
    }

    /// Add, what OpenCode may do, plan mode, and dictation while it is running.
    fn left_tools(&mut self, ui: &mut Ui, palette: &Palette, enabled: bool) {
        let add = tool_button(
            ui,
            palette,
            Icon::Plus,
            "Add files, a folder, or plan mode",
            enabled,
        );
        let mut chosen = None;
        egui::Popup::from_toggle_button_response(&add)
            .align(RectAlign::TOP_START)
            .gap(8.0)
            .width(300.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                chosen = self.add_menu_body(ui, palette);
                if chosen.is_some() {
                    ui.close();
                }
            });
        match chosen {
            Some(Add::Files) => self.pick_files(ui.ctx()),
            Some(Add::Folder) => self.pick_folder(ui.ctx()),
            Some(Add::Project) => self.pick_project(),
            Some(Add::Plan) => self.plan_mode = !self.plan_mode,
            None => {}
        }

        if self.listening(ui, palette) {
            return;
        }
        let full = self.settings.auto_approve;
        let (text, ink) = if full {
            ("Full access", palette.danger)
        } else {
            ("Standard access", palette.ink_muted)
        };
        let access = toolbar_chip(
            ui,
            palette,
            Some(Icon::Shield),
            if self.narrow { "" } else { text },
            "",
            ink,
            true,
            enabled,
        );
        let access = if self.narrow {
            access.on_hover_text(text)
        } else {
            access
        };
        egui::Popup::from_toggle_button_response(&access)
            .align(RectAlign::TOP_START)
            .gap(8.0)
            .width(330.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                if self.access_menu_body(ui, palette) {
                    ui.close();
                }
            });
        if self.plan_mode {
            let plan = toolbar_chip(
                ui,
                palette,
                Some(Icon::Bulb),
                "Plan mode",
                "",
                palette.accent,
                false,
                enabled,
            );
            if plan
                .on_hover_text("OpenCode plans without editing files. Click to turn off.")
                .clicked()
            {
                self.plan_mode = false;
            }
        }
    }

    fn add_menu_body(&mut self, ui: &mut Ui, palette: &Palette) -> Option<Add> {
        ui.spacing_mut().item_spacing.y = 0.0;
        menu_heading(ui, palette, "Add");
        let mut chosen = None;
        if menu_row(
            ui,
            palette,
            Icon::Paperclip,
            "Files",
            "Code, documents, images, audio, or video",
            false,
            false,
        )
        .clicked()
        {
            chosen = Some(Add::Files);
        }
        if menu_row(
            ui,
            palette,
            Icon::Folder,
            "Folder",
            "Its text files go with the task",
            false,
            false,
        )
        .clicked()
        {
            chosen = Some(Add::Folder);
        }
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(2.0);
        let project = self.settings.project.as_deref().map_or_else(
            || "Choose the folder OpenCode works in".to_owned(),
            crate::views::tilde,
        );
        if menu_row(
            ui,
            palette,
            Icon::Folder,
            "Work in a project",
            &project,
            false,
            false,
        )
        .clicked()
        {
            chosen = Some(Add::Project);
        }
        if menu_row(
            ui,
            palette,
            Icon::Bulb,
            "Plan mode",
            "Plans without editing files.",
            self.plan_mode,
            false,
        )
        .clicked()
        {
            chosen = Some(Add::Plan);
        }
        ui.add_space(4.0);
        chosen
    }

    /// Returns whether a choice was made.
    fn access_menu_body(&mut self, ui: &mut Ui, palette: &Palette) -> bool {
        ui.spacing_mut().item_spacing.y = 0.0;
        menu_heading(ui, palette, "What can OpenCode do on its own?");
        let mut chosen = None;
        if menu_row(
            ui,
            palette,
            Icon::Shield,
            "Standard access",
            "Runs commands and edits files in the project.",
            !self.settings.auto_approve,
            false,
        )
        .clicked()
        {
            chosen = Some(false);
        }
        if menu_row(
            ui,
            palette,
            Icon::Shield,
            "Full access",
            "Also approves what OpenCode would ask about.",
            self.settings.auto_approve,
            true,
        )
        .clicked()
        {
            chosen = Some(true);
        }
        ui.add_space(4.0);
        if let Some(full) = chosen {
            self.choose_access(ui.ctx(), full);
        }
        chosen.is_some()
    }

    fn mic_button(&mut self, ui: &mut Ui, palette: &Palette, enabled: bool) {
        let recording = matches!(self.inputs.dictation, Dictation::Recording(_))
            || self.inputs.staged_listening.is_some();
        let transcribing = matches!(self.inputs.dictation, Dictation::Transcribing(_));
        if recording {
            let (rect, response) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::click());
            response.widget_info(|| {
                WidgetInfo::labeled(WidgetType::Button, true, "Stop and transcribe")
            });
            ui.painter()
                .circle_filled(rect.center(), 14.0, palette.danger);
            if response.has_focus() {
                ui.painter()
                    .circle_stroke(rect.center(), 16.0, Stroke::new(1.5, palette.accent));
            }
            icons::paint(
                ui.painter(),
                Rect::from_center_size(rect.center(), vec2(12.0, 12.0)),
                Icon::Stop,
                palette.on_accent,
            );
            if response.on_hover_text("Stop and transcribe").clicked() {
                self.toggle_dictation(ui.ctx());
            }
        } else if transcribing {
            let (rect, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
            egui::Spinner::new()
                .size(14.0)
                .color(palette.accent)
                .paint_at(ui, Rect::from_center_size(rect.center(), vec2(14.0, 14.0)));
        } else {
            let name = match self.inputs.model {
                ModelState::Ready(_) => "Dictate",
                _ => "Dictate (downloads the speech model first)",
            };
            if tool_button(ui, palette, Icon::Mic, name, enabled).clicked() {
                self.toggle_dictation(ui.ctx());
            }
        }
    }

    /// The model and its effort, as one quiet button that opens the picker above it.
    fn model_chip(&mut self, ui: &mut Ui, palette: &Palette, enabled: bool) {
        let chip = if let Some(pick) = self.auto_pick() {
            // Auto, and what it would pick for the prompt as it stands.
            let shown = if self.narrow
                || (self.prompt.trim().is_empty() && self.inputs.pending.is_empty())
            {
                String::new()
            } else {
                pick.choice.label()
            };
            let chip = toolbar_chip(
                ui,
                palette,
                Some(Icon::Fork),
                "Auto",
                &shown,
                palette.ink,
                true,
                enabled,
            );
            let choices = &self.settings.auto_models;
            chip.on_hover_text(format!(
                "Auto picks a free model for each task.\nQuick: {}\nEveryday: {}\nHard: {}{}",
                choices.quick.label(),
                choices.everyday.label(),
                choices.hard.label(),
                if shown.is_empty() {
                    String::new()
                } else {
                    format!("\n\nThis task: {}, {}.", pick.tier.name(), pick.reason)
                }
            ))
        } else {
            let selected = models::at(self.route);
            let effort = self
                .effort_of(&selected)
                .filter(|_| !self.narrow)
                .map(|variant| models::effort_label(&variant))
                .unwrap_or_default();
            let chip = toolbar_chip(
                ui,
                palette,
                None,
                &selected.name,
                &effort,
                palette.ink,
                true,
                enabled,
            );
            chip.on_hover_text(format!(
                "{} · {} · {} context\n{}",
                selected.provider_name(),
                selected.reads(),
                selected.context_label(),
                models::provider_access(&selected.provider)
            ))
        };
        egui::Popup::from_toggle_button_response(&chip)
            .align(RectAlign::TOP_END)
            .gap(8.0)
            .width(340.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                if self.model_menu_popup(ui, palette) {
                    ui.close();
                }
            });
    }

    /// Auto or Manual, effort for the selected model, then every model. Returns whether to
    /// close.
    fn model_menu_popup(&mut self, ui: &mut Ui, palette: &Palette) -> bool {
        let auto = self.settings.model_mode == crate::auto::Mode::Auto;
        let choices = &self.settings.auto_models;
        let auto_detail = "A free model picked for each task.".to_owned();
        let _ = choices;
        let manual_name = models::at(self.route).name;
        let mut mode = None;
        if menu_row(ui, palette, Icon::Fork, "Auto", &auto_detail, auto, false).clicked() {
            mode = Some(crate::auto::Mode::Auto);
        }
        if menu_row(
            ui,
            palette,
            Icon::Sliders,
            "Manual",
            &format!("Every task runs on the model you choose, now {manual_name}."),
            !auto,
            false,
        )
        .clicked()
        {
            mode = Some(crate::auto::Mode::Manual);
        }
        if let Some(mode) = mode {
            self.settings.model_mode = mode;
            self.save_settings();
            return true;
        }
        ui.add_space(4.0);
        ui.separator();
        let selected = models::at(self.route);
        if !auto && !selected.variants.is_empty() {
            menu_heading(ui, palette, &format!("Effort for {}", selected.name));
            let current = self.effort_of(&selected);
            let mut picked = None;
            ui.horizontal_wrapped(|ui| {
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                if ui.selectable_label(current.is_none(), "Default").clicked() {
                    picked = Some(None);
                }
                for variant in &selected.variants {
                    let chosen = current.as_deref() == Some(variant.as_str());
                    if ui
                        .selectable_label(chosen, models::effort_label(variant))
                        .clicked()
                    {
                        picked = Some(Some(variant.clone()));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.add(
                    label(
                        "Higher effort thinks longer.",
                        theme::text(theme::SMALL),
                        palette.ink_faint,
                    )
                    .wrap(),
                );
            });
            if let Some(variant) = picked {
                self.set_effort(&selected.id, variant);
            }
            ui.add_space(6.0);
            ui.separator();
        }
        menu_heading(
            ui,
            palette,
            if auto {
                "Choosing a model switches to Manual"
            } else {
                "Model"
            },
        );
        let list = models::all();
        let before = self.route;
        let mut route = self.route;
        // In Auto, no row shows as chosen until one is picked.
        let mut shown = if auto { usize::MAX } else { route };
        let connect = ScrollArea::vertical()
            .max_height(300.0)
            .min_scrolled_height(240.0)
            .show(ui, |ui| model_menu_body(ui, palette, &list, &mut shown))
            .inner;
        if shown != usize::MAX {
            route = shown;
        }
        self.route = route;
        if let Some(provider) = connect {
            self.begin_connect(provider);
            return true;
        }
        if auto && shown != usize::MAX {
            self.settings.model_mode = crate::auto::Mode::Manual;
            self.save_settings();
            return true;
        }
        route != before
    }

    /// The rate-limit fallback, said in full: where a task may continue and what that shares.
    fn fallback_line(&mut self, ui: &mut Ui, palette: &Palette, running: bool) {
        let can_fall_back = self.can_fall_back();
        let backup = models::find(&self.settings.backup_model);
        let backup_name = backup
            .as_ref()
            .map_or_else(|| "the backup model".to_owned(), |model| model.name.clone());
        let mut fallback = self.settings.fallback && can_fall_back;
        let words = if self.narrow {
            format!("Continue on {backup_name}")
        } else {
            format!("Continue on {backup_name} after a rate limit")
        };
        let toggle = checkbox(
            ui,
            palette,
            &mut fallback,
            &words,
            can_fall_back && !running,
        );
        let toggle = toggle.on_hover_text(format!(
            "When a task hits a rate limit, it continues on {backup_name}."
        ));
        if toggle.changed() {
            self.settings.fallback = fallback;
            self.save_settings();
        }
        let selected = models::at(self.route);
        let note = match backup {
            None => "Choose a backup model in Settings.".to_owned(),
            Some(_) if !can_fall_back => {
                "This is the backup model. Choose another backup in Settings.".to_owned()
            }
            Some(backup) if fallback && backup.provider != selected.provider => {
                format!("Shares this session with {}.", backup.provider_name())
            }
            _ => String::new(),
        };
        if !self.narrow {
            ui.add(
                label(note, theme::text(theme::SMALL), palette.ink_faint)
                    .selectable(false)
                    .truncate(),
            );
        }
    }

    /// A ring that fills as the conversation uses the model's context window, with what is
    /// left beside it. Clicking it opens the details, as in Claude Code.
    fn context_ring(&mut self, ui: &mut Ui, palette: &Palette) {
        let context = self.context();
        let fraction = context.fraction();
        let text = match fraction {
            Some(_) => format!("{} left", tokens_short(context.left())),
            None if context.used > 0 => format!("{} used", tokens_short(context.used)),
            None => "Context".to_owned(),
        };
        let shown = if self.narrow {
            String::new()
        } else {
            text.clone()
        };
        let galley =
            ui.painter()
                .layout_no_wrap(shown, theme::text(theme::SMALL), palette.ink_muted);
        let size = vec2(8.0 + 16.0 + 7.0 + galley.size().x + 8.0, 26.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, true, format!("Context window: {text}"))
        });
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(rect, radius(theme::RADIUS_CONTROL), palette.hover);
        }
        if response.has_focus() {
            icons::focus_ring(painter, rect, theme::RADIUS_CONTROL, palette.accent);
        }
        let center = egui::pos2(rect.left() + 8.0 + 8.0, rect.center().y);
        paint_ring(painter, palette, center, 7.0, fraction.unwrap_or(0.0));
        painter.galley(
            egui::pos2(
                center.x + 8.0 + 7.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            palette.ink_muted,
        );
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        egui::Popup::from_toggle_button_response(&response)
            .align(RectAlign::TOP_END)
            .gap(8.0)
            .width(320.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                if self.context_panel(ui, palette) {
                    ui.close();
                }
            });
    }

    /// The context window in numbers, this session's tokens, and today's. Returns whether an
    /// action was taken that should close it.
    pub(crate) fn context_panel(&mut self, ui: &mut Ui, palette: &Palette) -> bool {
        let context = self.context();
        let fraction = context.fraction();
        let mut close = false;
        Frame::new()
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    ui.add(
                        label("Context window", theme::strong(theme::TEXT), palette.ink)
                            .selectable(false),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add(
                            label(
                                context.model.clone(),
                                theme::text(theme::SMALL),
                                palette.ink_faint,
                            )
                            .selectable(false),
                        );
                    });
                });
                ui.add_space(10.0);
                let (bar, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
                ui.painter().rect_filled(bar, radius(3), palette.hover);
                if let Some(fraction) = fraction {
                    let filled = Rect::from_min_size(
                        bar.min,
                        vec2(bar.width() * fraction.max(0.01), bar.height()),
                    );
                    ui.painter()
                        .rect_filled(filled, radius(3), fill_color(palette, fraction));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let used = match fraction {
                        Some(fraction) => format!(
                            "{} of {} used · {:.0}%",
                            tokens_short(context.used),
                            tokens_short(context.limit),
                            fraction * 100.0
                        ),
                        None => format!("{} used", tokens_short(context.used)),
                    };
                    ui.add(
                        label(used, theme::text(theme::SMALL), palette.ink_muted).selectable(false),
                    );
                    if fraction.is_some() {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add(
                                label(
                                    format!("{} left", tokens_short(context.left())),
                                    theme::strong(theme::SMALL),
                                    palette.ink,
                                )
                                .selectable(false),
                            );
                        });
                    }
                });
                ui.add_space(12.0);
                let row = |ui: &mut Ui, title: &str, value: String| {
                    ui.horizontal(|ui| {
                        ui.add(
                            label(title, theme::text(theme::SMALL), palette.ink_faint)
                                .selectable(false),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add(
                                label(value, theme::text(theme::SMALL), palette.ink)
                                    .selectable(false),
                            );
                        });
                    });
                    ui.add_space(5.0);
                };
                match &self.active {
                    Some(session) if session.spent.steps > 0 => row(
                        ui,
                        "This session",
                        format!(
                            "{} steps · {} read · {} written",
                            session.spent.steps,
                            tokens_short(session.spent.read),
                            tokens_short(session.spent.written)
                        ),
                    ),
                    _ => row(ui, "This session", "Nothing sent yet".to_owned()),
                }
                if let Ok(rows) = &self.usage {
                    let steps: i64 = rows.iter().map(|row| row.steps).sum();
                    let tokens: i64 = rows.iter().map(crate::storage::Usage::tokens).sum();
                    row(
                        ui,
                        "Today, all sessions",
                        format!(
                            "{steps} steps · {} tokens",
                            tokens_short(tokens.max(0) as u64)
                        ),
                    );
                }
                ui.add_space(6.0);
                ui.add(
                    label(
                        "From the last finished step.",
                        theme::text(theme::SMALL),
                        palette.ink_faint,
                    )
                    .wrap(),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if fraction.is_some_and(|fraction| fraction >= 0.7)
                        && !self.is_running()
                        && ui
                            .button("New Session")
                            .on_hover_text("Start fresh for an unrelated task")
                            .clicked()
                    {
                        self.reset();
                        close = true;
                    }
                    if ui.button("Usage").clicked() {
                        self.open(View::Usage);
                        close = true;
                    }
                });
            });
        close
    }

    /// Draws one of the composer's menus for a screenshot.
    #[cfg(debug_assertions)]
    pub(crate) fn preview_menu_body(
        &mut self,
        ui: &mut Ui,
        palette: &Palette,
        menu: crate::app::Menu,
    ) {
        use crate::app::Menu;
        match menu {
            Menu::Add => {
                self.add_menu_body(ui, palette);
            }
            Menu::Access => {
                self.access_menu_body(ui, palette);
            }
            Menu::Model => {
                self.model_menu_popup(ui, palette);
            }
            Menu::Context => {
                self.context_panel(ui, palette);
            }
            Menu::More => {
                self.more_menu(ui, palette);
            }
        }
    }

    /// Asks before Full access turns on, saying what it approves on this computer.
    pub(crate) fn full_access_dialog(&mut self, ctx: &egui::Context, palette: &Palette) {
        if !self.confirm_full_access {
            return;
        }
        let frame = Frame::new()
            .fill(palette.raised)
            .stroke(Stroke::new(1.0, palette.line))
            .corner_radius(radius(16))
            .shadow(Shadow {
                offset: [0, 10],
                blur: 30,
                spread: 0,
                color: palette.shadow,
            })
            .inner_margin(Margin::same(24));
        let mut answer = None;
        let shown = egui::Modal::new(egui::Id::new("confirm-full-access"))
            .frame(frame)
            .backdrop_color(Color32::from_black_alpha(90))
            .show(ctx, |ui| {
                ui.set_width(480.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (rect, _) = ui.allocate_exact_size(vec2(20.0, 26.0), Sense::hover());
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(rect.center(), vec2(19.0, 19.0)),
                        Icon::Warning,
                        palette.danger,
                    );
                    ui.add(
                        label("Turn on Full access?", theme::strong(theme::TITLE - 3.0), palette.ink)
                            .selectable(false),
                    );
                });
                ui.add_space(12.0);
                ui.add(
                    label(
                        "OpenCode will approve everything its rules would ask you about, including:",
                        theme::text(theme::READING),
                        palette.ink_muted,
                    )
                    .wrap(),
                );
                ui.add_space(16.0);

                let rules = match &self.ask_rules {
                    None => "Checking your OpenCode rules…".to_owned(),
                    Some(Ok(rules)) if rules.is_empty() => {
                        "Nothing right now.".to_owned()
                    }
                    Some(Ok(rules)) => format!("Your OpenCode rules ask before: {}.", rules.join(", ")),
                    Some(Err(_)) => {
                        "Every rule set to ask.".to_owned()
                    }
                };
                let items: [(Icon, &str, String); 3] = [
                    (
                        Icon::File,
                        "Secret files",
                        "Files like .env, which often hold keys and passwords.".to_owned(),
                    ),
                    (Icon::Shield, "Rules set to ask", rules),
                    (
                        Icon::Warning,
                        "Prompt injection",
                        "Hidden instructions in files or pages can steer the model.".to_owned(),
                    ),
                ];
                Frame::new()
                    .fill(palette.ground)
                    .stroke(Stroke::new(1.0, palette.line))
                    .corner_radius(radius(12))
                    .inner_margin(Margin::symmetric(14, 4))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for (index, (icon, title, detail)) in items.iter().enumerate() {
                            if index > 0 {
                                let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                                ui.painter().hline(
                                    line.x_range().shrink(0.0),
                                    line.center().y,
                                    Stroke::new(1.0, palette.line),
                                );
                            }
                            ui.add_space(10.0);
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 12.0;
                                let (rect, _) = ui.allocate_exact_size(vec2(18.0, 20.0), Sense::hover());
                                icons::paint(
                                    ui.painter(),
                                    Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
                                    *icon,
                                    palette.ink_muted,
                                );
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 2.0;
                                    ui.add(label(*title, theme::strong(theme::TEXT), palette.ink).selectable(false));
                                    ui.add(label(detail.clone(), theme::text(theme::SMALL), palette.ink_muted).wrap());
                                });
                            });
                            ui.add_space(10.0);
                        }
                    });
                ui.add_space(16.0);
                ui.add(
                    label(
                        "You can switch back to Standard access any time.",
                        theme::text(theme::SMALL),
                        palette.ink_faint,
                    )
                    .wrap(),
                );
                ui.add_space(20.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let confirm = egui::Button::new(
                        RichText::new("Turn On Full Access")
                            .font(theme::strong(theme::TEXT))
                            .color(palette.on_accent),
                    )
                    .fill(palette.danger)
                    .stroke(Stroke::NONE)
                    .min_size(vec2(0.0, 32.0));
                    if ui.add(confirm).clicked() {
                        answer = Some(true);
                    }
                    let cancel = ui.add(egui::Button::new("Cancel").min_size(vec2(84.0, 32.0)));
                    // The safe choice holds the keyboard when the dialog opens.
                    let opened = egui::Id::new("confirm-full-access-focused");
                    if !ui.data(|data| data.get_temp::<bool>(opened).unwrap_or(false)) {
                        cancel.request_focus();
                        ui.data_mut(|data| data.insert_temp(opened, true));
                    }
                    if cancel.clicked() {
                        answer = Some(false);
                    }
                });
            });
        if shown.should_close() && answer.is_none() {
            answer = Some(false);
        }
        if let Some(accepted) = answer {
            ctx.data_mut(|data| data.remove::<bool>(egui::Id::new("confirm-full-access-focused")));
            self.answer_full_access(accepted);
        }
    }

    /// The attachments waiting to go, as square cards.
    fn attachment_cards(&mut self, ui: &mut Ui, palette: &Palette) {
        if self.inputs.pending.is_empty() {
            return;
        }
        let refusals = self.refusals();
        struct Shown {
            id: Uuid,
            source: std::path::PathBuf,
            kind: Kind,
            name: String,
            type_label: String,
            note: String,
            tone: Tone,
            preview: Option<egui::TextureHandle>,
        }
        let ctx = ui.ctx().clone();
        let mut cards = Vec::new();
        for item in &self.inputs.pending {
            let refused = refusals.iter().any(|(id, ..)| *id == item.id);
            let (note, tone) = match &item.prep {
                Prep::Preparing => (
                    match item.kind {
                        Kind::Document => "Reading text…",
                        Kind::Audio => "Transcribing…",
                        Kind::Folder => "Gathering files…",
                        _ => "Checking…",
                    }
                    .to_owned(),
                    Tone::Busy,
                ),
                Prep::Ready(_) if refused => ("Not readable here".to_owned(), Tone::Warn),
                Prep::Ready(sent) => (sent.note.clone(), Tone::Quiet),
                Prep::NeedsModel => ("Needs the speech model".to_owned(), Tone::Warn),
                Prep::Failed(_) => ("Can’t attach".to_owned(), Tone::Danger),
            };
            cards.push(Shown {
                id: item.id,
                source: item.source.clone(),
                kind: item.kind,
                name: item.name.clone(),
                type_label: attach::type_label(&item.source, item.kind),
                note,
                tone,
                preview: None,
            });
        }
        for card in &mut cards {
            card.preview = self.inputs.thumbnail(&ctx, &card.source);
        }
        let mut removed = None;
        let mut opened = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for card in &cards {
                let clicks = attachment_card(
                    ui,
                    palette,
                    CARD,
                    Card {
                        kind: card.kind,
                        name: &card.name,
                        type_label: &card.type_label,
                        note: &card.note,
                        tone: card.tone,
                        preview: card.preview.as_ref(),
                    },
                    palette.ground,
                    true,
                );
                if clicks.remove {
                    removed = Some(card.id);
                } else if clicks.open {
                    opened = Some(card.source.clone());
                }
            }
        });
        ui.add_space(10.0);
        if let Some(id) = removed {
            self.remove_attachment(id);
        }
        if let Some(path) = opened
            && let Err(message) = open_path(&path)
        {
            self.inputs.notice = Some(message);
        }
    }

    /// What's wrong with any attachment, and how to fix it.
    fn attachment_notes(&mut self, ui: &mut Ui, palette: &Palette) {
        let refusals = self.refusals();
        let mut switch_to = None;
        for (id, message, alternative) in &refusals {
            let name = self
                .inputs
                .pending
                .iter()
                .find(|item| item.id == *id)
                .map(|item| item.name.as_str())
                .unwrap_or_default();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add(
                    label(
                        format!("{name}: {message}"),
                        theme::text(theme::SMALL),
                        palette.warn,
                    )
                    .wrap(),
                );
                if let Some(index) = alternative
                    && ui
                        .small_button(format!("Use {}", models::at(*index).name))
                        .clicked()
                {
                    switch_to = Some(*index);
                }
            });
            ui.add_space(6.0);
        }
        if let Some(index) = switch_to {
            self.route = index;
        }
        for item in &self.inputs.pending {
            if let Prep::Failed(message) = &item.prep {
                ui.add(
                    label(
                        format!("{}: {message}", item.name),
                        theme::text(theme::SMALL),
                        palette.danger,
                    )
                    .wrap(),
                );
                ui.add_space(6.0);
            }
        }
        let mut dismiss = false;
        if let Some(notice) = &self.inputs.notice {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                ui.add(label(notice.clone(), theme::text(theme::SMALL), palette.warn).wrap());
                dismiss = ui.small_button("Dismiss").clicked();
            });
            ui.add_space(8.0);
        }
        if dismiss {
            self.inputs.notice = None;
        }
    }

    /// While dictating, the model picker's place shows the microphone level and the time.
    /// Returns whether it drew anything.
    fn listening(&mut self, ui: &mut Ui, palette: &Palette) -> bool {
        let (level, elapsed) = match (&self.inputs.dictation, self.inputs.staged_listening) {
            (_, Some(staged)) => staged,
            (Dictation::Recording(recorder), None) => (recorder.level(), recorder.elapsed()),
            (Dictation::Transcribing(_), None) => {
                ui.add(
                    label(
                        "Transcribing on this computer…",
                        theme::text(theme::SMALL),
                        palette.ink_muted,
                    )
                    .selectable(false),
                );
                return true;
            }
            (Dictation::Idle, None) => return false,
        };
        let (dot, _) = ui.allocate_exact_size(vec2(8.0, 20.0), Sense::hover());
        ui.painter()
            .circle_filled(dot.center(), 4.0, palette.danger.gamma_multiply(breath(ui)));
        ui.add(
            label(
                format!("Listening {}", minutes(elapsed)),
                theme::strong(theme::SMALL),
                palette.ink,
            )
            .selectable(false),
        );
        // Speech peaks sit low on a linear scale; a square root spreads them across the bar.
        let (bar, _) = ui.allocate_exact_size(vec2(72.0, 20.0), Sense::hover());
        let track = Rect::from_center_size(bar.center(), vec2(72.0, 4.0));
        ui.painter().rect_filled(track, radius(2), palette.hover);
        let filled = Rect::from_min_size(track.min, vec2(72.0 * level.sqrt().min(1.0), 4.0));
        ui.painter().rect_filled(filled, radius(2), palette.danger);
        if ui
            .add(
                label("Cancel", theme::text(theme::SMALL), palette.ink_muted)
                    .selectable(false)
                    .sense(Sense::click()),
            )
            .on_hover_text("Esc")
            .clicked()
        {
            self.cancel_dictation();
            self.inputs.staged_listening = None;
        }
        true
    }

    /// Offers the speech model when dictation or an audio file needs it, and shows progress.
    fn speech_model_panel(&mut self, ui: &mut Ui, palette: &Palette) {
        let downloading = matches!(self.inputs.model, ModelState::Downloading { .. });
        let failed = matches!(self.inputs.model, ModelState::Failed(_));
        if !(self.inputs.offer_model && !matches!(self.inputs.model, ModelState::Ready(_)))
            && !downloading
        {
            return;
        }
        let mut action = None;
        Frame::new()
            .fill(palette.accent_wash.gamma_multiply(0.5))
            .corner_radius(radius(10))
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 8.0;
                match &self.inputs.model {
                    ModelState::Downloading { received, .. } => {
                        let got = received.load(std::sync::atomic::Ordering::Relaxed);
                        let fraction = got as f32 / speech::MODEL.bytes as f32;
                        ui.add(
                            label(
                                format!(
                                    "Downloading the speech model: {} of {}",
                                    speech::megabytes(got),
                                    speech::megabytes(speech::MODEL.bytes)
                                ),
                                theme::text(theme::SMALL),
                                palette.ink,
                            )
                            .selectable(false),
                        );
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::ProgressBar::new(fraction.min(1.0))
                                    .desired_width((ui.available_width() - 80.0).max(60.0))
                                    .desired_height(6.0)
                                    .fill(palette.accent)
                                    .corner_radius(radius(3)),
                            );
                            if ui.small_button("Cancel").clicked() {
                                action = Some("cancel");
                            }
                        });
                    }
                    ModelState::Failed(message) => {
                        ui.add(
                            label(message.clone(), theme::text(theme::SMALL), palette.danger)
                                .wrap(),
                        );
                    }
                    _ => {
                        ui.add(
                            label(
                                format!(
                                    "Speech is turned into text on this computer. The model downloads once ({}).",
                                    speech::megabytes(speech::MODEL.bytes)
                                ),
                                theme::text(theme::SMALL),
                                palette.ink,
                            )
                            .wrap(),
                        );
                    }
                }
                if !downloading {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let download = egui::Button::new(
                            RichText::new(if failed { "Try Again" } else { "Download" })
                                .font(theme::strong(theme::SMALL))
                                .color(palette.on_accent),
                        )
                        .fill(palette.accent)
                        .stroke(Stroke::NONE);
                        if ui.add(download).clicked() {
                            action = Some("download");
                        }
                        if ui.small_button("Not Now").clicked() {
                            action = Some("dismiss");
                        }
                    });
                }
            });
        ui.add_space(10.0);
        match action {
            Some("download") => self.start_download(ui.ctx()),
            Some("cancel") => self.cancel_download(),
            Some("dismiss") => {
                self.inputs.offer_model = false;
                if failed {
                    self.inputs.model = ModelState::Missing;
                }
            }
            _ => {}
        }
    }
}

/// A drop-down of every model, grouped by provider, each with what it reads, followed by the
/// providers that would add more free models. Returns the chosen position in [`models::all`],
/// and a provider to connect when one of their Connect buttons was clicked.
pub(crate) fn model_menu(
    ui: &mut Ui,
    palette: &Palette,
    id: &str,
    current: usize,
    width: f32,
) -> (usize, Option<&'static str>) {
    let list = models::all();
    let mut chosen = current.min(list.len() - 1);
    let mut connect = None;
    let chevron = palette.ink_muted;
    egui::ComboBox::from_id_salt(id)
        .selected_text(RichText::new(&list[chosen].name).font(theme::strong(theme::TEXT)))
        .width(width)
        .height(460.0)
        .icon(move |ui, rect, _, _| {
            let at = Rect::from_center_size(rect.center(), vec2(12.0, 12.0));
            icons::paint(ui.painter(), at, Icon::ChevronDown, chevron);
        })
        .show_ui(ui, |ui| {
            connect = model_menu_body(ui, palette, &list, &mut chosen);
        });
    (chosen, connect)
}

/// The inside of the model drop-down. Returns a provider to connect when one of the Connect
/// buttons was clicked.
pub(crate) fn model_menu_body(
    ui: &mut Ui,
    palette: &Palette,
    list: &[models::Model],
    chosen: &mut usize,
) -> Option<&'static str> {
    let mut connect = None;
    let small = |ui: &mut Ui, text: String, strong: bool, color| {
        let font = if strong {
            theme::strong(theme::SMALL)
        } else {
            theme::text(theme::SMALL)
        };
        ui.add(label(text, font, color).selectable(false).wrap());
    };
    ui.set_min_width(320.0);
    ui.set_max_width(380.0);
    let present = models::providers(list);
    for provider in &present {
        ui.add_space(6.0);
        small(ui, models::provider_name(provider), true, palette.ink_muted);
        small(
            ui,
            models::provider_access(provider).to_owned(),
            false,
            palette.ink_faint,
        );
        ui.add_space(2.0);
        for (index, model) in list.iter().enumerate() {
            if &model.provider != provider {
                continue;
            }
            let mut job = LayoutJob::default();
            job.append(
                &model.name,
                0.0,
                egui::TextFormat::simple(theme::text(theme::TEXT), palette.ink),
            );
            let context = model.context_label();
            let detail = if context.is_empty() {
                format!("   {}", model.reads())
            } else {
                format!("   {} · {context}", model.reads())
            };
            job.append(
                &detail,
                0.0,
                egui::TextFormat::simple(theme::text(theme::SMALL), palette.ink_faint),
            );
            ui.selectable_value(chosen, index, job)
                .on_hover_text(&model.id);
        }
    }

    // What else is free, and the button that gets it.
    let missing: Vec<_> = providers::CONNECTABLE
        .iter()
        .filter(|provider| !present.iter().any(|known| known == provider.id))
        .collect();
    let local = present.iter().any(|provider| models::is_local(provider));
    if !missing.is_empty() || !local {
        ui.add_space(8.0);
        ui.separator();
        small(
            ui,
            "More free models with a free account".into(),
            true,
            palette.ink_muted,
        );
        for provider in missing {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if ui.small_button("Connect…").clicked() {
                    connect = Some(provider.id);
                    ui.close();
                }
                ui.add(
                    label(provider.name, theme::strong(theme::SMALL), palette.ink)
                        .selectable(false),
                );
            });
            small(ui, provider.models.to_owned(), false, palette.ink_faint);
            ui.add_space(2.0);
        }
        if !local {
            ui.add_space(4.0);
            small(
                ui,
                "Ollama, LM Studio, and vLLM models show here while running.".into(),
                false,
                palette.ink_faint,
            );
        }
    }
    connect
}

/// Draws the context ring: a track, and an arc from twelve o'clock for the used part.
pub(crate) fn paint_ring(
    painter: &egui::Painter,
    palette: &Palette,
    center: egui::Pos2,
    radius: f32,
    fraction: f32,
) {
    painter.circle_stroke(center, radius, Stroke::new(2.2, palette.line_strong));
    if fraction <= 0.0 {
        return;
    }
    let steps = (64.0 * fraction).ceil().max(2.0) as usize;
    let start = -std::f32::consts::FRAC_PI_2;
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|step| {
            let angle = start + std::f32::consts::TAU * fraction * step as f32 / steps as f32;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        Stroke::new(2.4, fill_color(palette, fraction)),
    ));
}

/// The branch checked out in `project`, read from `.git/HEAD`.
fn git_branch(project: &std::path::Path) -> Option<String> {
    let head = std::fs::read_to_string(project.join(".git").join("HEAD")).ok()?;
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    #[test]
    fn token_counts_are_short() {
        assert_eq!(super::tokens_short(999), "999");
        assert_eq!(super::tokens_short(48_213), "48.2K");
        assert_eq!(super::tokens_short(200_000), "200K");
        assert_eq!(super::tokens_short(1_048_576), "1M");
    }
}
