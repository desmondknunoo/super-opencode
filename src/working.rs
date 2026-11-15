//! The small moving sign at the start of the live activity line. Each kind of step has its
//! own, so a glance says what is happening: the app's `>_` mark typing while the model
//! thinks, a page being scanned while a file is read, a terminal typing while a command
//! runs, lines being written during an edit, a magnifier sweeping during a search, a globe
//! turning on the web, and two orbits while the model is reached.

use std::f32::consts::TAU;

use eframe::egui::{Color32, CornerRadius, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    Think,
    Read,
    Run,
    Edit,
    Search,
    Web,
    Wait,
}

impl Sign {
    /// The sign for a line of the activity text, as `activity` and the runner word it.
    pub fn of(doing: &str) -> Self {
        let starts = |prefixes: &[&str]| prefixes.iter().any(|prefix| doing.starts_with(prefix));
        if starts(&["Thinking", "Running a subagent", "Briefing a subagent"]) {
            Self::Think
        } else if starts(&["Reading", "Listing", "Choosing a file"]) {
            Self::Read
        } else if starts(&["Running", "Writing a command"]) {
            Self::Run
        } else if starts(&["Searching the web", "Fetching", "Using the browser"]) {
            Self::Web
        } else if starts(&["Searching for", "Looking for", "Writing a search"]) {
            Self::Search
        } else if starts(&["Editing", "Writing", "Updating the plan"]) {
            Self::Edit
        } else {
            Self::Wait
        }
    }
}

/// Smooth 0 → 1 → 0 over one period.
fn wave(time: f32, period: f32) -> f32 {
    0.5 - 0.5 * (time / period * TAU).cos()
}

