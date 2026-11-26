//! Everything the window draws: sidebar, transcript, composer, status bar, and the
//! usage and settings pages.

use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Frame, Label, Layout, Margin, Rect, Response,
    RichText, ScrollArea, Sense, Stroke, Ui, WidgetInfo, WidgetType,
    text::{LayoutJob, TextWrapping},
    vec2,
};

use crate::{
    app::{App, NAME, Parked, Status, View},
    composer::{Card, SENT_CARD, Tone, attachment_card},
    icons::{self, Icon},
    inputs::ModelState,
    markdown, models,
    storage::{Appearance, Pace, Scale, Usage},
    theme::{self, Palette},
};

/// Room for the window buttons where the sidebar runs under the title bar.
const TITLE_BAR: f32 = if cfg!(target_os = "macos") { 30.0 } else { 0.0 };
const SHORTCUT_NEW: &str = if cfg!(target_os = "macos") {
    "⌘N"
} else {
    "Ctrl+N"
};
pub(crate) fn radius(value: u8) -> CornerRadius {
    CornerRadius::same(value)
}

/// The categories of the Settings page, with the words its search finds them by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Section {
    Models,
    Providers,
    Tasks,
    Voice,
    Appearance,
    OpenCode,
    Storage,
    Help,
}

impl Section {
    pub(crate) const ALL: [Self; 8] = [
        Self::Models,
        Self::Providers,
        Self::Tasks,
        Self::Voice,
        Self::Appearance,
        Self::OpenCode,
        Self::Storage,
        Self::Help,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Models => "Models",
            Self::Providers => "Free accounts",
            Self::Tasks => "Tasks",
            Self::Voice => "Voice",
            Self::Appearance => "Appearance",
            Self::OpenCode => "OpenCode",
            Self::Storage => "Storage",
            Self::Help => "Help",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Self::Models => Icon::Fork,
            Self::Providers => Icon::Shield,
            Self::Tasks => Icon::List,
            Self::Voice => Icon::Mic,
            Self::Appearance => Icon::Sliders,
            Self::OpenCode => Icon::Terminal,
            Self::Storage => Icon::Folder,
            Self::Help => Icon::Book,
        }
    }

    fn matches(self, query: &str) -> bool {
        let words = match self {
            Self::Models => {
                "models auto manual quick everyday hard effort default backup rate limit fallback catalog look for models"
            }
            Self::Providers => {
                "free accounts providers keys api key connect disconnect openrouter nvidia modelscope z.ai google hugging face"
            }
            Self::Tasks => {
                "tasks time limit approve full access permissions notify notification attention send return keep awake sleep"
            }
            Self::Voice => {
                "voice dictation microphone speech model whisper language read aloud reading speed"
            }
            Self::Appearance => {
                "appearance theme light dark text size scale transcript thinking verbose width motion animation links browser"
            }
            Self::OpenCode => "opencode executable path install cli",
            Self::Storage => "storage data folder usage history clear",
            Self::Help => "help tutorial credits github version",
        };
        self.name().to_lowercase().contains(query) || words.contains(query)
    }
}

pub(crate) fn label(text: impl Into<String>, font: egui::FontId, color: Color32) -> Label {
    Label::new(RichText::new(text).font(font).color(color))
}

/// One line of text cut to `width` with an ellipsis.
pub(crate) fn one_line(
    ui: &Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.replace('\n', " "), font, color);
    job.wrap = TextWrapping::truncate_at_width(width.max(0.0));
    ui.painter().layout_job(job)
}

/// A full-width row that acts as a button: optional icon, label, optional trailing note.
fn row(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    text: &str,
    trailing: Option<&str>,
    selected: bool,
    enabled: bool,
) -> Response {
    row_marked(
        ui,
        palette,
        icon,
        text,
        trailing,
        selected,
        enabled,
        Mark::None,
    )
}

/// What a sidebar row shows at its end instead of its note.
#[derive(Clone, Copy)]
pub(crate) enum Mark<'a> {
    None,
    /// A task runs: the sign for what it is doing.
    Working(&'a str),
    /// A task finished while the session was out of view: its result's colour.
    News(Color32),
}

/// [`row`], with a mark in place of the trailing note.
#[allow(clippy::too_many_arguments)]
fn row_marked(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    text: &str,
    trailing: Option<&str>,
    selected: bool,
    enabled: bool,
    mark: Mark,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), sense);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, selected, text));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    let fill = if selected {
        palette.accent_wash
    } else if enabled && response.hovered() {
        palette.hover
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, radius(theme::RADIUS_CONTROL), fill);
    if response.has_focus() {
        icons::focus_ring(
            painter,
            rect.shrink(1.5),
            theme::RADIUS_CONTROL,
            palette.accent,
        );
    }
    let ink = if !enabled {
        palette.ink_faint
    } else if selected {
        palette.ink
    } else {
        palette.ink_muted
    };
    let mut left = rect.left() + 9.0;
    if let Some(icon) = icon {
        let at = Rect::from_center_size(egui::pos2(left + 8.0, rect.center().y), vec2(15.0, 15.0));
        icons::paint(painter, at, icon, ink);
        left += 25.0;
    }
    let mut right = rect.right() - 9.0;
    if let Mark::Working(doing) = mark {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
        right -= 16.0;
        crate::working::paint(
            painter,
            Rect::from_center_size(egui::pos2(right + 8.0, rect.center().y), vec2(14.0, 14.0)),
            crate::working::Sign::of(doing),
            ui.input(|input| input.time),
            palette.accent,
            palette.accent.gamma_multiply(0.42),
            false,
        );
        right -= 8.0;
    } else if let Mark::News(color) = mark {
        right -= 8.0;
        painter.circle_filled(egui::pos2(right + 4.0, rect.center().y), 3.5, color);
        right -= 8.0;
    } else if let Some(note) = trailing {
        let galley = painter.layout_no_wrap(
            note.to_owned(),
            theme::text(theme::SMALL),
            palette.ink_faint,
        );
        right -= galley.size().x;
        painter.galley(
            egui::pos2(right, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.ink_faint,
        );
        right -= 8.0;
    }
    let font = if selected {
        theme::strong(theme::TEXT)
    } else {
        theme::text(theme::TEXT)
    };
    let galley = one_line(ui, text, font, ink, right - left);
    ui.painter().galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response
}

/// A round icon button, used for send and stop.
pub(crate) fn round_button(
    ui: &mut Ui,
    palette: &Palette,
    icon: Icon,
    name: &str,
    fill: Color32,
    ink: Color32,
    enabled: bool,
) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name));
    let painter = ui.painter();
    let (fill, ink) = if enabled {
        (fill, ink)
    } else {
        (palette.hover, palette.ink_faint)
    };
    painter.circle_filled(rect.center(), 16.0, fill);
    if enabled && response.hovered() {
        // A thin ring says "pressable" without changing the control's size.
        painter.circle_stroke(
            rect.center(),
            17.5,
            Stroke::new(1.5, fill.gamma_multiply(0.45)),
        );
    }
    if response.has_focus() {
        painter.circle_stroke(rect.center(), 18.5, Stroke::new(1.5, palette.accent));
    }
    icons::paint(
        painter,
        Rect::from_center_size(rect.center(), vec2(15.0, 15.0)),
        icon,
        ink,
    );
    response.on_hover_text(name)
}

/// A checkbox with a square box. egui's own takes the control radius and reads as a radio.
pub(crate) fn checkbox(
    ui: &mut Ui,
    palette: &Palette,
    checked: &mut bool,
    text: &str,
    enabled: bool,
) -> Response {
    let fade = if enabled { 1.0 } else { 0.55 };
    let ink = palette.ink_muted.gamma_multiply(fade);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::text(theme::SMALL), ink);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(vec2(24.0 + galley.size().x, 22.0), sense);
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    let state = *checked;
    response.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, state, text));
    let painter = ui.painter();
    let square = Rect::from_center_size(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        vec2(15.0, 15.0),
    );
    if state {
        painter.rect_filled(square, radius(4), palette.accent.gamma_multiply(fade));
        let tick = [(3.6, 7.8), (6.4, 10.6), (11.4, 4.8)];
        painter.line(
            tick.iter().map(|&(x, y)| square.min + vec2(x, y)).collect(),
            Stroke::new(1.6, palette.on_accent),
        );
    } else {
        painter.rect_filled(square, radius(4), palette.raised);
        painter.rect_stroke(
            square,
            radius(4),
            Stroke::new(1.0, palette.ink_faint.gamma_multiply(fade)),
            egui::StrokeKind::Inside,
        );
    }
    if response.has_focus() {
        icons::focus_ring(painter, square, 4, palette.accent);
    }
    painter.galley(
        egui::pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response
}

/// The conversation column's width from Settings, set each frame.
pub(crate) fn column_width(ctx: &egui::Context) -> f32 {
    ctx.data(|data| data.get_temp::<f32>(egui::Id::new("column-width")))
        .unwrap_or(theme::COLUMN)
}

/// Centres a column of at most the chosen width in the space available.
pub(crate) fn column<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let full = ui.available_width();
    let width = full.min(column_width(ui.ctx()));
    let side = ((full - width) / 2.0).floor();
    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(side);
        ui.vertical(|ui| {
            ui.set_width(width);
            add(ui)
        })
        .inner
    })
    .inner
}

