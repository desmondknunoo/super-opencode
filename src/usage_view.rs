//! The Usage page: a year of activity drawn like GitHub's contribution graph, streaks, daily
//! tokens by model, and the sessions that used the most. The start screen shows a smaller
//! version of the activity grid.

use chrono::{Datelike, Days, NaiveDate};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Frame, Layout, Margin, Rect, ScrollArea, Sense,
    Stroke, Ui, pos2, vec2,
};

use crate::{
    app::{App, View},
    history::{self, Daily, History, Source},
    models,
    theme::{self, Palette},
    views::{column, compact, grouped, label, one_line, problem, radius, segmented, usage_table},
};

/// One blue, light to dark on the light theme and dark to light on the dark theme, checked
/// for steps that read apart and a faintest step that still shows against the card.
const LIGHT_SHADES: [Color32; 4] = [
    Color32::from_rgb(0x86, 0xB6, 0xEF),
    Color32::from_rgb(0x55, 0x98, 0xE7),
    Color32::from_rgb(0x2A, 0x78, 0xD6),
    Color32::from_rgb(0x18, 0x4F, 0x95),
];
const DARK_SHADES: [Color32; 4] = [
    Color32::from_rgb(0x18, 0x4F, 0x95),
    Color32::from_rgb(0x25, 0x6A, 0xBF),
    Color32::from_rgb(0x39, 0x87, 0xE5),
    Color32::from_rgb(0x86, 0xB6, 0xEF),
];
/// Model colours in a fixed order, checked for colour-blind separation on both themes.
const LIGHT_SERIES: [Color32; 5] = [
    Color32::from_rgb(0x2A, 0x78, 0xD6),
    Color32::from_rgb(0xEB, 0x68, 0x34),
    Color32::from_rgb(0x1B, 0xAF, 0x7A),
    Color32::from_rgb(0xED, 0xA1, 0x00),
    Color32::from_rgb(0xE8, 0x7B, 0xA4),
];
const DARK_SERIES: [Color32; 5] = [
    Color32::from_rgb(0x39, 0x87, 0xE5),
    Color32::from_rgb(0xD9, 0x59, 0x26),
    Color32::from_rgb(0x19, 0x9E, 0x70),
    Color32::from_rgb(0xC9, 0x85, 0x00),
    Color32::from_rgb(0xD5, 0x51, 0x81),
];

fn dark(ui: &Ui) -> bool {
    ui.visuals().dark_mode
}

/// The colour for an activity shade: 0 is a day with nothing, 1 to 4 are busier days.
fn shade_color(ui: &Ui, palette: &Palette, shade: usize) -> Color32 {
    match shade {
        0 if dark(ui) => palette.line,
        0 => palette.hover,
        _ if dark(ui) => DARK_SHADES[shade - 1],
        _ => LIGHT_SHADES[shade - 1],
    }
}

/// The colour for a chart series: a leading model's slot, or grey for every other model.
fn series_color(ui: &Ui, palette: &Palette, slot: Option<usize>) -> Color32 {
    match slot {
        Some(slot) if dark(ui) => DARK_SERIES[slot % DARK_SERIES.len()],
        Some(slot) => LIGHT_SERIES[slot % LIGHT_SERIES.len()],
        None => palette.ink_faint.gamma_multiply(0.55),
    }
}

/// A model's name for people: the catalog's, else its ID without the provider and the
/// free tag.
pub(crate) fn model_name(id: &str) -> String {
    let named = models::name_of(id);
    if named != id {
        return named;
    }
    let bare = id.split_once('/').map_or(id, |(_, rest)| rest);
    bare.trim_end_matches(":free")
        .trim_end_matches("-free")
        .to_owned()
}

fn plural(count: u32, one: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{} {one}s", grouped(i64::from(count)))
    }
}

fn card<R>(ui: &mut Ui, palette: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(palette.raised)
        .stroke(Stroke::new(1.0, palette.line))
        .corner_radius(radius(theme::RADIUS_SURFACE))
        .inner_margin(Margin::symmetric(18, 16))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

fn title_row(ui: &mut Ui, palette: &Palette, title: &str, right: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add(label(title, theme::strong(theme::READING + 1.0), palette.ink).selectable(false));
        ui.with_layout(Layout::right_to_left(Align::Center), right);
    });
}

