//! The tutorial: setting up OpenCode, choosing a project and how models are picked, adding
//! free models with a free key, and using the composer. It opens on first launch and from
//! Settings › Help at any time.

use eframe::egui::{
    self, Align, Color32, Frame, Layout, Margin, Rect, RichText, Sense, Shadow, Stroke, Ui, vec2,
};

use crate::{
    app::App,
    icons::{self, Icon},
    theme::{self, Palette},
    views::{label, radius, segmented, tilde},
};

pub const PAGES: usize = 6;

/// A row on a page: an icon, a title, and what to know.
fn point(ui: &mut Ui, palette: &Palette, icon: Icon, title: &str, detail: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 20.0), Sense::hover());
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
            icon,
            palette.ink_muted,
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add(label(title, theme::strong(theme::TEXT), palette.ink).selectable(false));
            ui.add(label(detail, theme::text(theme::SMALL), palette.ink_muted).wrap());
        });
    });
    ui.add_space(12.0);
}

fn paragraph(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.add(label(text, theme::text(theme::READING), palette.ink_muted).wrap());
    ui.add_space(14.0);
}

/// A state line with a coloured dot: found, missing, chosen.
fn state(ui: &mut Ui, palette: &Palette, good: bool, text: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (dot, _) = ui.allocate_exact_size(vec2(8.0, 18.0), Sense::hover());
        let color = if good { palette.ok } else { palette.warn };
        ui.painter().circle_filled(dot.center(), 3.5, color);
        ui.add(label(text, theme::strong(theme::TEXT), palette.ink).truncate());
    });
    ui.add_space(12.0);
}

fn primary(palette: &Palette, text: &str) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text)
            .font(theme::strong(theme::TEXT))
            .color(palette.on_accent),
    )
    .fill(palette.accent)
    .stroke(Stroke::NONE)
    .min_size(vec2(0.0, 32.0))
}

impl App {
    /// Opens the tutorial at its first page.
    pub(crate) fn show_tutorial(&mut self) {
        self.tutorial = Some(0);
    }

    fn close_tutorial(&mut self) {
        self.tutorial = None;
        if !self.settings.tutorial_seen {
            self.settings.tutorial_seen = true;
            self.save_settings();
        }
    }