/// `/Users/sam/code/app` as `~/code/app`.
pub(crate) fn tilde(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// How long ago, in the shortest form that is still unambiguous.
pub(crate) fn ago(seconds: f64) -> String {
    let minutes = (seconds / 60.0) as i64;
    match minutes {
        0 => "now".into(),
        1..=59 => format!("{minutes}m"),
        60..=1439 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

/// 48213 as 48.2k.
pub(crate) fn compact(count: i64) -> String {
    match count {
        0..=999 => count.to_string(),
        1_000..=999_999 => format!("{:.1}k", count as f64 / 1_000.0),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
}

/// 48213 as 48,213.
pub(crate) fn grouped(count: i64) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn status_color(palette: &Palette, status: Status) -> Option<Color32> {
    match status {
        Status::Ready => None,
        Status::Running | Status::Stopping => Some(palette.accent),
        Status::Complete => Some(palette.ok),
        Status::Stopped => Some(palette.warn),
        Status::Failed => Some(palette.danger),
    }
}

/// Opacity of a mark that breathes while work is in progress. Keeps the window repainting.
pub(crate) fn breath(ui: &Ui) -> f32 {
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(50));
    0.6 + 0.4 * (ui.input(|input| input.time) * 2.4).sin() as f32
}

fn status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 3.5, color);
}

impl App {
    pub(crate) fn draw(&mut self, ui: &mut Ui) {
        let palette = Palette::of(ui.ctx());
        let width = self.settings.width.points();
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("column-width"), width));
        let status = egui::Panel::bottom("status-bar")
            .resizable(false)
            .show_separator_line(false)
            .exact_size(27.0)
            .frame(
                Frame::new()
                    .fill(palette.rail)
                    .inner_margin(Margin::symmetric(12, 0)),
            )
            .show(ui, |ui| self.status_bar(ui, &palette))
            .response
            .rect;
        let sidebar = egui::Panel::left("sidebar")
            .resizable(true)
            .show_separator_line(false)
            .default_size(252.0)
            .min_size(212.0)
            .max_size(340.0)
            .frame(
                Frame::new()
                    .fill(palette.rail)
                    .inner_margin(Margin::symmetric(10, 10)),
            )
            .show(ui, |ui| self.sidebar(ui, &palette))
            .response
            .rect;
        // The pane takes what the conversation can spare, keeping it at least 460 wide, or
        // the whole window when expanded.
        let spare = (ui.available_width() - 460.0).max(320.0);
        let pane = self.pane;
        let wide = self.pane_wide;
        let terminal_side = self.terminal_open && self.settings.terminal_side;
        let terminal_bottom = self.terminal_open && !self.settings.terminal_side;
        // The side holds the pane, the terminal under it, or both.
        let side = (pane.is_some() && !wide || terminal_side).then(|| {
            egui::Panel::right("pane")
                .resizable(true)
                .show_separator_line(false)
                .default_size(spare.min(560.0))
                .min_size(320.0_f32.min(spare))
                .max_size(spare)
                .frame(Frame::new().fill(palette.raised))
                .show(ui, |ui| {
                    let height = ui.available_height();
                    match (pane.filter(|_| !wide), terminal_side) {
                        (Some(pane), true) => {
                            egui::Panel::bottom("terminal-side")
                                .resizable(true)
                                .show_separator_line(true)
                                .default_size(height * 0.42)
                                .min_size(120.0)
                                .max_size(height - 120.0)
                                .frame(Frame::new().fill(palette.raised))
                                .show(ui, |ui| self.terminal_dock(ui, &palette));
                            self.pane_ui(ui, &palette, pane);
                        }
                        (Some(pane), false) => self.pane_ui(ui, &palette, pane),
                        (None, _) => self.terminal_dock(ui, &palette),
                    }
                })
                .response
                .rect
        });
        // At the bottom, the terminal spans the conversation's width, as in Codex.
        let bottom = terminal_bottom.then(|| {
            let height = ui.available_height();
            egui::Panel::bottom("terminal-bottom")
                .resizable(true)
                .show_separator_line(false)
                .default_size((height * 0.36).max(180.0))
                .min_size(110.0)
                .max_size(height - 160.0)
                .frame(Frame::new().fill(palette.raised))
                .show(ui, |ui| self.terminal_dock(ui, &palette))
                .response
                .rect
        });
        if let (Some(pane), true) = (pane, wide) {
            egui::CentralPanel::default()
                .frame(Frame::new().fill(palette.raised))
                .show(ui, |ui| self.pane_ui(ui, &palette, pane));
        }
        if !(pane.is_some() && wide) {
            egui::CentralPanel::default()
                .frame(Frame::new().fill(palette.ground))
                .show(ui, |ui| match self.view {
                    View::Chat => self.chat(ui, &palette),
                    View::Usage => self.usage_page(ui, &palette),
                    View::Settings => self.settings_page(ui, &palette),
                });
        }
        #[cfg(debug_assertions)]
        if let Some(menu) = self.preview_menu {
            // Screenshots can't hold a menu open, so its contents are drawn where it opens.
            let (pivot, at) = match menu {
                crate::app::Menu::Add => (Align2::LEFT_BOTTOM, egui::pos2(310.0, 590.0)),
                crate::app::Menu::Access => (Align2::LEFT_BOTTOM, egui::pos2(345.0, 590.0)),
                crate::app::Menu::Model => (Align2::RIGHT_TOP, egui::pos2(1000.0, 70.0)),
                crate::app::Menu::Context => (Align2::RIGHT_BOTTOM, egui::pos2(1056.0, 648.0)),
                crate::app::Menu::More => (Align2::RIGHT_TOP, egui::pos2(1086.0, 44.0)),
            };
            egui::Area::new(egui::Id::new(("menu-preview", menu)))
                .pivot(pivot)
                .fixed_pos(at)
                .show(ui.ctx(), |ui| {
                    Frame::menu(ui.style()).show(ui, |ui| {
                        let width = match menu {
                            crate::app::Menu::Add => 300.0,
                            crate::app::Menu::Access => 330.0,
                            crate::app::Menu::Model => 340.0,
                            crate::app::Menu::Context => 320.0,
                            crate::app::Menu::More => 250.0,
                        };
                        ui.set_width(width);
                        self.preview_menu_body(ui, &palette, menu);
                    });
                });
        }
        #[cfg(debug_assertions)]
        if self.preview_signs {
            signs_sheet(ui.ctx(), &palette);
        }
        self.full_access_dialog(&ui.ctx().clone(), &palette);
        self.tutorial_dialog(&ui.ctx().clone(), &palette);
        // Hairlines mark where the work area meets the rail and the status bar. They are
        // painted last so no panel covers them.
        let line = Stroke::new(1.0, palette.line);
        ui.painter().vline(sidebar.right(), sidebar.y_range(), line);
        if let Some(side) = side {
            ui.painter().vline(side.left(), side.y_range(), line);
        }
        if let Some(bottom) = bottom {
            ui.painter().hline(bottom.x_range(), bottom.top(), line);
        }
        ui.painter().hline(status.x_range(), status.top(), line);
    }

    fn sidebar(&mut self, ui: &mut Ui, palette: &Palette) {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(TITLE_BAR);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.image((self.logo.id(), vec2(22.0, 22.0)));
            ui.add(label(NAME, theme::strong(theme::TEXT + 0.5), palette.ink).selectable(false));
        });
        ui.add_space(14.0);

        let idle = !self.is_running();
        let fresh = self.view == View::Chat && self.active.is_none();
        if row(
            ui,
            palette,
            Some(Icon::Plus),
            "New session",
            Some(SHORTCUT_NEW),
            fresh,
            true,
        )
        .clicked()
        {
            self.reset();
        }
        let project = self
            .settings
            .project
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned());
        let project_row = row(
            ui,
            palette,
            Some(Icon::Folder),
            project.as_deref().unwrap_or("Choose project folder…"),
            project.is_some().then_some("Change"),
            false,
            idle,
        );
        if project_row.clicked() {
            self.pick_project();
        }
        if let Some(path) = &self.settings.project {
            project_row.on_hover_text(tilde(path));
        }

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.add_space(9.0);
            ui.add(
                label("Sessions", theme::strong(theme::SMALL), palette.ink_muted).selectable(false),
            );
        });
        ui.add_space(4.0);

        let footer = 2.0 * 30.0 + 2.0 + 12.0;
        let mut selected = None;
        let mut stop = None;
        let mut toggle = None;
        let mut show_all = None;
        // Sessions grouped by project folder: the current project first and open, then the
        // others by their latest session, closed until clicked.
        let current = self.settings.project.clone();
        let mut groups: Vec<(PathBuf, Vec<&crate::storage::Session>)> = Vec::new();
        for session in &self.sessions {
            match groups
                .iter_mut()
                .find(|(path, _)| *path == session.project_path)
            {
                Some((_, list)) => list.push(session),
                None => groups.push((session.project_path.clone(), vec![session])),
            }
        }
        if let Some(current) = &current
            && let Some(index) = groups.iter().position(|(path, _)| path == current)
        {
            let group = groups.remove(index);
            groups.insert(0, group);
        }
        ScrollArea::vertical()
            .max_height((ui.available_height() - footer).max(40.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                let active = self.active.as_ref().map(|session| session.id);
                let here = current.as_ref();
                let current_has_sessions =
                    groups.first().is_some_and(|(path, _)| Some(path) == here);
                if !current_has_sessions {
                    ui.horizontal(|ui| {
                        ui.add_space(9.0);
                        let note = if here.is_some() {
                            "No saved sessions in this project."
                        } else {
                            "Sessions appear here once a project is chosen."
                        };
                        ui.add(label(note, theme::text(theme::SMALL), palette.ink_faint).wrap());
                    });
                    ui.add_space(6.0);
                }
                for (path, sessions) in &groups {
                    let is_current = Some(path) == here;
                    let open = is_current != self.sidebar_toggled.contains(path);
                    let name = path
                        .file_name()
                        .map_or_else(|| tilde(path), |name| name.to_string_lossy().into_owned());
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    if response.hovered() {
                        ui.painter().rect_filled(rect, radius(6), palette.hover);
                    }
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(
                            egui::pos2(rect.left() + 15.0, rect.center().y),
                            vec2(10.0, 10.0),
                        ),
                        if open {
                            Icon::ChevronDown
                        } else {
                            Icon::Forward
                        },
                        palette.ink_faint,
                    );
                    let title = one_line(
                        ui,
                        &name,
                        theme::strong(theme::SMALL),
                        if is_current {
                            palette.ink
                        } else {
                            palette.ink_muted
                        },
                        rect.width() - 64.0,
                    );
                    ui.painter().galley(
                        egui::pos2(rect.left() + 28.0, rect.center().y - title.size().y / 2.0),
                        title,
                        palette.ink,
                    );
                    let running_inside = sessions.iter().any(|session| {
                        (active == Some(session.id) && self.is_running())
                            || self.parked.get(&session.id).is_some_and(Parked::is_running)
                    });
                    if running_inside && !open {
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(50));
                        crate::working::paint(
                            ui.painter(),
                            Rect::from_center_size(
                                egui::pos2(rect.right() - 16.0, rect.center().y),
                                vec2(14.0, 14.0),
                            ),
                            crate::working::Sign::Wait,
                            ui.input(|input| input.time),
                            palette.accent,
                            palette.accent.gamma_multiply(0.42),
                            false,
                        );
                    } else {
                        ui.painter().text(
                            egui::pos2(rect.right() - 9.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            sessions.len().to_string(),
                            theme::text(theme::SMALL - 1.0),
                            palette.ink_faint,
                        );
                    }
                    if response.on_hover_text(tilde(path)).clicked() {
                        toggle = Some(path.clone());
                    }
                    if !open {
                        continue;
                    }
                    let limit = if self.sidebar_all.contains(path) {
                        usize::MAX
                    } else {
                        8
                    };
                    for session in sessions.iter().take(limit) {
                        let now_open = self.view == View::Chat && active == Some(session.id);
                        let age = ago(session.age());
                        let doing_here = if active == Some(session.id) && self.is_running() {
                            Some(
                                self.doing
                                    .as_ref()
                                    .map_or("Working", |(doing, _)| doing.as_str()),
                            )
                        } else {
                            self.parked.get(&session.id).and_then(|parked| {
                                parked.is_running().then(|| {
                                    parked
                                        .doing
                                        .as_ref()
                                        .map_or("Working", |(doing, _)| doing.as_str())
                                })
                            })
                        };
                        let mark = match (doing_here, self.parked.get(&session.id)) {
                            (Some(doing), _) => Mark::Working(doing),
                            (None, Some(parked)) => match parked.status {
                                Status::Failed => Mark::News(palette.danger),
                                Status::Stopped => Mark::News(palette.warn),
                                _ => Mark::News(palette.ok),
                            },
                            (None, None) => Mark::None,
                        };
                        let response = row_marked(
                            ui,
                            palette,
                            None,
                            session.title(),
                            Some(&age),
                            now_open,
                            true,
                            mark,
                        );
                        if response.clicked() {
                            selected = Some((*session).clone());
                        }
                        if matches!(mark, Mark::Working(_)) {
                            response.context_menu(|ui| {
                                if ui.button("Stop Task").clicked() {
                                    stop = Some(session.id);
                                    ui.close();
                                }
                            });
                        } else {
                            response.on_hover_text(session.title());
                        }
                    }
                    if sessions.len() > limit {
                        ui.horizontal(|ui| {
                            ui.add_space(9.0);
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(format!(
                                            "Show {} more",
                                            sessions.len() - limit
                                        ))
                                        .font(theme::text(theme::SMALL))
                                        .color(palette.ink_faint),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                show_all = Some(path.clone());
                            }
                        });
                    }
                    ui.add_space(6.0);
                }
            });
        if let Some(path) = toggle
            && !self.sidebar_toggled.remove(&path)
        {
            self.sidebar_toggled.insert(path);
        }
        if let Some(path) = show_all {
            self.sidebar_all.insert(path);
        }
        if let Some(id) = stop {
            self.stop_session(id);
        }
        if let Some(session) = selected {
            // A session from another project opens that project too.
            if self.settings.project.as_ref() != Some(&session.project_path) {
                self.choose_project(session.project_path.clone());
            }
            self.select_session(session);
        }

        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            if row(
                ui,
                palette,
                Some(Icon::Sliders),
                "Settings",
                None,
                self.view == View::Settings,
                true,
            )
            .clicked()
            {
                self.open(View::Settings);
            }
            if row(
                ui,
                palette,
                Some(Icon::Bars),
                "Usage",
                None,
                self.view == View::Usage,
                true,
            )
            .clicked()
            {
                self.open(View::Usage);
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui, palette: &Palette) {
        let small = theme::text(theme::SMALL);
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 7.0;
            if self.is_installing() {
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
                crate::working::paint(
                    ui.painter(),
                    rect,
                    crate::working::Sign::Wait,
                    ui.input(|input| input.time),
                    palette.accent,
                    palette.accent.gamma_multiply(0.42),
                    self.settings.reduce_motion(),
                );
                ui.add(
                    label("Installing OpenCode…", small.clone(), palette.ink_muted)
                        .selectable(false),
                );
            } else {
                match self.opencode() {
                    Some(_) => {
                        status_dot(ui, palette.ok);
                        ui.add(
                            label("OpenCode ready", small.clone(), palette.ink_muted)
                                .selectable(false),
                        );
                    }
                    None => {
                        status_dot(ui, palette.warn);
                        let link = ui.add(
                            label("OpenCode isn’t installed", small.clone(), palette.warn)
                                .selectable(false)
                                .sense(Sense::click()),
                        );
                        if link.clicked() {
                            self.open(View::Settings);
                            self.settings_section = Section::OpenCode;
                        }
                    }
                }
            }
            if let Some(path) = &self.settings.project {
                ui.add_space(8.0);
                ui.add(label(tilde(path), small.clone(), palette.ink_faint).truncate());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let text = match &self.usage {
                    Ok(rows) if rows.is_empty() => "No steps recorded today".to_owned(),
                    Ok(rows) => {
                        let steps: i64 = rows.iter().map(|row| row.steps).sum();
                        let tokens: i64 = rows.iter().map(Usage::tokens).sum();
                        format!(
                            "Today: {} {} · {} tokens",
                            grouped(steps),
                            if steps == 1 { "step" } else { "steps" },
                            compact(tokens)
                        )
                    }
                    Err(_) => "Usage unavailable".to_owned(),
                };
                let link = ui.add(
                    label(text, small, palette.ink_muted)
                        .selectable(false)
                        .sense(Sense::click()),
                );
                if link.on_hover_text("Open usage").clicked() {
                    self.open(View::Usage);
                }
            });
        });
    }

    pub(crate) fn header(&mut self, ui: &mut Ui, palette: &Palette, title: &str) {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            Stroke::new(1.0, palette.line),
        );
        let mut right = self.pane_buttons(ui, palette, rect) - 14.0;
        let painter = ui.painter();
        if self.view == View::Chat && self.status != Status::Ready {
            let galley = painter.layout_no_wrap(
                self.status.label().to_owned(),
                theme::text(theme::SMALL),
                palette.ink_muted,
            );
            right -= galley.size().x;
            painter.galley(
                egui::pos2(right, rect.center().y - galley.size().y / 2.0),
                galley,
                palette.ink_muted,
            );
            if let Some(color) = status_color(palette, self.status) {
                let strength = if matches!(self.status, Status::Running | Status::Stopping) {
                    breath(ui)
                } else {
                    1.0
                };
                right -= 11.0;
                painter.circle_filled(
                    egui::pos2(right, rect.center().y),
                    3.5,
                    color.gamma_multiply(strength),
                );
            }
            right -= 16.0;
        }
        let left = rect.left() + 24.0;
        let galley = one_line(
            ui,
            title,
            theme::strong(theme::TEXT + 0.5),
            palette.ink,
            right - left,
        );
        ui.painter().galley(
            egui::pos2(left, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.ink,
        );
    }

    fn chat(&mut self, ui: &mut Ui, palette: &Palette) {
        ui.spacing_mut().item_spacing.y = 0.0;
        let title = self
            .active
            .as_ref()
            .map_or("New session".to_owned(), |session| {
                session.title().to_owned()
            });
        self.header(ui, palette, &title);
        egui::Panel::bottom("composer")
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(palette.ground).inner_margin(Margin {
                left: 24,
                right: 24,
                top: 6,
                bottom: 14,
            }))
            .show(ui, |ui| column(ui, |ui| self.composer(ui, palette)));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(palette.ground))
            .show(ui, |ui| match &self.active {
                Some(_) => self.transcript(ui, palette),
                None => self.welcome(ui, palette),
            });
    }

    fn welcome(&mut self, ui: &mut Ui, palette: &Palette) {
        let has_project = self.settings.project.is_some();
        let shows_activity =
            has_project && matches!(&self.history, Some(Ok(history)) if !history.steps.is_empty());
        let height = match (has_project, shows_activity) {
            (true, true) => 330.0,
            (true, false) => 150.0,
            _ => 196.0,
        };
        ui.add_space(((ui.available_height() - height) / 2.0 - 12.0).max(16.0));
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.image((self.logo.id(), vec2(44.0, 44.0)));
            ui.add_space(18.0);
            let heading = if has_project {
                "What are we building?"
            } else {
                "Choose a project to start"
            };
            ui.add(label(heading, theme::strong(theme::TITLE), palette.ink).selectable(false));
            ui.add_space(8.0);
            let detail = if has_project {
                "OpenCode works in this folder with its own tools, on a free model."
            } else {
                "OpenCode reads, edits, and runs commands in the folder you choose."
            };
            ui.add(label(detail, theme::text(theme::READING), palette.ink_muted).selectable(false));
            if shows_activity {
                ui.add_space(30.0);
                self.activity_card(ui, palette);
            }
            if !has_project {
                ui.add_space(20.0);
                let button = egui::Button::new(
                    RichText::new("Choose Project Folder…")
                        .font(theme::strong(theme::TEXT))
                        .color(palette.on_accent),
                )
                .fill(palette.accent)
                .stroke(Stroke::NONE)
                .min_size(vec2(0.0, 34.0));
                if ui.add(button).clicked() {
                    self.pick_project();
                }
            }
            if let Some(error) = &self.task_error {
                ui.add_space(16.0);
                ui.add(label(error.clone(), theme::text(theme::TEXT), palette.danger).wrap());
            }
        });
    }

    fn transcript(&mut self, ui: &mut Ui, palette: &Palette) {
        let Some(session) = &self.active else { return };
        // Previews for sent pictures, loaded before drawing so the session stays borrowed
        // only for reading.
        let ctx = ui.ctx().clone();
        let mut previews = std::collections::HashMap::new();
        for item in session.turns.iter().flat_map(|turn| &turn.attachments) {
            if let Some(path) = item.original()
                && let Some(texture) = self.inputs.thumbnail(&ctx, path)
            {
                previews.insert(path.to_path_buf(), texture);
            }
        }
        // Pictures OpenCode read show as thumbnails in the reply.
        let project = session.project_path.clone();
        let mut thumbs = std::collections::HashMap::new();
        for turn in &session.turns {
            for path in markdown::image_paths(&turn.output, &project) {
                if let Some(texture) = self.inputs.thumbnail(&ctx, &path) {
                    thumbs.insert(path, texture);
                }
            }
        }
        let files = markdown::Files {
            project: Some(project.as_path()),
            thumbs: &thumbs,
        };
        let mut open = None;
        let running = self.is_running();
        let doing = self.doing.clone();
        let task_started = self.task_started;
        let transcript = self.settings.transcript;
        let still = self.settings.reduce_motion();
        let failed = self.status == Status::Failed && !running;
        let can_continue = failed && self.limited && self.can_fall_back();
        let backup = models::find(&self.settings.backup_model);
        let backup_name = backup
            .as_ref()
            .map_or_else(|| "the backup".to_owned(), |model| model.name.clone());
        let shares = backup
            .filter(|backup| backup.provider != models::at(self.route).provider)
            .map(|backup| {
                format!(
                    "Continuing shares this session with {}.",
                    backup.provider_name()
                )
            });
        let mut retry = None;
        let mut act = None;
        ScrollArea::vertical()
            .id_salt(session.id)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                Frame::new()
                    .inner_margin(Margin {
                        left: 24,
                        right: 24,
                        top: 22,
                        bottom: 18,
                    })
                    .show(ui, |ui| {
                        column(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let last = session.turns.len().saturating_sub(1);
                            for (index, turn) in session.turns.iter().enumerate() {
                                if index > 0 {
                                    ui.add_space(30.0);
                                }
                                prompt(ui, palette, &turn.prompt);
                                if !turn.attachments.is_empty() {
                                    ui.add_space(8.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                                        for item in &turn.attachments {
                                            let kind = crate::inputs::sent_kind(item);
                                            let original = item.original();
                                            let type_label = original.map_or_else(
                                                || kind.noun().to_owned(),
                                                |path| crate::attach::type_label(path, kind),
                                            );
                                            let clicks = attachment_card(
                                                ui,
                                                palette,
                                                SENT_CARD,
                                                Card {
                                                    kind,
                                                    name: &item.name,
                                                    type_label: &type_label,
                                                    note: &item.note,
                                                    tone: Tone::Quiet,
                                                    preview: original
                                                        .and_then(|path| previews.get(path)),
                                                },
                                                palette.raised,
                                                false,
                                            );
                                            if clicks.open {
                                                open = original.map(Path::to_path_buf);
                                            }
                                        }
                                    });
                                }
                                ui.add_space(12.0);
                                ui.add(
                                    label(
                                        &turn.model,
                                        theme::strong(theme::SMALL),
                                        palette.ink_faint,
                                    )
                                    .selectable(false),
                                );
                                ui.add_space(6.0);
                                let live = running && index == last;
                                if !turn.output.is_empty() {
                                    let clicked = ui
                                        .scope(|ui| {
                                            markdown::show(
                                                ui,
                                                palette,
                                                &turn.output,
                                                transcript,
                                                &files,
                                            )
                                        })
                                        .inner;
                                    if clicked.is_some() {
                                        open = clicked;
                                    }
                                } else if !live {
                                    ui.add(label(
                                        "No output.",
                                        theme::text(theme::READING),
                                        palette.ink_faint,
                                    ));
                                }
                                if live {
                                    if !turn.output.is_empty() {
                                        ui.add_space(10.0);
                                    }
                                    let (doing, since) = doing.clone().unwrap_or_else(|| {
                                        ("Starting OpenCode".to_owned(), task_started)
                                    });
                                    activity_line(ui, palette, &doing, since, task_started, still);
                                } else if !turn.output.trim().is_empty() {
                                    ui.add_space(8.0);
                                    let reading = self.inputs.reading == Some(turn.id);
                                    if let Some(action) = actions(ui, palette, turn.id, reading) {
                                        act = Some((action, turn.id, turn.output.clone()));
                                    }
                                }
                            }
                            for error in [&self.task_error, &self.storage_error]
                                .into_iter()
                                .flatten()
                            {
                                ui.add_space(18.0);
                                problem(ui, palette, error);
                            }
                            if failed {
                                ui.add_space(12.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;
                                    if can_continue {
                                        let onward = egui::Button::new(
                                            RichText::new(format!("Continue on {backup_name}"))
                                                .font(theme::strong(theme::TEXT))
                                                .color(palette.on_accent),
                                        )
                                        .fill(palette.accent)
                                        .stroke(Stroke::NONE);
                                        if ui.add(onward).clicked() {
                                            retry = Some(true);
                                        }
                                    }
                                    if ui.button("Try Again").clicked() {
                                        retry = Some(false);
                                    }
                                    if can_continue && let Some(shares) = &shares {
                                        ui.add(
                                            label(
                                                shares.clone(),
                                                theme::text(theme::SMALL),
                                                palette.ink_faint,
                                            )
                                            .selectable(false),
                                        );
                                    }
                                });
                            }
                        });
                    });
            });
        if let Some(on_backup) = retry {
            self.retry(ui.ctx(), on_backup);
        }
        if let Some(path) = open {
            if path.exists() {
                if let Err(message) = open_path(&path) {
                    self.inputs.notice = Some(message);
                }
            } else {
                self.inputs.notice = Some(format!("{} is no longer there.", path.display()));
            }
        }
        match act {
            Some((Action::Read, id, output)) => self.read_aloud(id, &output),
            Some((Action::StopReading, ..)) => self.stop_reading(),
            Some((Action::Copy, _, output)) => {
                ui.ctx().copy_text(crate::activity::without_steps(&output));
            }
            None => {}
        }
    }

    fn settings_page(&mut self, ui: &mut Ui, palette: &Palette) {
        ui.spacing_mut().item_spacing.y = 0.0;
        self.header(ui, palette, "Settings");
        let query = self.settings_query.trim().to_lowercase();
        let shown: Vec<Section> = Section::ALL
            .into_iter()
            .filter(|section| query.is_empty() || section.matches(&query))
            .collect();
        if !shown.contains(&self.settings_section)
            && let Some(first) = shown.first()
        {
            self.settings_section = *first;
        }
        let height = ui.available_height();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            // The categories, with a search box over them.
            ui.allocate_ui_with_layout(vec2(210.0, height), Layout::top_down(Align::Min), |ui| {
                Frame::new()
                    .inner_margin(Margin {
                        left: 14,
                        right: 10,
                        top: 16,
                        bottom: 10,
                    })
                    .show(ui, |ui| {
                        ui.set_min_height(height - 26.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add(
                            egui::TextEdit::singleline(&mut self.settings_query)
                                .hint_text("Search settings")
                                .desired_width(f32::INFINITY)
                                .margin(Margin::symmetric(8, 6)),
                        );
                        ui.add_space(10.0);
                        for section in &shown {
                            if row(
                                ui,
                                palette,
                                Some(section.icon()),
                                section.name(),
                                None,
                                self.settings_section == *section,
                                true,
                            )
                            .clicked()
                            {
                                self.settings_section = *section;
                                self.scroll_settings_to = Some(0.0);
                            }
                        }
                        if shown.is_empty() {
                            ui.add(
                                label(
                                    "No setting matches.",
                                    theme::text(theme::SMALL),
                                    palette.ink_faint,
                                )
                                .wrap(),
                            );
                        }
                    });
            });
            let (line, _) = ui.allocate_exact_size(vec2(1.0, height), Sense::hover());
            ui.painter().vline(
                line.center().x,
                line.y_range(),
                Stroke::new(1.0, palette.line),
            );
            let width = ui.available_width();
            ui.allocate_ui_with_layout(vec2(width, height), Layout::top_down(Align::Min), |ui| {
                ui.set_max_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                if shown.is_empty() {
                    return;
                }
                let mut area = ScrollArea::vertical()
                    .id_salt(("settings", self.settings_section))
                    .auto_shrink([false, false]);
                if let Some(offset) = self.scroll_settings_to.take() {
                    area = area.vertical_scroll_offset(offset);
                }
                area.show(ui, |ui| {
                    Frame::new()
                        .inner_margin(Margin::symmetric(28, 22))
                        .show(ui, |ui| {
                            ui.set_max_width((width - 56.0).clamp(240.0, 700.0));
                            ui.spacing_mut().item_spacing.y = 0.0;
                            match self.settings_section {
                                Section::Models => self.model_settings(ui, palette),
                                Section::Providers => self.provider_settings(ui, palette),
                                Section::Tasks => self.task_settings(ui, palette),
                                Section::Voice => {
                                    heading(ui, palette, "Voice");
                                    self.voice_settings(ui, palette);
                                }
                                Section::Appearance => self.appearance_settings(ui, palette),
                                Section::OpenCode => self.opencode_settings(ui, palette),
                                Section::Storage => self.storage_settings(ui, palette),
                                Section::Help => {
                                    self.help_settings(ui, palette);
                                    ui.add_space(30.0);
                                    ui.add(label(
                                        format!("{NAME} {}", env!("CARGO_PKG_VERSION")),
                                        theme::text(theme::SMALL),
                                        palette.ink_faint,
                                    ));
                                }
                            }
                        });
                });
            });
        });
    }

    fn model_settings(&mut self, ui: &mut Ui, palette: &Palette) {
        heading(ui, palette, "Models");
        let idle = !self.is_running();
        setting(
            ui,
            palette,
            "Choosing the model",
            "Manual runs every task on the model you choose in the composer. Auto looks at each task on this computer and picks one of the three models below: Quick for short questions and small edits, Hard for larger changes, investigations, long prompts, several files, and Plan mode, and Everyday for the rest. A task with pictures or video goes to one of them that reads them. Each reply names the model it ran on and why.",
        );
        let mode = self.settings.model_mode;
        if let Some(chosen) = ui
            .add_enabled_ui(idle, |ui| {
                segmented(
                    ui,
                    palette,
                    mode,
                    &[
                        (crate::auto::Mode::Manual, "Manual"),
                        (crate::auto::Mode::Auto, "Auto"),
                    ],
                )
            })
            .inner
        {
            self.settings.model_mode = chosen;
            self.save_settings();
        }
        if self.settings.model_mode == crate::auto::Mode::Auto {
            ui.add_space(12.0);
            for tier in [
                crate::auto::Tier::Quick,
                crate::auto::Tier::Everyday,
                crate::auto::Tier::Hard,
            ] {
                self.auto_tier_setting(ui, palette, tier, idle);
            }
        }
        ui.add_space(18.0);
        setting(
            ui,
            palette,
            "Default model",
            "In Manual, new sessions start on this model. A saved session reopens on the model it last used.",
        );
        let current = crate::models::index_of(&self.settings.default_model).unwrap_or_default();
        let chosen = ui
            .add_enabled_ui(idle, |ui| {
                crate::composer::model_menu(ui, palette, "default-model", current, 230.0)
            })
            .inner;
        let (chosen, connect) = chosen;
        if let Some(provider) = connect {
            self.begin_connect(provider);
        }
        if chosen != current {
            self.settings.default_model = models::at(chosen).id;
            self.save_settings();
        }
        ui.add_space(18.0);
        setting(
            ui,
            palette,
            "Backup after a rate limit",
            "A task continues on this model when its model is rate limited and the option under the composer is on. Continuing on another provider shares the session with that provider.",
        );
        let current = self.backup_route().unwrap_or_default();
        let chosen = ui
            .add_enabled_ui(idle, |ui| {
                crate::composer::model_menu(ui, palette, "backup-model", current, 230.0)
            })
            .inner;
        let (chosen, connect) = chosen;
        if let Some(provider) = connect {
            self.begin_connect(provider);
        }
        if chosen != current || self.backup_route().is_none() {
            self.settings.backup_model = models::at(chosen).id;
            self.save_settings();
        }
        ui.add_space(18.0);
        setting(
            ui,
            palette,
            "Model list",
            "The list holds every model OpenCode can use that is priced at $0 and supports tools: OpenCode’s own, OpenRouter’s free models, free models from providers you connect, and models running on this computer in Ollama, LM Studio, or vLLM. OpenRouter prices are checked again before each run. Plans billed by subscription, such as GitHub Copilot, are left out.",
        );
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let busy = self.discovery.is_some();
            if ui
                .add_enabled(!busy, egui::Button::new("Look for Models Again"))
                .clicked()
            {
                self.start_discovery(ui.ctx());
            }
            if busy {
                ui.add(egui::Spinner::new().size(14.0).color(palette.accent));
            }
            ui.add(
                label(
                    self.discovery_status.clone(),
                    theme::text(theme::SMALL),
                    palette.ink_muted,
                )
                .wrap(),
            );
        });
    }

    /// The tutorial, and who made the app.
    fn help_settings(&mut self, ui: &mut Ui, palette: &Palette) {
        heading(ui, palette, "Help");
        setting(
            ui,
            palette,
            "Tutorial",
            "Installing OpenCode, choosing a project and how models are picked, adding free models with a free key, and using the composer.",
        );
        if ui.button("Show Tutorial").clicked() {
            self.show_tutorial();
        }
        ui.add_space(18.0);
        setting(ui, palette, "Made by", "");
        let link = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let (rect, _) = ui.allocate_exact_size(vec2(18.0, 20.0), Sense::hover());
                icons::paint(
                    ui.painter(),
                    Rect::from_center_size(rect.center(), vec2(17.0, 17.0)),
                    Icon::GitHub,
                    palette.ink,
                );
                ui.hyperlink_to(
                    RichText::new("desmondknunoo")
                        .font(theme::strong(theme::TEXT))
                        .color(palette.accent),
                    "https://github.com/desmondknunoo",
                )
            })
            .inner;
        link.on_hover_text("github.com/desmondknunoo");
        ui.add_space(18.0);
        setting(
            ui,
            palette,
            "Free model directory",
            "A wider catalogue of free LLM APIs and keys than OpenCode supports.",
        );
        let link = ui.hyperlink_to(
            RichText::new("awesome-free-llm-apis")
                .font(theme::strong(theme::TEXT))
                .color(palette.accent),
            "https://github.com/open-free-llm-api/awesome-freellm-apis",
        );
        link.on_hover_text("github.com/open-free-llm-api/awesome-freellm-apis");
    }

    /// One of Auto's tiers: its model and, where the model offers them, its effort level.
    fn auto_tier_setting(
        &mut self,
        ui: &mut Ui,
        palette: &Palette,
        tier: crate::auto::Tier,
        idle: bool,
    ) {
        let choice = self.settings.auto_models.of(tier).clone();
        let current = models::index_of(&choice.model).unwrap_or_default();
        let mut changed = None;
        let mut connect = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (rect, _) = ui.allocate_exact_size(vec2(76.0, 30.0), Sense::hover());
            ui.painter().text(
                rect.left_center(),
                Align2::LEFT_CENTER,
                tier.name(),
                theme::strong(theme::TEXT),
                palette.ink,
            );
            ui.add_enabled_ui(idle, |ui| {
                let (chosen, wants) = crate::composer::model_menu(
                    ui,
                    palette,
                    &format!("auto-{}", tier.name()),
                    current,
                    230.0,
                );
                connect = wants;
                if chosen != current {
                    changed = Some(crate::auto::Choice {
                        model: models::at(chosen).id,
                        effort: None,
                    });
                }
                let model = models::at(current);
                if !model.variants.is_empty() {
                    let shown = choice
                        .effort
                        .as_deref()
                        .map_or_else(|| "Default effort".to_owned(), models::effort_label);
                    egui::ComboBox::from_id_salt(("auto-effort", tier.name()))
                        .selected_text(shown)
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(choice.effort.is_none(), "Default effort")
                                .clicked()
                            {
                                changed = Some(crate::auto::Choice {
                                    model: choice.model.clone(),
                                    effort: None,
                                });
                            }
                            for variant in &model.variants {
                                let picked = choice.effort.as_deref() == Some(variant.as_str());
                                if ui
                                    .selectable_label(picked, models::effort_label(variant))
                                    .clicked()
                                {
                                    changed = Some(crate::auto::Choice {
                                        model: choice.model.clone(),
                                        effort: Some(variant.clone()),
                                    });
                                }
                            }
                        });
                }
            });
        });
        ui.add_space(6.0);
        if let Some(provider) = connect {
            self.begin_connect(provider);
        }
        if let Some(choice) = changed {
            *self.settings.auto_models.of_mut(tier) = choice;
            self.save_settings();
        }
    }

    /// The providers whose free models need an account: connect, disconnect, and where to
    /// get a key.
    fn provider_settings(&mut self, ui: &mut Ui, palette: &Palette) {
        heading(ui, palette, "Free accounts");
        let busy = self.provider_task.is_some();
        if let Some((message, failed)) = &self.provider_message {
            let color = if *failed { palette.danger } else { palette.ok };
            ui.add(label(message.clone(), theme::text(theme::SMALL), color).wrap());
            ui.add_space(8.0);
        }
        let mut action: Option<(&'static str, &'static str)> = None;
        for provider in crate::providers::CONNECTABLE {
            let connection = self.is_connected(provider.id).cloned();
            Frame::new()
                .fill(palette.raised)
                .stroke(Stroke::new(1.0, palette.line))
                .corner_radius(radius(theme::RADIUS_SURFACE))
                .inner_margin(Margin::symmetric(14, 11))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.add(
                            label(provider.name, theme::strong(theme::TEXT), palette.ink)
                                .selectable(false)
                                .sense(Sense::hover()),
                        )
                        .on_hover_text(format!("Adds {}.", provider.models));
                        let (state, color) = match &connection {
                            Some(_) => ("Connected", palette.ok),
                            None => ("Not connected", palette.ink_faint),
                        };
                        let (dot, _) = ui.allocate_exact_size(vec2(8.0, 18.0), Sense::hover());
                        ui.painter().circle_filled(dot.center(), 3.5, color);
                        ui.add(
                            label(state, theme::text(theme::SMALL), palette.ink_muted)
                                .selectable(false),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            match &connection {
                                Some(found) if found.added_here() => {
                                    if ui
                                        .add_enabled(!busy, egui::Button::new("Disconnect"))
                                        .clicked()
                                    {
                                        action = Some((provider.id, "disconnect"));
                                    }
                                    if ui
                                        .add_enabled(!busy, egui::Button::new("Change Key…"))
                                        .clicked()
                                    {
                                        action = Some((provider.id, "connect"));
                                    }
                                }
                                Some(_) => {
                                    ui.add(
                                        label(
                                            "Connected in OpenCode",
                                            theme::text(theme::SMALL),
                                            palette.ink_faint,
                                        )
                                        .selectable(false),
                                    );
                                }
                                None if self.connecting != Some(provider.id) => {
                                    let connect = egui::Button::new(
                                        RichText::new("Connect…")
                                            .font(theme::strong(theme::SMALL))
                                            .color(palette.on_accent),
                                    )
                                    .fill(palette.accent)
                                    .stroke(Stroke::NONE);
                                    if ui.add_enabled(!busy, connect).clicked() {
                                        action = Some((provider.id, "connect"));
                                    }
                                }
                                None => {}
                            }
                        });
                    });
                    // The models it adds, on hover over the name, keep the card short.
                    if self.connecting == Some(provider.id) {
                        ui.add_space(12.0);
                        let mut entered = false;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            let save_width = 78.0;
                            let field = egui::TextEdit::singleline(&mut self.key_input)
                                .password(true)
                                .hint_text(
                                    RichText::new(format!("{} key", provider.name))
                                        .color(palette.ink_faint),
                                )
                                .font(theme::text(theme::TEXT))
                                .margin(Margin::symmetric(10, 8))
                                .background_color(palette.ground)
                                .desired_width(ui.available_width() - save_width - 8.0);
                            let field = ui.add(field);
                            if !field.has_focus() && self.key_input.is_empty() {
                                field.request_focus();
                            }
                            entered = field.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter));
                            let ready = !self.key_input.trim().is_empty() && !busy;
                            let (fill, ink) = if ready {
                                (palette.accent, palette.on_accent)
                            } else {
                                (palette.hover, palette.ink_faint)
                            };
                            let save = egui::Button::new(
                                RichText::new("Save")
                                    .font(theme::strong(theme::TEXT))
                                    .color(ink),
                            )
                            .fill(fill)
                            .stroke(Stroke::NONE)
                            .corner_radius(radius(8))
                            .min_size(vec2(save_width, 34.0));
                            if ui.add_enabled(ready, save).clicked() || (entered && ready) {
                                action = Some((provider.id, "save"));
                            }
                        });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 14.0;
                            ui.hyperlink_to(
                                RichText::new("Get a free key ↗").font(theme::text(theme::SMALL)),
                                provider.key_page,
                            )
                            .on_hover_text(provider.note);
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Cancel")
                                            .font(theme::text(theme::SMALL))
                                            .color(palette.ink_muted),
                                    )
                                    .frame(false),
                                )
                                .clicked()
                            {
                                action = Some((provider.id, "cancel"));
                            }
                        });
                    }
                    if busy && self.provider_task.is_some() && action.is_none() {
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(200));
                    }
                });
            ui.add_space(8.0);
        }
        if busy {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0).color(palette.accent));
                ui.add(
                    label(
                        "Talking to OpenCode…",
                        theme::text(theme::SMALL),
                        palette.ink_muted,
                    )
                    .selectable(false),
                );
            });
        }
        match action {
            Some((provider, "connect")) => {
                self.key_input.clear();
                self.provider_message = None;
                self.connecting = Some(provider);
            }
            Some((_, "save")) => self.save_key(ui.ctx()),
            Some((_, "cancel")) => self.cancel_connect(),
            Some((provider, "disconnect")) => self.disconnect(ui.ctx(), provider),
            _ => {}
        }
    }

    fn task_settings(&mut self, ui: &mut Ui, palette: &Palette) {
        heading(ui, palette, "Tasks");
        setting(
            ui,
            palette,
            "Time limit",
            "Stops a task that runs longer than this, so a stuck command can’t run all day.",
        );
        if let Some(minutes) = segmented(
            ui,
            palette,
            self.settings.time_limit_minutes,
            &[
                (0, "None"),
                (10, "10 min"),
                (20, "20 min"),
                (30, "30 min"),
                (60, "1 hour"),
            ],
        ) {
            self.settings.time_limit_minutes = minutes;
            self.save_settings();
        }

        ui.add_space(18.0);
        setting(
            ui,
            palette,
            "Access",
            "What OpenCode may do without asking. A task can’t stop to ask you, so anything not allowed is declined and the task carries on. The same choice is under Access in the composer.",
        );
        if let Some(full) = segmented(
            ui,
            palette,
            self.settings.auto_approve,
            &[(false, "Standard access"), (true, "Full access")],
        ) {
            self.choose_access(ui.ctx(), full);
        }
        ui.add_space(18.0);
        setting(
            ui,
            palette,
            "Tasks at once",
            "How many tasks may run at the same time, across sessions and projects. Free models limit requests per account, so more at once reaches those limits sooner.",
        );
        if let Some(count) = segmented(
            ui,
            palette,
            self.settings.max_tasks,
            &[(1, "1"), (2, "2"), (3, "3"), (4, "4"), (6, "6")],
        ) {
            self.settings.max_tasks = count;
            self.save_settings();
        }

        ui.add_space(18.0);
        let mut awake = self.settings.keep_awake;
        if checkbox(
            ui,
            palette,
            &mut awake,
            "Keep the computer awake during tasks",
            crate::awake::SUPPORTED,
        )
        .on_hover_text(
            "The display can still turn off, and closing a laptop still puts it to sleep.",
        )
        .changed()
        {
            self.settings.keep_awake = awake;
            self.save_settings();
        }
        ui.add_space(12.0);
        let mut notify = self.settings.notify;
        if checkbox(
            ui,
            palette,
            &mut notify,
            "Notify me when a task finishes",
            true,
        )
        .changed()
        {
            self.settings.notify = notify;
            self.save_settings();
        }

        ui.add_space(18.0);
        let (enter, modified) = if cfg!(target_os = "macos") {
            ("Return sends", "⌘Return sends")
        } else {
            ("Enter sends", "Ctrl+Enter sends")
        };
        setting(
            ui,
            palette,
            "Sending a task",
            "The other key combination adds a new line.",
        );
        if let Some(choice) = segmented(
            ui,
            palette,
            self.settings.send_with_modifier,
            &[(false, enter), (true, modified)],
        ) {
            self.settings.send_with_modifier = choice;
            self.save_settings();
        }
    }

    fn appearance_settings(&mut self, ui: &mut Ui, palette: &Palette) {
        heading(ui, palette, "Appearance");
        setting(ui, palette, "Theme", "");
        if let Some(choice) = segmented(
            ui,
            palette,
            self.settings.appearance,
            &[
                (Appearance::System, "Match System"),
                (Appearance::Light, "Light"),
                (Appearance::Dark, "Dark"),
            ],
        ) {
            self.settings.appearance = choice;
            self.save_settings();
            self.apply_appearance(ui.ctx());
        }
        ui.add_space(18.0);
        setting(ui, palette, "Text and controls", "");
        if let Some(choice) = segmented(
            ui,
            palette,
            self.settings.scale,
            &[
                (Scale::Smaller, "Smaller"),
                (Scale::Normal, "Default"),
                (Scale::Larger, "Larger"),
                (Scale::Largest, "Largest"),
            ],
        ) {
            self.settings.scale = choice;
            self.save_settings();
            self.apply_appearance(ui.ctx());
        }
        ui.add_space(18.0);
        setting(
            ui,
            palette,
            "Transcript",
            "Normal shows replies and each step OpenCode took. Thinking adds how long the model thought before each step. Verbose also shows every command and path in full.",
        );
        if let Some(choice) = segmented(
            ui,
            palette,
            self.settings.transcript,
            &[
                (crate::storage::Transcript::Normal, "Normal"),
                (crate::storage::Transcript::Thinking, "Thinking"),
                (crate::storage::Transcript::Verbose, "Verbose"),
            ],
        ) {
            self.settings.transcript = choice;
            self.save_settings();
        }
        ui.add_space(18.0);
        setting(ui, palette, "Conversation width", "");
        if let Some(choice) = segmented(
            ui,
            palette,
            self.settings.width,
            &[
                (crate::storage::Width::Narrow, "Narrow"),
                (crate::storage::Width::Wide, "Wide"),
                (crate::storage::Width::Full, "Full"),
            ],
        ) {
            self.settings.width = choice;
            self.save_settings();
        }
        ui.add_space(18.0);
        setting(
            ui,
            palette,
            "Motion",
            "Reduced keeps the working dots and loading bars still. Match System follows Reduce motion in the system’s accessibility settings.",
        );
        if let Some(choice) = segmented(
            ui,
            palette,
            self.settings.motion,
            &[
                (crate::storage::Motion::System, "Match System"),
                (crate::storage::Motion::Reduced, "Reduced"),
                (crate::storage::Motion::Full, "Full"),
            ],
        ) {
            self.settings.motion = choice;
            self.save_settings();
        }
        ui.add_space(18.0);
        if crate::browser::SUPPORTED {
            setting(
                ui,
                palette,
                "Open links",
                "Web links in replies, the tutorial, and Settings can open in a panel beside the conversation. Pages there don’t share your browser’s saved logins or extensions; the panel’s arrow button opens the page in your browser.",
            );
            if let Some(choice) = segmented(
                ui,
                palette,
                self.settings.links_in_app,
                &[(false, "In Your Browser"), (true, "In Super OpenCode")],
            ) {
                self.settings.links_in_app = choice;
                self.save_settings();
                if !choice {
                    self.browser = None;
                }
            }
        } else {
            setting(
                ui,
                palette,
                "Open links",
                "Links open in your default browser. The in-app browser comes with the macOS and Windows builds.",
            );
        }
    }

    fn opencode_settings(&mut self, ui: &mut Ui, palette: &Palette) {
        heading(ui, palette, "OpenCode");
        let idle = !self.is_running();
        let (path, origin) = match (&self.settings.executable, &self.detected) {
            (Some(chosen), _) => (Some(chosen), "Chosen by you."),
            (None, Some(found)) => (Some(found), "Found automatically."),
            (None, None) => (None, ""),
        };
        match path {
            Some(path) => {
                ui.add(label(tilde(path), theme::mono(theme::CODE), palette.ink).wrap());
                ui.add_space(4.0);
                ui.add(label(origin, theme::text(theme::SMALL), palette.ink_faint));
            }
            None => {
                let (text, color) = match (&self.install_failed, self.is_installing()) {
                    (_, true) => ("Installing OpenCode…".to_owned(), palette.ink_muted),
                    (Some(problem), false) => (problem.clone(), palette.danger),
                    (None, false) => ("OpenCode isn’t installed.".to_owned(), palette.warn),
                };
                ui.add(label(text, theme::text(theme::READING), color).wrap());
                if !self.is_installing() {
                    ui.add_space(8.0);
                    if ui.button("Install OpenCode").clicked() {
                        self.install_opencode();
                    }
                }
            }
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_enabled_ui(idle, |ui| {
                if ui.button("Choose Executable…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_title("Choose opencode or opencode.exe")
                        .pick_file()
                {
                    self.settings.executable = Some(path);
                    self.save_settings();
                }
                if self.settings.executable.is_some()
                    && ui.button("Use Automatic Detection").clicked()
                {
                    self.settings.executable = None;
                    self.save_settings();
                    self.detected = crate::runner::find_opencode();
                }
            });
            ui.hyperlink_to("Install OpenCode", "https://opencode.ai/docs/");
        });
    }

    fn storage_settings(&mut self, ui: &mut Ui, palette: &Palette) {
        heading(ui, palette, "Storage");
        let Some(location) = self
            .store
            .as_ref()
            .map(|store| store.location().to_path_buf())
        else {
            problem(
                ui,
                palette,
                "Local storage is unavailable, so sessions and usage are not saved.",
            );
            return;
        };
        ui.add(label(tilde(&location), theme::mono(theme::CODE), palette.ink).wrap());

        ui.add(
            label(
                "Sessions, settings, the usage record, the speech model, and text made from attachments stay in this folder on this computer.",
                theme::text(theme::SMALL),
                palette.ink_faint,
            )
            .wrap(),
        );
        ui.add_space(12.0);
        let confirming = egui::Id::new("confirm-clear-usage");
        let asking = ui.data(|data| data.get_temp::<bool>(confirming).unwrap_or(false));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let reveal = if cfg!(target_os = "macos") {
                "Show in Finder"
            } else if cfg!(windows) {
                "Show in File Explorer"
            } else {
                "Open Folder"
            };
            if ui.button(reveal).clicked()
                && let Err(message) = open_path(&location)
            {
                self.storage_error = Some(message);
            }
            if !asking {
                if ui.button("Clear Usage History…").clicked() {
                    ui.data_mut(|data| data.insert_temp(confirming, true));
                }
            } else {
                ui.add(
                    label(
                        "Delete every recorded step and token count? This can’t be undone.",
                        theme::text(theme::SMALL),
                        palette.danger,
                    )
                    .selectable(false),
                );
                let delete = egui::Button::new(
                    RichText::new("Delete Usage History")
                        .font(theme::strong(theme::SMALL))
                        .color(palette.on_accent),
                )
                .fill(palette.danger)
                .stroke(Stroke::NONE);
                if ui.add(delete).clicked() {
                    if let Some(store) = &self.store
                        && let Err(error) = store.clear_usage()
                    {
                        self.storage_error = Some(format!("Couldn’t clear usage: {error}"));
                    }
                    self.refresh_usage();
                    ui.data_mut(|data| data.insert_temp(confirming, false));
                }
                if ui.button("Cancel").clicked() {
                    ui.data_mut(|data| data.insert_temp(confirming, false));
                }
            }
        });
        if let Some(error) = &self.storage_error {
            ui.add_space(12.0);
            problem(ui, palette, error);
        }
    }
}