/// The activity grid's period: twelve months up to today, or one calendar year.
fn period(year: Option<i32>, today: NaiveDate) -> (NaiveDate, NaiveDate) {
    match year {
        Some(year) => (
            NaiveDate::from_ymd_opt(year, 1, 1).unwrap_or(today),
            NaiveDate::from_ymd_opt(year, 12, 31).unwrap_or(today),
        ),
        None => (today - Days::new(364), today),
    }
}

/// Draws the activity grid for `first` to `last` with cells of `cell` points. Days after
/// `today` are left empty. Returns the day under the pointer.
fn activity_grid(
    ui: &mut Ui,
    palette: &Palette,
    history: &History,
    (first, last): (NaiveDate, NaiveDate),
    today: NaiveDate,
    cell: f32,
    labels: bool,
) -> Option<NaiveDate> {
    const GAP: f32 = 3.0;
    let days = history.days();
    let weeks = history::weeks(first, last);
    let limits = history::shade_limits(
        days.range(first..=last.min(today))
            .map(|(_, day)| day.tokens),
    );
    let left = if labels { 30.0 } else { 0.0 };
    let top = if labels { 18.0 } else { 0.0 };
    let step = cell + GAP;
    let size = vec2(
        left + weeks.len() as f32 * step - GAP,
        top + 7.0 * step - GAP,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect.expand(2.0));
    let small = theme::text(theme::SMALL - 1.0);
    if labels {
        for (row, name) in [(1, "Mon"), (3, "Wed"), (5, "Fri")] {
            painter.text(
                pos2(
                    rect.left(),
                    rect.top() + top + row as f32 * step + cell / 2.0,
                ),
                Align2::LEFT_CENTER,
                name,
                small.clone(),
                palette.ink_faint,
            );
        }
        // A month is named over the first week that holds its first day, when there is room.
        let mut last_label = f32::NEG_INFINITY;
        for (index, week) in weeks.iter().enumerate() {
            let Some(first_of_month) = week
                .iter()
                .flatten()
                .find(|day| day.day() == 1 || (index == 0 && *day == &first))
            else {
                continue;
            };
            let x = rect.left() + left + index as f32 * step;
            if x - last_label < 30.0 || (index + 2 >= weeks.len() && index > 0) {
                continue;
            }
            painter.text(
                pos2(x, rect.top()),
                Align2::LEFT_TOP,
                first_of_month.format("%b").to_string(),
                small.clone(),
                palette.ink_faint,
            );
            last_label = x;
        }
    }
    let pointer = response.hover_pos();
    let mut hovered = None;
    for (index, week) in weeks.iter().enumerate() {
        for (row, day) in week.iter().enumerate() {
            let Some(day) = day else { continue };
            if *day > today {
                continue;
            }
            let at = pos2(
                rect.left() + left + index as f32 * step,
                rect.top() + top + row as f32 * step,
            );
            let square = Rect::from_min_size(at, vec2(cell, cell));
            let tokens = days.get(day).map_or(0, |day| day.tokens);
            let fill = shade_color(ui, palette, history::shade(tokens, limits));
            painter.rect_filled(square, CornerRadius::same(2), fill);
            if pointer.is_some_and(|pointer| square.expand(GAP / 2.0).contains(pointer)) {
                hovered = Some(*day);
                painter.rect_stroke(
                    square.expand(1.0),
                    CornerRadius::same(3),
                    Stroke::new(1.5, palette.ink_muted),
                    egui::StrokeKind::Middle,
                );
            }
        }
    }
    if let Some(day) = hovered {
        let found = days.get(&day).copied().unwrap_or_default();
        response.on_hover_ui_at_pointer(|ui| {
            ui.add(
                label(
                    day.format("%A %-d %B %Y").to_string(),
                    theme::strong(theme::SMALL),
                    palette.ink,
                )
                .selectable(false),
            );
            let detail = if found.tokens == 0 {
                "Nothing recorded".to_owned()
            } else {
                format!(
                    "{} tokens · {}",
                    grouped(found.tokens as i64),
                    plural(found.steps, "step")
                )
            };
            ui.add(label(detail, theme::text(theme::SMALL), palette.ink_muted).selectable(false));
        });
    }
    hovered
}