/// Draws `sign` in `rect` at `time` seconds. `strong` is the moving part, `soft` the frame.
/// With `still`, the sign is drawn at rest.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    sign: Sign,
    time: f64,
    strong: Color32,
    soft: Color32,
    still: bool,
) {
    let time = if still { 0.35 } else { (time % 600.0) as f32 };
    let unit = rect.width() / 16.0;
    let at = |x: f32, y: f32| -> Pos2 { pos2(rect.left() + x * unit, rect.top() + y * unit) };
    let thin = Stroke::new((1.3 * unit).max(1.0), soft);
    let bold = Stroke::new((1.6 * unit).max(1.2), strong);
    match sign {
        Sign::Think => {
            // The mark's chevron, nudging forward, and its underscore typing out and back.
            let nudge = wave(time, 1.6) * 0.8;
            painter.line(
                vec![
                    at(2.6 + nudge, 4.2),
                    at(6.4 + nudge, 8.0),
                    at(2.6 + nudge, 11.8),
                ],
                bold,
            );
            let grow = wave(time, 1.6);
            let start = 8.6;
            painter.line_segment([at(start, 12.0), at(start + 1.6 + grow * 4.6, 12.0)], bold);
        }
        Sign::Read => {
            // A page, with a bar scanning down its lines and leaving them lit.
            let page = Rect::from_min_max(at(3.0, 1.6), at(13.0, 14.4));
            painter.rect_stroke(
                page,
                CornerRadius::same((1.5 * unit) as u8),
                thin,
                StrokeKind::Middle,
            );
            let scan = 3.6 + ((time * 0.9) % 1.0) * 9.2;
            for (index, y) in [4.6_f32, 7.2, 9.8, 12.2].into_iter().enumerate() {
                let length = if index == 3 { 3.6 } else { 5.6 };
                let lit = y < scan;
                painter.line_segment(
                    [at(5.2, y), at(5.2 + length, y)],
                    if lit { bold } else { thin },
                );
            }
            painter.line_segment(
                [at(3.8, scan), at(12.2, scan)],
                Stroke::new(unit.max(0.8), strong.gamma_multiply(0.55)),
            );
        }
        Sign::Run => {
            // A terminal: its prompt, and a cursor typing three steps then starting over.
            let window = Rect::from_min_max(at(1.6, 2.8), at(14.4, 13.2));
            painter.rect_stroke(
                window,
                CornerRadius::same((1.8 * unit) as u8),
                thin,
                StrokeKind::Middle,
            );
            painter.line(vec![at(4.0, 6.0), at(6.0, 8.0), at(4.0, 10.0)], bold);
            let step = ((time * 2.6) as i32 % 4) as f32;
            for typed in 0..step as i32 {
                let x = 7.4 + typed as f32 * 1.7;
                painter.line_segment([at(x, 10.2), at(x + 1.0, 10.2)], thin);
            }
            let blink = (time * 2.6).fract() < 0.7;
            if blink {
                let x = 7.4 + step * 1.7;
                painter.rect_filled(
                    Rect::from_min_max(at(x, 6.6), at(x + 1.3, 10.6)),
                    CornerRadius::ZERO,
                    strong,
                );
            }
        }
        Sign::Edit => {
            // Lines written one after another, each growing behind a caret.
            let cycle = (time * 0.7) % 3.0;
            let rows = [4.0_f32, 8.0, 12.0];
            let lengths = [10.0_f32, 7.6, 9.0];
            for (index, (y, length)) in rows.into_iter().zip(lengths).enumerate() {
                let progress = (cycle - index as f32).clamp(0.0, 1.0);
                if index as f32 > cycle {
                    painter.line_segment(
                        [at(3.0, y), at(3.0 + length, y)],
                        Stroke::new(thin.width, soft.gamma_multiply(0.5)),
                    );
                    continue;
                }
                let end = 3.0 + length * progress;
                painter.line_segment(
                    [at(3.0, y), at(end.max(3.2), y)],
                    if progress < 1.0 { bold } else { thin },
                );
                if progress < 1.0 {
                    painter.line_segment(
                        [at(end + 0.9, y - 2.0), at(end + 0.9, y + 1.6)],
                        Stroke::new(unit.max(0.9), strong),
                    );
                }
            }
        }
        Sign::Search => {
            // A line of text, and a magnifier sweeping along it.
            painter.line_segment([at(1.6, 12.6), at(14.4, 12.6)], thin);
            let x = 4.6 + wave(time, 2.0) * 6.4;
            let center = at(x, 6.6);
            painter.circle_stroke(center, 3.2 * unit, bold);
            painter.line_segment([at(x + 2.3, 8.9), at(x + 4.2, 10.8)], bold);
        }
        Sign::Web => {
            // A globe turning: its meridian sweeps from edge to edge.
            let center = at(8.0, 8.0);
            let radius = 6.0 * unit;
            painter.circle_stroke(center, radius, thin);
            painter.line_segment([at(2.0, 8.0), at(14.0, 8.0)], thin);
            let turn = (time * 0.8) % 1.0;
            for offset in [0.0_f32, 0.5] {
                let phase = (turn + offset) % 1.0;
                let width = (phase * TAU).cos() * radius;
                let points: Vec<Pos2> = (0..=16)
                    .map(|step| {
                        let angle =
                            step as f32 / 16.0 * std::f32::consts::PI - std::f32::consts::FRAC_PI_2;
                        center + vec2(width * angle.cos(), radius * angle.sin())
                    })
                    .collect();
                let front = (phase * TAU).sin() > 0.0;
                painter.line(points, if front { bold } else { thin });
            }
        }
        Sign::Wait => {
            // Two tilted orbits, each with a dot travelling on it.
            let center = at(8.0, 8.0);
            for (tilt, speed, offset) in [(0.6_f32, 1.0_f32, 0.0_f32), (-0.6, -0.8, 0.5)] {
                let (sin, cos) = tilt.sin_cos();
                let orbit = |angle: f32| {
                    let (x, y) = (6.2 * unit * angle.cos(), 2.4 * unit * angle.sin());
                    center + vec2(x * cos - y * sin, x * sin + y * cos)
                };
                let points: Vec<Pos2> = (0..=32)
                    .map(|step| orbit(step as f32 / 32.0 * TAU))
                    .collect();
                painter.line(points, thin);
                let angle = (time * speed + offset) * TAU;
                painter.circle_filled(orbit(angle), 1.6 * unit, strong);
            }
            painter.circle_filled(center, 1.1 * unit, soft);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_step_has_its_sign() {
        assert_eq!(Sign::of("Thinking"), Sign::Think);
        assert_eq!(Sign::of("Reading src/main.rs"), Sign::Read);
        assert_eq!(Sign::of("Running cargo test"), Sign::Run);
        assert_eq!(Sign::of("Writing a command"), Sign::Run);
        assert_eq!(Sign::of("Editing src/app.rs"), Sign::Edit);
        assert_eq!(Sign::of("Writing the reply"), Sign::Edit);
        assert_eq!(Sign::of("Searching for “total”"), Sign::Search);
        assert_eq!(Sign::of("Searching the web for “egui”"), Sign::Web);
        assert_eq!(Sign::of("Fetching https://example.com"), Sign::Web);
        assert_eq!(Sign::of("Waiting for the model"), Sign::Wait);
        assert_eq!(Sign::of("Starting OpenCode"), Sign::Wait);
        // Every phrase the activity module writes lands on a sign.
        for tool in [
            "read",
            "shell",
            "edit",
            "write",
            "grep",
            "glob",
            "webfetch",
            "websearch",
            "subagent",
        ] {
            let _ = Sign::of(&crate::activity::doing(tool, "x"));
            let _ = Sign::of(&crate::activity::preparing(tool));
        }
        assert_eq!(
            Sign::of(&crate::activity::doing("glob", "*.rs")),
            Sign::Search
        );
        assert_eq!(
            Sign::of(&crate::activity::doing("subagent", "audit")),
            Sign::Think
        );
    }
}