fn heading(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.add(label(text, theme::strong(theme::READING + 2.0), palette.ink).selectable(false));
    ui.add_space(14.0);
}

/// A setting's name above its control. What it does shows when the name is pointed at.
fn setting(ui: &mut Ui, palette: &Palette, title: &str, note: &str) {
    let name = ui.add(
        label(title, theme::strong(theme::TEXT), palette.ink)
            .selectable(false)
            .sense(Sense::hover()),
    );
    if !note.is_empty() {
        name.on_hover_text(note);
    }
    ui.add_space(8.0);
}

/// A row of mutually exclusive choices. Returns the choice when it changes.
pub(crate) fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    palette: &Palette,
    current: T,
    options: &[(T, &str)],
) -> Option<T> {
    let mut chosen = None;
    Frame::new()
        .fill(palette.raised)
        .stroke(Stroke::new(1.0, palette.line))
        .corner_radius(radius(theme::RADIUS_CONTROL + 3))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (value, name) in options {
                    if ui.selectable_label(current == *value, *name).clicked() && current != *value
                    {
                        chosen = Some(*value);
                    }
                }
            });
        });
    chosen
}

/// Opens `path` in its default app: files in their viewer, folders in the file manager.
pub(crate) fn open_path(path: &Path) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Couldn’t open {}: {error}", path.display()))
}