/// “Less ■■■■■ More”, right-aligned.
fn shade_key(ui: &mut Ui, palette: &Palette) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let small = theme::text(theme::SMALL - 1.0);
        ui.add(label("More", small.clone(), palette.ink_faint).selectable(false));
        for shade in (0..5).rev() {
            let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter()
                .rect_filled(rect, CornerRadius::same(2), shade_color(ui, palette, shade));
        }
        ui.add(label("Less", small, palette.ink_faint).selectable(false));
    });
}

/// Streaks and totals under the grid, as label over value in equal columns.
fn facts(ui: &mut Ui, palette: &Palette, facts: &[(&str, String)]) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let width = rect.width() / facts.len() as f32;
    for (index, (name, value)) in facts.iter().enumerate() {
        let left = rect.left() + index as f32 * width;
        let room = width - 14.0;
        let name = one_line(ui, name, theme::text(theme::SMALL), palette.ink_faint, room);
        let value = one_line(
            ui,
            value,
            theme::strong(theme::TEXT + 1.0),
            palette.ink,
            room,
        );
        let painter = ui.painter();
        painter.galley(pos2(left, rect.top()), name, palette.ink_faint);
        painter.galley(pos2(left, rect.top() + 18.0), value, palette.ink);
    }
}

/// A token count for an axis or a cell: 200k rather than 200.0k.
fn short(count: u64) -> String {
    compact(count as i64)
        .replace(".0k", "k")
        .replace(".0M", "M")
}