    pub(crate) fn tutorial_dialog(&mut self, ctx: &egui::Context, palette: &Palette) {
        let Some(page) = self.tutorial else { return };
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
            .inner_margin(Margin::same(26));
        let mut go = None;
        let mut close = false;
        let shown = egui::Modal::new(egui::Id::new("tutorial"))
            .frame(frame)
            .backdrop_color(Color32::from_black_alpha(90))
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add(
                    label(
                        format!("{} of {PAGES}", page + 1),
                        theme::text(theme::SMALL),
                        palette.ink_faint,
                    )
                    .selectable(false),
                );
                ui.add_space(6.0);
                let title = match page {
                    0 => "Welcome to Super OpenCode",
                    1 => "OpenCode",
                    2 => "Choose a project",
                    3 => "Choosing models",
                    4 => "More free models",
                    _ => "The basics",
                };
                ui.add(
                    label(title, theme::strong(theme::TITLE - 3.0), palette.ink).selectable(false),
                );
                ui.add_space(14.0);
                // Pages keep the same height so the buttons don't jump.
                let body = ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 250.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_min_height(250.0);
                        self.tutorial_page(ui, palette, page, &mut close);
                    },
                );
                drop(body);
                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if page + 1 < PAGES
                        && ui
                            .add(egui::Button::new("Skip Tutorial").frame(false))
                            .clicked()
                    {
                        close = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let last = page + 1 == PAGES;
                        if ui
                            .add(primary(palette, if last { "Done" } else { "Next" }))
                            .clicked()
                        {
                            if last {
                                close = true;
                            } else {
                                go = Some(page + 1);
                            }
                        }
                        if page > 0
                            && ui
                                .add(egui::Button::new("Back").min_size(vec2(0.0, 32.0)))
                                .clicked()
                        {
                            go = Some(page - 1);
                        }
                    });
                });
            });
        if shown.should_close() {
            close = true;
        }
        if close {
            self.close_tutorial();
        } else if let Some(page) = go {
            self.tutorial = Some(page);
        }
    }

    fn tutorial_page(&mut self, ui: &mut Ui, palette: &Palette, page: usize, close: &mut bool) {
        match page {
            0 => {
                paragraph(
                    ui,
                    palette,
                    "A coding agent that works in your project, on free models.",
                );
                point(
                    ui,
                    palette,
                    Icon::Check,
                    "Free models only",
                    "Nothing is ever charged.",
                );
                point(
                    ui,
                    palette,
                    Icon::Folder,
                    "Runs on this computer",
                    "Your files stay here.",
                );
                point(ui, palette, Icon::Book, "Back any time", "Settings › Help.");
            }
            1 => {
                match (
                    self.opencode().cloned(),
                    self.is_installing(),
                    self.install_failed.clone(),
                ) {
                    (Some(_), _, _) => state(ui, palette, true, "Installed"),
                    (None, true, _) => {
                        state(ui, palette, false, "Installing in the background…");
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(250));
                    }
                    (None, false, Some(problem)) => {
                        state(ui, palette, false, "The install didn’t finish");
                        ui.add(label(problem, theme::text(theme::SMALL), palette.ink_faint).wrap());
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if ui.button("Try Again").clicked() {
                                self.install_opencode();
                            }
                            ui.hyperlink_to("Install guide", "https://opencode.ai/docs/");
                        });
                    }
                    (None, false, None) => {
                        state(ui, palette, false, "Not installed");
                        if ui.button("Install OpenCode").clicked() {
                            self.install_opencode();
                        }
                    }
                }
            }
            2 => {
                match self.settings.project.clone() {
                    Some(path) => state(ui, palette, true, &tilde(&path)),
                    None => state(ui, palette, false, "No folder yet"),
                }
                paragraph(
                    ui,
                    palette,
                    "OpenCode reads, edits, and runs commands in this folder.",
                );
                if ui.button("Choose Project Folder…").clicked() {
                    self.pick_project();
                }
            }
            3 => {
                let mode = self.settings.model_mode;
                if let Some(chosen) = segmented(
                    ui,
                    palette,
                    mode,
                    &[
                        (crate::auto::Mode::Manual, "Manual"),
                        (crate::auto::Mode::Auto, "Auto"),
                    ],
                ) {
                    self.settings.model_mode = chosen;
                    self.save_settings();
                }
                ui.add_space(14.0);
                point(ui, palette, Icon::Sliders, "Manual", "You pick the model.");
                point(
                    ui,
                    palette,
                    Icon::Fork,
                    "Auto",
                    "A model is picked for each task.",
                );
            }
            4 => {
                paragraph(ui, palette, "Optional: a free key adds more models.");
                let mut connect = None;
                for provider in crate::providers::CONNECTABLE {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let connected = self.is_connected(provider.id).is_some();
                        ui.add(
                            label(provider.name, theme::strong(theme::TEXT), palette.ink)
                                .selectable(false),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if connected {
                                ui.add(
                                    label("Connected", theme::text(theme::SMALL), palette.ok)
                                        .selectable(false),
                                );
                            } else if ui.small_button("Connect…").clicked() {
                                connect = Some(provider.id);
                            }
                            ui.hyperlink_to("Get a key", provider.key_page);
                        });
                    });
                    ui.add_space(6.0);
                }
                if let Some(provider) = connect {
                    *close = true;
                    self.begin_connect(provider);
                }
            }
            _ => {
                let dictate = if cfg!(target_os = "macos") {
                    "⌘D"
                } else {
                    "Ctrl+D"
                };
                point(
                    ui,
                    palette,
                    Icon::Plus,
                    "+",
                    "Attach files, or plan without editing.",
                );
                point(
                    ui,
                    palette,
                    Icon::Shield,
                    "Access",
                    "What OpenCode may do on its own.",
                );
                point(ui, palette, Icon::Mic, dictate, "Speak your prompt.");
                point(
                    ui,
                    palette,
                    Icon::List,
                    "Top right",
                    "Tasks, terminal, changes, browser, files.",
                );
                point(
                    ui,
                    palette,
                    Icon::Bars,
                    "Usage",
                    "Your activity and streaks.",
                );
            }
        }
    }
}