/// A prompt as the user wrote it, led by the chevron from the app's mark.
fn prompt(ui: &mut Ui, palette: &Palette, text: &str) {
    Frame::new()
        .fill(palette.raised)
        .stroke(Stroke::new(1.0, palette.line))
        .corner_radius(radius(theme::RADIUS_SURFACE))
        .inner_margin(Margin {
            left: 12,
            right: 14,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = 9.0;
                let (rect, _) =
                    ui.allocate_exact_size(vec2(13.0, theme::READING * 1.5), Sense::hover());
                icons::paint(
                    ui.painter(),
                    Rect::from_center_size(rect.center(), vec2(13.0, 13.0)),
                    Icon::Prompt,
                    palette.accent,
                );
                markdown::plain(ui, palette, text);
            });
        });
}

/// What the running task is doing and for how long, after a small moving sign for that kind
/// of step:
/// “Running cargo test  12s”, with the whole task's time on the right.
fn activity_line(
    ui: &mut Ui,
    palette: &Palette,
    doing: &str,
    since: i64,
    started: i64,
    still: bool,
) {
    // With reduced motion the sign keeps still and the times tick once a second.
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(if still {
            1000
        } else {
            33
        }));
    let now = crate::runner::now_millis();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let painter = ui.painter();
    let time = ui.input(|input| input.time);
    crate::working::paint(
        painter,
        Rect::from_center_size(
            egui::pos2(rect.left() + 11.0, rect.center().y),
            vec2(18.0, 18.0),
        ),
        crate::working::Sign::of(doing),
        time,
        palette.accent,
        palette.accent.gamma_multiply(0.42),
        still,
    );
    let total = painter.layout_no_wrap(
        format!("{} total", crate::activity::elapsed((now - started) / 1000)),
        theme::text(theme::SMALL),
        palette.ink_faint,
    );
    let total_left = rect.right() - total.size().x;
    let show_total = since > started + 1500;
    if show_total {
        painter.galley(
            egui::pos2(total_left, rect.center().y - total.size().y / 2.0),
            total,
            palette.ink_faint,
        );
    }
    let left = rect.left() + 34.0;
    // The step's time stays visible however long the description is.
    let step = painter.layout_no_wrap(
        crate::activity::elapsed((now - since) / 1000),
        theme::text(theme::SMALL),
        palette.ink_faint,
    );
    let right = if show_total {
        total_left - 16.0
    } else {
        rect.right()
    };
    let mut job = egui::text::LayoutJob::single_section(
        doing.to_owned(),
        egui::TextFormat::simple(theme::text(theme::TEXT), palette.ink_muted),
    );
    job.wrap.max_width = (right - left - step.size().x - 10.0).max(40.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = painter.layout_job(job);
    let words_end = left + galley.size().x;
    painter.galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.ink_muted,
    );
    painter.galley(
        egui::pos2(
            words_end + 10.0,
            rect.center().y - step.size().y / 2.0 + 0.5,
        ),
        step,
        palette.ink_faint,
    );
}