/// Stacked daily bars with a hover breakdown, then a key that names every series.
fn daily_chart(ui: &mut Ui, palette: &Palette, daily: &Daily, leaders: &[String]) {
    let slot = |series: &Option<String>| {
        series
            .as_ref()
            .and_then(|model| leaders.iter().position(|leader| leader == model))
    };
    let totals: Vec<u64> = daily.values.iter().map(|day| day.iter().sum()).collect();
    let peak = totals.iter().copied().max().unwrap_or(0);
    let axis = 44.0;
    let height = 170.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height + 22.0), Sense::hover());
    let plot = Rect::from_min_max(
        pos2(rect.left() + axis, rect.top() + 8.0),
        pos2(rect.right(), rect.top() + height),
    );
    let painter = ui.painter_at(rect.expand(2.0));
    let small = theme::text(theme::SMALL - 1.0);
    // Recessive guides at half and full height, labelled on the left.
    let top_value = nice_ceiling(peak);
    for fraction in [0.0_f32, 0.5, 1.0] {
        let y = plot.bottom() - plot.height() * fraction;
        painter.hline(
            plot.x_range(),
            y,
            Stroke::new(
                1.0,
                if fraction == 0.0 {
                    palette.line_strong
                } else {
                    palette.line
                },
            ),
        );
        if top_value > 0 {
            painter.text(
                pos2(plot.left() - 8.0, y),
                Align2::RIGHT_CENTER,
                short((top_value as f64 * f64::from(fraction)) as u64),
                small.clone(),
                palette.ink_faint,
            );
        }
    }
    let count = daily.days.len().max(1);
    let slot_width = plot.width() / count as f32;
    let bar = (slot_width * 0.64).clamp(2.0, 28.0);
    let pointer = response.hover_pos();
    let mut hovered = None;
    for (index, values) in daily.values.iter().enumerate() {
        let center = plot.left() + slot_width * (index as f32 + 0.5);
        let column = Rect::from_center_size(
            pos2(center, plot.center().y),
            vec2(slot_width, plot.height()),
        );
        if pointer.is_some_and(|pointer| column.contains(pointer)) {
            hovered = Some(index);
            painter.rect_filled(
                column,
                CornerRadius::ZERO,
                palette.hover.gamma_multiply(0.6),
            );
        }
        if top_value == 0 {
            continue;
        }
        let mut base = plot.bottom();
        let shown: Vec<usize> = (0..values.len()).filter(|&s| values[s] > 0).collect();
        for (position, &series) in shown.iter().enumerate() {
            let tall = plot.height() * values[series] as f32 / top_value as f32;
            let top_of_stack = position + 1 == shown.len();
            let segment = Rect::from_min_max(
                pos2(center - bar / 2.0, base - tall),
                pos2(center + bar / 2.0, base),
            );
            let corners = if top_of_stack {
                let round = (bar / 2.0).min(4.0) as u8;
                CornerRadius {
                    nw: round,
                    ne: round,
                    sw: 0,
                    se: 0,
                }
            } else {
                CornerRadius::ZERO
            };
            painter.rect_filled(
                segment,
                corners,
                series_color(ui, palette, slot(&daily.series[series])),
            );
            // A thin gap of card colour between stacked segments.
            if !top_of_stack && tall > 3.0 {
                painter.hline(
                    segment.x_range(),
                    segment.top(),
                    Stroke::new(1.5, palette.raised),
                );
            }
            base -= tall;
        }
    }
    // Dates under the first, middle, and last bars.
    let marks = [0, count / 2, count - 1];
    for (position, &index) in marks.iter().enumerate() {
        let Some(day) = daily.days.get(index) else {
            continue;
        };
        let x = plot.left() + slot_width * (index as f32 + 0.5);
        let align = match position {
            0 => Align2::LEFT_TOP,
            1 => Align2::CENTER_TOP,
            _ => Align2::RIGHT_TOP,
        };
        let x = match position {
            0 => x - bar / 2.0,
            2 => x + bar / 2.0,
            _ => x,
        };
        painter.text(
            pos2(x, plot.bottom() + 6.0),
            align,
            day.format("%-d %b").to_string(),
            small.clone(),
            palette.ink_faint,
        );
    }
    if let Some(index) = hovered {
        let day = daily.days[index];
        let values = &daily.values[index];
        response.on_hover_ui_at_pointer(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add(
                label(
                    format!(
                        "{} · {} tokens",
                        day.format("%a %-d %b"),
                        grouped(totals[index] as i64)
                    ),
                    theme::strong(theme::SMALL),
                    palette.ink,
                )
                .selectable(false),
            );
            for (series, value) in daily.series.iter().zip(values) {
                if *value == 0 {
                    continue;
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    swatch(ui, series_color(ui, palette, slot(series)));
                    let name = series
                        .as_deref()
                        .map_or_else(|| "Other models".to_owned(), model_name);
                    ui.add(
                        label(
                            format!("{name}  {}", compact(*value as i64)),
                            theme::text(theme::SMALL),
                            palette.ink_muted,
                        )
                        .selectable(false),
                    );
                });
            }
        });
    }
    // The key names every series with its share, so no colour has to be told apart alone.
    ui.add_space(10.0);
    let total = daily.total().max(1);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(18.0, 8.0);
        for (index, series) in daily.series.iter().enumerate() {
            let share = daily.series_total(index) as f64 / total as f64 * 100.0;
            let name = series
                .as_deref()
                .map_or_else(|| "Other models".to_owned(), model_name);
            let share = if share < 0.1 {
                "<0.1%".to_owned()
            } else {
                format!("{share:.1}%")
            };
            key_item(
                ui,
                palette,
                series_color(ui, palette, slot(series)),
                &name,
                &share,
            );
        }
    });
}

/// One entry in a chart key, laid out as a single item so a row of them wraps: a swatch,
/// the series name, and its share.
fn key_item(ui: &mut Ui, palette: &Palette, color: Color32, name: &str, share: &str) {
    let painter = ui.painter();
    let name = painter.layout_no_wrap(
        name.to_owned(),
        theme::text(theme::SMALL),
        palette.ink_muted,
    );
    let share = painter.layout_no_wrap(share.to_owned(), theme::strong(theme::SMALL), palette.ink);
    let height = name.size().y.max(share.size().y);
    let size = vec2(10.0 + 6.0 + name.size().x + 6.0 + share.size().x, height);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        Rect::from_center_size(pos2(rect.left() + 5.0, rect.center().y), vec2(10.0, 10.0)),
        CornerRadius::same(2),
        color,
    );
    let name_width = name.size().x;
    painter.galley(
        pos2(rect.left() + 16.0, rect.top()),
        name,
        palette.ink_muted,
    );
    painter.galley(
        pos2(rect.left() + 22.0 + name_width, rect.top()),
        share,
        palette.ink,
    );
}