/// An error: what went wrong and, where the message has one, what to do next.
pub(crate) fn problem(ui: &mut Ui, palette: &Palette, text: &str) {
    Frame::new()
        .fill(palette.raised)
        .stroke(Stroke::new(1.0, palette.danger.gamma_multiply(0.55)))
        .corner_radius(radius(theme::RADIUS_SURFACE))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let mut lines = text.lines();
            let headline = lines.next().unwrap_or_default();
            ui.add(label(headline, theme::text(theme::READING), palette.danger).wrap());
            let detail = lines.collect::<Vec<_>>().join("\n");
            if !detail.is_empty() {
                ui.add_space(6.0);
                ui.add(label(detail, theme::mono(theme::CODE), palette.ink_muted).wrap());
            }
        });
}

pub(crate) fn usage_table(ui: &mut Ui, palette: &Palette, rows: &[Usage]) {
    const COLUMNS: [&str; 7] = [
        "Model",
        "Steps",
        "Input",
        "Output",
        "Reasoning",
        "Cache read",
        "Cache write",
    ];
    let numbers = |row: &Usage| {
        [
            row.steps,
            row.input,
            row.output,
            row.reasoning,
            row.cache_read,
            row.cache_write,
        ]
    };
    let mut totals = [0i64; 6];
    for row in rows {
        for (total, value) in totals.iter_mut().zip(numbers(row)) {
            *total += value;
        }
    }

    Frame::new()
        .fill(palette.raised)
        .stroke(Stroke::new(1.0, palette.line))
        .corner_radius(radius(theme::RADIUS_SURFACE))
        .inner_margin(Margin::symmetric(4, 4))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ScrollArea::horizontal().id_salt("usage").show(ui, |ui| {
                let width = ui.available_width().max(660.0);
                let number_width = 86.0;
                let name_width = width - 6.0 * number_width - 24.0;
                let line = |ui: &mut Ui,
                            name: &str,
                            note: &str,
                            values: Option<[i64; 6]>,
                            kind: u8| {
                    let (rect, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
                    let painter = ui.painter();
                    let (font, ink) = match kind {
                        0 => (theme::strong(theme::SMALL), palette.ink_muted),
                        2 => (theme::strong(theme::TEXT), palette.ink),
                        _ => (theme::text(theme::TEXT), palette.ink),
                    };
                    if kind != 0 {
                        painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, palette.line));
                    }
                    let galley = one_line(ui, name, font.clone(), ink, name_width - 12.0);
                    let after = rect.left() + 12.0 + galley.size().x + 8.0;
                    ui.painter().galley(
                        egui::pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
                        galley,
                        ink,
                    );
                    let room = rect.left() + name_width - after;
                    if !note.is_empty() && room > 40.0 {
                        let galley =
                            one_line(ui, note, theme::text(theme::SMALL), palette.ink_faint, room);
                        ui.painter().galley(
                            egui::pos2(after, rect.center().y - galley.size().y / 2.0),
                            galley,
                            palette.ink_faint,
                        );
                    }
                    for index in 0..6 {
                        let right =
                            rect.left() + 12.0 + name_width + (index as f32 + 1.0) * number_width;
                        let (text, font) = match values {
                            Some(values) => (grouped(values[index]), theme::mono(theme::CODE)),
                            None => (COLUMNS[index + 1].to_owned(), font.clone()),
                        };
                        ui.painter().text(
                            egui::pos2(right, rect.center().y),
                            Align2::RIGHT_CENTER,
                            text,
                            font,
                            ink,
                        );
                    }
                };
                ui.spacing_mut().item_spacing.y = 0.0;
                line(ui, COLUMNS[0], "", None, 0);
                for row in rows {
                    // Models this app offers are shown by name; any other is shown by its ID.
                    let (name, provider) = models::find(&row.model).map_or_else(
                        || (row.model.clone(), String::new()),
                        |model| (model.name.clone(), model.provider_name()),
                    );
                    line(ui, &name, &provider, Some(numbers(row)), 1);
                }
                if rows.len() > 1 {
                    line(ui, "Total", "", Some(totals), 2);
                }
            });
        });
}

#[derive(Clone, Copy)]
enum Action {
    Read,
    StopReading,
    Copy,
}

/// A small text button with an icon, for actions on a response.
fn quiet_button(ui: &mut Ui, palette: &Palette, icon: Icon, text: &str) -> Response {
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        theme::text(theme::SMALL),
        palette.ink_muted,
    );
    let size = vec2(galley.size().x + 32.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, text));
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, radius(theme::RADIUS_CONTROL), palette.hover);
    }
    if response.has_focus() {
        icons::focus_ring(painter, rect, theme::RADIUS_CONTROL, palette.accent);
    }
    icons::paint(
        painter,
        Rect::from_center_size(
            egui::pos2(rect.left() + 15.0, rect.center().y),
            vec2(14.0, 14.0),
        ),
        icon,
        palette.ink_muted,
    );
    painter.galley(
        egui::pos2(rect.left() + 27.0, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.ink_muted,
    );
    response
}

/// Read aloud and Copy, under a finished response.
fn actions(ui: &mut Ui, palette: &Palette, turn: uuid::Uuid, reading: bool) -> Option<Action> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(-8.0);
        let (icon, text, action) = if reading {
            (Icon::Stop, "Stop reading", Action::StopReading)
        } else {
            (Icon::Speaker, "Read aloud", Action::Read)
        };
        if quiet_button(ui, palette, icon, text).clicked() {
            chosen = Some(action);
        }
        let copied_at = egui::Id::new(("copied", turn));
        let now = ui.input(|input| input.time);
        let recent = ui
            .data(|data| data.get_temp::<f64>(copied_at))
            .is_some_and(|at| now - at < 1.5);
        if recent {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
        let text = if recent { "Copied" } else { "Copy" };
        if quiet_button(ui, palette, Icon::Copy, text).clicked() {
            ui.data_mut(|data| data.insert_temp(copied_at, now));
            chosen = Some(Action::Copy);
        }
    });
    chosen
}