fn swatch(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(2), color);
}

/// A round number at or above `value` for the chart's top guide: 1, 2, or 5 times a power
/// of ten.
fn nice_ceiling(value: u64) -> u64 {
    if value == 0 {
        return 0;
    }
    let mut power = 1u64;
    while power.saturating_mul(10) <= value {
        power *= 10;
    }
    [1, 2, 5, 10]
        .iter()
        .map(|step| step * power)
        .find(|candidate| *candidate >= value)
        .unwrap_or(value)
}

impl App {
    pub(crate) fn usage_page(&mut self, ui: &mut Ui, palette: &Palette) {
        ui.spacing_mut().item_spacing.y = 0.0;
        self.header(ui, palette, "Usage");
        let mut open = None;
        let mut area = ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(offset) = self.scroll_settings_to.take() {
            area = area.vertical_scroll_offset(offset);
        }
        area.show(ui, |ui| {
            Frame::new()
                .inner_margin(Margin::symmetric(24, 24))
                .show(ui, |ui| {
                    column(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        open = self.usage_body(ui, palette);
                    });
                });
        });
        if let Some(session) = open {
            self.select_session(session);
            self.view = View::Chat;
        }
    }

    fn usage_body(&mut self, ui: &mut Ui, palette: &Palette) -> Option<crate::storage::Session> {
        let source = self.settings.usage_source;
        if let Some(chosen) = segmented(
            ui,
            palette,
            source,
            &[
                (Source::OpenCode, "All OpenCode use"),
                (Source::App, "Super OpenCode only"),
            ],
        ) {
            self.settings.usage_source = chosen;
            self.save_settings();
            self.history = None;
            self.usage_year = None;
            self.load_history();
        }
        ui.add_space(8.0);
        ui.add_space(18.0);

        let history = match &self.history {
            None => {
                ui.add(label(
                    "Reading usage…",
                    theme::text(theme::READING),
                    palette.ink_faint,
                ));
                return None;
            }
            Some(Err(error)) => {
                problem(ui, palette, error);
                return None;
            }
            Some(Ok(history)) => std::sync::Arc::clone(history),
        };
        if history.steps.is_empty() {
            ui.add(
                label(
                    "Nothing recorded yet.",
                    theme::text(theme::READING),
                    palette.ink_faint,
                )
                .wrap(),
            );
            return None;
        }
        let today = history::today();
        let summary = history.summary(today);

        // The year of activity.
        let range = period(self.usage_year, today);
        let in_range: u64 = history
            .days()
            .range(range.0..=range.1)
            .map(|(_, day)| day.tokens)
            .sum();
        let years = history.years();
        card(ui, palette, |ui| {
            let heading = match self.usage_year {
                None => format!("{} tokens in the last year", compact(in_range as i64)),
                Some(year) => format!("{} tokens in {year}", compact(in_range as i64)),
            };
            let mut year = self.usage_year;
            title_row(ui, palette, &heading, |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if years.len() > 1 || years.first().is_some_and(|y| *y < today.year()) {
                    for choice in years.iter().rev() {
                        if ui
                            .selectable_label(year == Some(*choice), choice.to_string())
                            .clicked()
                        {
                            year = Some(*choice);
                        }
                    }
                    if ui.selectable_label(year.is_none(), "Last year").clicked() {
                        year = None;
                    }
                }
            });
            self.usage_year = year;
            ui.add_space(14.0);
            let weeks = history::weeks(range.0, range.1).len() as f32;
            let cell = ((ui.available_width() - 30.0) / weeks - 3.0).clamp(7.0, 13.0);
            ScrollArea::horizontal().id_salt("activity").show(ui, |ui| {
                activity_grid(ui, palette, &history, range, today, cell, true);
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                shade_key(ui, palette);
            });
            ui.add_space(14.0);
            let (rule, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().hline(
                rule.x_range(),
                rule.center().y,
                Stroke::new(1.0, palette.line),
            );
            ui.add_space(14.0);
            let busiest = summary.busiest.map_or_else(
                || "None yet".to_owned(),
                |(day, tokens)| {
                    let date = if day.year() == today.year() {
                        day.format("%-d %b")
                    } else {
                        day.format("%-d %b %Y")
                    };
                    format!("{date} · {}", short(tokens))
                },
            );
            let mut shown = vec![
                ("Current streak", plural(summary.current_streak, "day")),
                ("Longest streak", plural(summary.longest_streak, "day")),
                ("Active days", grouped(i64::from(summary.active_days))),
                ("Busiest day", busiest),
            ];
            if source == Source::OpenCode {
                shown.push(("Model charges", format!("${:.2}", summary.cost)));
            }
            facts(ui, palette, &shown);
        });
        ui.add_space(18.0);

        // Daily tokens by model.
        let days = self.usage_days;
        let daily = history.daily(today, days);
        let leaders = history.leaders();
        card(ui, palette, |ui| {
            let mut chosen = days;
            title_row(ui, palette, "Daily tokens", |ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (choice, name) in [(90, "90 days"), (30, "30 days"), (7, "7 days")] {
                    if ui.selectable_label(days == choice, name).clicked() {
                        chosen = choice;
                    }
                }
            });
            self.usage_days = chosen;
            ui.add_space(4.0);
            let first = daily.days.first().copied().unwrap_or(today);
            ui.add(
                label(
                    format!(
                        "{} – {} · {} tokens",
                        first.format("%-d %b"),
                        today.format("%-d %b %Y"),
                        grouped(daily.total() as i64)
                    ),
                    theme::text(theme::SMALL),
                    palette.ink_faint,
                )
                .selectable(false),
            );
            ui.add_space(12.0);
            daily_chart(ui, palette, &daily, &leaders);
        });
        ui.add_space(18.0);

        // The sessions that used the most in the same days.
        let since = today - Days::new(days.saturating_sub(1));
        let top = history.top_sessions(since, 8);
        let mut open = None;
        card(ui, palette, |ui| {
            title_row(ui, palette, "Top sessions", |_| {});
            ui.add_space(4.0);
            ui.add(
                label(
                    format!("The last {days} days, by tokens."),
                    theme::text(theme::SMALL),
                    palette.ink_faint,
                )
                .selectable(false),
            );
            ui.add_space(10.0);
            if top.is_empty() {
                let note = match source {
                    Source::App => "None yet.",
                    Source::OpenCode => "No sessions in these days.",
                };
                ui.add(label(note, theme::text(theme::TEXT), palette.ink_faint).wrap());
                return;
            }
            for (index, used) in top.iter().enumerate() {
                let saved = self
                    .sessions
                    .iter()
                    .find(|session| session.server_session.as_deref() == Some(used.id.as_str()));
                let recorded = history.sessions.get(&used.id);
                let title = saved
                    .map(|session| session.title().to_owned())
                    .unwrap_or_else(|| {
                        recorded
                            .map(|found| found.title.clone())
                            .filter(|title| !title.is_empty())
                            .unwrap_or_else(|| "Untitled session".to_owned())
                    });
                let folder = saved
                    .map(|session| session.project_path.to_string_lossy().into_owned())
                    .or_else(|| recorded.map(|found| found.directory.clone()))
                    .map(|path| {
                        std::path::Path::new(&path)
                            .file_name()
                            .map_or(path.clone(), |name| name.to_string_lossy().into_owned())
                    })
                    .unwrap_or_default();
                let clickable = saved.is_some();
                let sense = if clickable {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 44.0), sense);
                let painter = ui.painter();
                if index > 0 {
                    painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, palette.line));
                }
                if clickable && response.hovered() {
                    painter.rect_filled(rect.shrink2(vec2(0.0, 1.0)), radius(6), palette.hover);
                }
                let numbers = 190.0;
                let name_width = rect.width() - numbers - 12.0;
                let title_galley = one_line(
                    ui,
                    &title,
                    theme::text(theme::TEXT),
                    palette.ink,
                    name_width,
                );
                let detail = [folder.as_str(), &model_name(&used.model)]
                    .iter()
                    .filter(|part| !part.is_empty())
                    .copied()
                    .collect::<Vec<_>>()
                    .join(" · ");
                let detail_galley = one_line(
                    ui,
                    &detail,
                    theme::text(theme::SMALL - 1.0),
                    palette.ink_faint,
                    name_width,
                );
                let painter = ui.painter();
                painter.galley(
                    pos2(rect.left() + 6.0, rect.top() + 6.0),
                    title_galley,
                    palette.ink,
                );
                painter.galley(
                    pos2(rect.left() + 6.0, rect.top() + 25.0),
                    detail_galley,
                    palette.ink_faint,
                );
                painter.text(
                    pos2(rect.right() - 6.0, rect.top() + 14.0),
                    Align2::RIGHT_CENTER,
                    format!("{} tokens", compact(used.tokens as i64)),
                    theme::mono(theme::CODE),
                    palette.ink,
                );
                let last = history::local_day(used.last)
                    .map(|day| day.format("%-d %b").to_string())
                    .unwrap_or_default();
                painter.text(
                    pos2(rect.right() - 6.0, rect.top() + 31.0),
                    Align2::RIGHT_CENTER,
                    format!("{} · last used {last}", plural(used.steps, "step")),
                    theme::text(theme::SMALL - 1.0),
                    palette.ink_faint,
                );
                if clickable && response.on_hover_text("Open this session").clicked() {
                    open = saved.cloned();
                }
            }
        });

        // Today's tokens in detail, from Super OpenCode's own record.
        ui.add_space(26.0);
        ui.add(
            label(
                "Today in Super OpenCode",
                theme::strong(theme::READING + 1.0),
                palette.ink,
            )
            .selectable(false),
        );
        ui.add_space(10.0);
        match &self.usage {
            Err(error) => problem(ui, palette, error),
            Ok(rows) if rows.is_empty() => {
                ui.add(label(
                    "No steps recorded today.",
                    theme::text(theme::TEXT),
                    palette.ink_faint,
                ));
            }
            Ok(rows) => usage_table(ui, palette, rows),
        }
        open
    }

    /// The start screen's activity card: the current streak and the last months of activity.
    /// Clicking it opens the Usage page.
    pub(crate) fn activity_card(&mut self, ui: &mut Ui, palette: &Palette) {
        let Some(Ok(history)) = &self.history else {
            return;
        };
        if history.steps.is_empty() {
            return;
        }
        let history = std::sync::Arc::clone(history);
        let today = history::today();
        let summary = history.summary(today);
        let week: u64 = history
            .days()
            .range(today - Days::new(6)..=today)
            .map(|(_, day)| day.tokens)
            .sum();
        let width = 470.0_f32.min(ui.available_width());
        let response = ui
            .allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                card(ui, palette, |ui| {
                    ui.horizontal(|ui| {
                        let streak = match (summary.current_streak, summary.last_active) {
                            (0, Some(day)) => format!("Last active {}", day.format("%-d %B")),
                            (0, None) => "No activity yet".to_owned(),
                            (days, _) => format!("{days}-day streak"),
                        };
                        ui.add(
                            label(streak, theme::strong(theme::TEXT), palette.ink)
                                .selectable(false),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add(
                                label(
                                    format!("{} tokens this week", compact(week as i64)),
                                    theme::text(theme::SMALL),
                                    palette.ink_faint,
                                )
                                .selectable(false),
                            );
                        });
                    });
                    ui.add_space(10.0);
                    let cell = 11.0;
                    let weeks = ((ui.available_width() + 3.0) / (cell + 3.0))
                        .floor()
                        .max(4.0) as u64;
                    let first = today - Days::new(7 * weeks - 1);
                    activity_grid(ui, palette, &history, (first, today), today, cell, false);
                });
            })
            .response
            .interact(Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Open usage");
        if response.clicked() {
            self.open(View::Usage);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guides_land_on_round_numbers() {
        assert_eq!(nice_ceiling(0), 0);
        assert_eq!(nice_ceiling(7), 10);
        assert_eq!(nice_ceiling(130_000), 200_000);
        assert_eq!(nice_ceiling(400_000), 500_000);
        assert_eq!(nice_ceiling(1_000), 1_000);
    }

    #[test]
    fn unknown_models_drop_their_provider_and_free_tag() {
        assert_eq!(
            model_name("opencode/muse-spark-1.3-contributor-free"),
            "muse-spark-1.3-contributor"
        );
        assert_eq!(model_name("openrouter/acme/thing:free"), "acme/thing");
    }
}