impl App {
    fn voice_settings(&mut self, ui: &mut Ui, palette: &Palette) {
        let model = crate::speech::MODEL;
        let idle = matches!(self.inputs.dictation, crate::inputs::Dictation::Idle);
        let mut action = None;
        match &self.inputs.model {
            ModelState::Ready(path) => {
                ui.add(
                    label(
                        format!(
                            "Speech model installed: Whisper base, {}.",
                            crate::speech::megabytes(model.bytes)
                        ),
                        theme::text(theme::READING),
                        palette.ink,
                    )
                    .wrap(),
                );
                ui.add_space(4.0);
                ui.add(label(tilde(path), theme::mono(theme::CODE), palette.ink_faint).wrap());
                ui.add_space(10.0);
                if ui
                    .add_enabled(idle, egui::Button::new("Remove Speech Model"))
                    .clicked()
                {
                    action = Some(false);
                }
            }
            ModelState::Downloading { received, .. } => {
                let got = received.load(std::sync::atomic::Ordering::Relaxed);
                ui.add(label(
                    format!(
                        "Downloading the speech model: {} of {}",
                        crate::speech::megabytes(got),
                        crate::speech::megabytes(model.bytes)
                    ),
                    theme::text(theme::READING),
                    palette.ink,
                ));
            }
            state => {
                if let ModelState::Failed(message) = state {
                    problem(ui, palette, message);
                    ui.add_space(10.0);
                }
                ui.add(
                    label(
                        format!(
                            "Speech is turned into text on this computer. The speech model downloads once ({}).",
                            crate::speech::megabytes(model.bytes)
                        ),
                        theme::text(theme::READING),
                        palette.ink_muted,
                    )
                    .wrap(),
                );
                ui.add_space(10.0);
                if ui.button("Download Speech Model").clicked() {
                    action = Some(true);
                }
            }
        }
        match action {
            Some(true) => self.start_download(ui.ctx()),
            Some(false) => self.remove_model(),
            None => {}
        }

        ui.add_space(18.0);
        setting(
            ui,
            palette,
            "Dictation language",
            "Detecting the language works for most speech. Choosing it helps with short phrases and accents.",
        );
        let current = self.settings.dictation_language.clone();
        let shown = current
            .as_deref()
            .and_then(|code| crate::speech::LANGUAGES.iter().find(|(id, _)| *id == code))
            .map_or("Detect automatically", |(_, name)| *name);
        let mut picked = current.clone();
        let chevron = palette.ink_muted;
        egui::ComboBox::from_id_salt("dictation-language")
            .selected_text(shown)
            .width(200.0)
            .icon(move |ui, rect, _, _| {
                let at = Rect::from_center_size(rect.center(), vec2(12.0, 12.0));
                icons::paint(ui.painter(), at, Icon::ChevronDown, chevron);
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut picked, None, "Detect automatically");
                for (code, name) in crate::speech::LANGUAGES {
                    ui.selectable_value(&mut picked, Some((*code).to_owned()), *name);
                }
            });
        if picked != current {
            self.settings.dictation_language = picked;
            self.save_settings();
        }

        ui.add_space(18.0);
        let available = crate::speak::available();
        let mut read = self.settings.read_aloud && available;
        if checkbox(
            ui,
            palette,
            &mut read,
            "Read each response aloud when it finishes",
            available,
        )
        .changed()
        {
            self.settings.read_aloud = read;
            self.save_settings();
        }
        if available {
            ui.add_space(10.0);
            setting(ui, palette, "Reading speed", "");
            if let Some(pace) = segmented(
                ui,
                palette,
                self.settings.reading_pace,
                &[
                    (Pace::Slower, "Slower"),
                    (Pace::Normal, "Normal"),
                    (Pace::Faster, "Faster"),
                ],
            ) {
                self.settings.reading_pace = pace;
                self.save_settings();
            }
        }
        if !available {
            ui.add_space(4.0);
            ui.add(
                label(
                    "Needs espeak-ng.",
                    theme::text(theme::SMALL),
                    palette.ink_faint,
                )
                .wrap(),
            );
        }
    }
}

/// Debug builds: every working sign at eight moments, large, for checking their motion.
#[cfg(debug_assertions)]
fn signs_sheet(ctx: &egui::Context, palette: &Palette) {
    use crate::working::{Sign, paint};
    egui::Area::new(egui::Id::new("signs-sheet"))
        .fixed_pos(egui::pos2(270.0, 60.0))
        .show(ctx, |ui| {
            Frame::new()
                .fill(palette.raised)
                .stroke(Stroke::new(1.0, palette.line))
                .corner_radius(radius(12))
                .inner_margin(Margin::same(16))
                .show(ui, |ui| {
                    for (sign, name) in [
                        (Sign::Think, "Thinking"),
                        (Sign::Read, "Reading"),
                        (Sign::Run, "Running"),
                        (Sign::Edit, "Editing"),
                        (Sign::Search, "Searching"),
                        (Sign::Web, "Web"),
                        (Sign::Wait, "Waiting"),
                    ] {
                        ui.horizontal(|ui| {
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(90.0, 52.0), Sense::hover());
                            ui.painter().text(
                                rect.left_center(),
                                Align2::LEFT_CENTER,
                                name,
                                theme::text(theme::TEXT),
                                palette.ink,
                            );
                            for step in 0..8 {
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(52.0, 52.0), Sense::hover());
                                paint(
                                    ui.painter(),
                                    rect.shrink(4.0),
                                    sign,
                                    f64::from(step) * 0.22,
                                    palette.accent,
                                    palette.accent.gamma_multiply(0.42),
                                    false,
                                );
                            }
                            let (rect, _) =
                                ui.allocate_exact_size(vec2(24.0, 52.0), Sense::hover());
                            paint(
                                ui.painter(),
                                Rect::from_center_size(rect.center(), vec2(18.0, 18.0)),
                                sign,
                                0.3,
                                palette.accent,
                                palette.accent.gamma_multiply(0.42),
                                false,
                            );
                        });
                    }
                });
        });
}
