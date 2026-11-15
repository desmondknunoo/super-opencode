//! The interface's icons, drawn as vectors on one 16-unit grid with one stroke weight.

use eframe::egui::{Color32, CornerRadius, Painter, Pos2, Rect, Stroke, StrokeKind, vec2};

#[derive(Clone, Copy)]
pub enum Icon {
    Plus,
    Folder,
    /// Two sliders: settings.
    Sliders,
    /// Three bars: usage.
    Bars,
    ArrowUp,
    Stop,
    ChevronDown,
    /// The chevron from the app's mark. It leads each prompt in the transcript.
    Prompt,
    Paperclip,
    Mic,
    Speaker,
    Copy,
    Close,
    /// A page: text files and documents.
    File,
    Image,
    /// A video frame with a play mark.
    Film,
    /// Sound bars: audio files.
    Wave,
    Check,
    /// What OpenCode may do on its own.
    Shield,
    /// Plan mode.
    Bulb,
    /// A triangle with an exclamation mark, for a risky choice.
    Warning,
    /// One path splitting into three: Auto picking a model for each task.
    Fork,
    /// GitHub's mark, simplified to a cat-eared circle.
    GitHub,
    /// An open book: the tutorial.
    Book,
    Back,
    Forward,
    /// A circular arrow: load the page again.
    Reload,
    /// Three lines with dots: the task list.
    List,
    /// A prompt and a cursor: the terminal.
    Terminal,
    /// A box with plus over minus: changed files.
    Diff,
    /// A globe: the browser.
    Globe,
    /// Three dots: more actions.
    More,
    /// Two arrows apart: make the pane fill the window.
    Expand,
    /// Two arrows together: give the conversation its room back.
    Collapse,
    /// A window with its bottom part marked: dock at the bottom.
    PanelBottom,
    /// A window with its right part marked: dock at the side.
    PanelSide,
}

/// Points along an arc of the 16-unit grid, from `start` to `end` degrees (0 is right,
/// 90 is down).
fn arc(center: (f32, f32), radius: f32, start: f32, end: f32) -> Vec<(f32, f32)> {
    let steps = 10;
    (0..=steps)
        .map(|step| {
            let angle = (start + (end - start) * step as f32 / steps as f32).to_radians();
            (
                center.0 + radius * angle.cos(),
                center.1 + radius * angle.sin(),
            )
        })
        .collect()
}

/// Draws `icon` to fill `rect`, which is treated as a 16 by 16 grid.
pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let unit = rect.width() / 16.0;
    let at = |x: f32, y: f32| -> Pos2 { rect.min + vec2(x * unit, y * unit) };
    let stroke = Stroke::new((1.5 * unit).max(1.2), color);
    let line = |points: &[(f32, f32)]| {
        painter.line(points.iter().map(|&(x, y)| at(x, y)).collect(), stroke);
    };
    match icon {
        Icon::Plus => {
            line(&[(8.0, 3.0), (8.0, 13.0)]);
            line(&[(3.0, 8.0), (13.0, 8.0)]);
        }
        Icon::Folder => line(&[
            (2.0, 12.5),
            (2.0, 3.5),
            (6.3, 3.5),
            (7.8, 5.5),
            (14.0, 5.5),
            (14.0, 12.5),
            (2.0, 12.5),
        ]),
        Icon::Sliders => {
            line(&[(2.0, 5.0), (14.0, 5.0)]);
            line(&[(2.0, 11.0), (14.0, 11.0)]);
            for (x, y) in [(10.0, 5.0), (6.0, 11.0)] {
                painter.circle_filled(at(x, y), 2.4 * unit, color);
            }
        }
        Icon::Bars => {
            line(&[(3.5, 13.0), (3.5, 8.5)]);
            line(&[(8.0, 13.0), (8.0, 3.0)]);
            line(&[(12.5, 13.0), (12.5, 6.0)]);
        }
        Icon::ArrowUp => {
            line(&[(8.0, 13.0), (8.0, 3.2)]);
            line(&[(3.8, 7.4), (8.0, 3.2), (12.2, 7.4)]);
        }
        Icon::Stop => {
            painter.rect_filled(
                Rect::from_min_max(at(4.0, 4.0), at(12.0, 12.0)),
                CornerRadius::same(2),
                color,
            );
        }
        Icon::ChevronDown => line(&[(4.0, 6.2), (8.0, 10.2), (12.0, 6.2)]),
        Icon::Paperclip => {
            // Outer loop, then the inner one that holds the paper.
            let mut points = vec![(10.8, 5.0), (10.8, 10.6)];
            points.extend(arc((8.0, 10.6), 2.8, 0.0, 180.0));
            points.push((5.2, 4.4));
            points.extend(arc((7.4, 4.4), 2.2, 180.0, 360.0));
            points.push((9.6, 10.2));
            points.extend(arc((8.0, 10.2), 1.6, 0.0, 180.0));
            points.push((6.4, 5.4));
            line(&points);
        }
        Icon::Mic => {
            let capsule = Rect::from_min_max(at(5.6, 1.8), at(10.4, 10.2));
            painter.rect_stroke(
                capsule,
                CornerRadius::same((2.4 * unit) as u8),
                stroke,
                StrokeKind::Middle,
            );
            line(&arc((8.0, 8.0), 4.6, 0.0, 180.0));
            line(&[(8.0, 12.6), (8.0, 14.4)]);
            line(&[(5.6, 14.4), (10.4, 14.4)]);
        }
        Icon::Speaker => {
            line(&[
                (2.2, 6.0),
                (4.8, 6.0),
                (8.4, 2.8),
                (8.4, 13.2),
                (4.8, 10.0),
                (2.2, 10.0),
                (2.2, 6.0),
            ]);
            line(&arc((8.4, 8.0), 3.0, -50.0, 50.0));
            line(&arc((8.4, 8.0), 5.6, -50.0, 50.0));
        }
        Icon::Copy => {
            painter.rect_stroke(
                Rect::from_min_max(at(5.4, 5.4), at(13.6, 13.6)),
                CornerRadius::same((1.8 * unit) as u8),
                stroke,
                StrokeKind::Middle,
            );
            line(&[(10.6, 2.4), (3.6, 2.4), (2.4, 3.6), (2.4, 10.6)]);
        }
        Icon::Close => {
            line(&[(3.5, 3.5), (12.5, 12.5)]);
            line(&[(12.5, 3.5), (3.5, 12.5)]);
        }
        Icon::File => {
            line(&[
                (3.4, 1.8),
                (9.6, 1.8),
                (12.6, 4.8),
                (12.6, 14.2),
                (3.4, 14.2),
                (3.4, 1.8),
            ]);
            line(&[(9.6, 1.8), (9.6, 4.8), (12.6, 4.8)]);
            line(&[(5.6, 8.4), (10.4, 8.4)]);
            line(&[(5.6, 11.2), (10.4, 11.2)]);
        }
        Icon::Image => {
            painter.rect_stroke(
                Rect::from_min_max(at(1.8, 2.8), at(14.2, 13.2)),
                CornerRadius::same((1.8 * unit) as u8),
                stroke,
                StrokeKind::Middle,
            );
            line(&[
                (2.4, 11.6),
                (6.2, 7.6),
                (9.0, 10.2),
                (10.6, 8.8),
                (13.6, 11.6),
            ]);
            painter.circle_filled(at(10.6, 5.8), 1.2 * unit, color);
        }
        Icon::Film => {
            painter.rect_stroke(
                Rect::from_min_max(at(1.8, 2.8), at(14.2, 13.2)),
                CornerRadius::same((1.8 * unit) as u8),
                stroke,
                StrokeKind::Middle,
            );
            painter.add(eframe::egui::Shape::convex_polygon(
                vec![at(6.6, 5.4), at(10.8, 8.0), at(6.6, 10.6)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Check => line(&[(3.2, 8.4), (6.6, 11.8), (12.8, 4.6)]),
        Icon::Shield => {
            let mut points = vec![(8.0, 1.8), (13.2, 3.8), (13.2, 7.6)];
            points.extend(arc((8.0, 7.6), 5.2, 0.0, 90.0).into_iter().map(|(x, y)| {
                // Flatten the lower curve into the shield's point.
                (x, 7.6 + (y - 7.6) * 1.35)
            }));
            points.extend(
                arc((8.0, 7.6), 5.2, 90.0, 180.0)
                    .into_iter()
                    .map(|(x, y)| (x, 7.6 + (y - 7.6) * 1.35)),
            );
            points.extend([(2.8, 7.6), (2.8, 3.8), (8.0, 1.8)]);
            line(&points);
        }
        Icon::Bulb => {
            let mut points = vec![(6.0, 10.6)];
            points.extend(arc((8.0, 6.4), 4.4, 120.0, 420.0));
            points.push((10.0, 10.6));
            line(&points);
            line(&[(6.0, 10.6), (10.0, 10.6)]);
            line(&[(6.2, 12.6), (9.8, 12.6)]);
            line(&[(6.8, 14.4), (9.2, 14.4)]);
        }
        Icon::Fork => {
            line(&[(8.0, 14.6), (8.0, 8.4)]);
            line(&[(8.0, 8.4), (3.2, 3.2)]);
            line(&[(8.0, 8.4), (8.0, 1.8)]);
            line(&[(8.0, 8.4), (12.8, 3.2)]);
            for (x, y) in [(3.2, 3.2), (8.0, 1.8), (12.8, 3.2)] {
                painter.circle_filled(at(x, y), 1.3 * unit, color);
            }
        }
        Icon::GitHub => {
            // A head with two ears, and the tail curling out at the bottom left.
            let mut head = vec![(4.0, 2.6), (6.2, 4.4)];
            head.extend(arc((8.0, 8.4), 4.6, -110.0, -70.0));
            head.extend([(9.8, 4.4), (12.0, 2.6), (12.4, 6.0)]);
            head.extend(arc((8.0, 8.4), 4.6, -20.0, 200.0));
            head.extend([(3.6, 6.0), (4.0, 2.6)]);
            line(&head);
            line(&[(6.2, 12.8), (6.2, 14.6)]);
            line(&[(9.8, 12.8), (9.8, 14.6)]);
            line(&[(6.2, 13.6), (4.0, 13.2), (3.0, 11.8)]);
        }
        Icon::Back => line(&[(10.4, 3.0), (5.4, 8.0), (10.4, 13.0)]),
        Icon::Forward => line(&[(5.6, 3.0), (10.6, 8.0), (5.6, 13.0)]),
        Icon::Reload => {
            line(&arc((8.0, 8.0), 5.2, -60.0, 250.0));
            line(&[(12.9, 1.9), (10.6, 3.5), (12.9, 5.6)]);
        }
        Icon::List => {
            for y in [4.0, 8.0, 12.0] {
                painter.circle_filled(at(2.8, y), 1.1 * unit, color);
                line(&[(5.6, y), (13.6, y)]);
            }
        }
        Icon::Terminal => {
            line(&[
                (2.6, 2.8),
                (13.4, 2.8),
                (13.4, 13.2),
                (2.6, 13.2),
                (2.6, 2.8),
            ]);
            line(&[(4.8, 6.0), (7.0, 8.0), (4.8, 10.0)]);
            line(&[(8.2, 10.4), (11.2, 10.4)]);
        }
        Icon::Diff => {
            line(&[
                (2.6, 2.6),
                (13.4, 2.6),
                (13.4, 13.4),
                (2.6, 13.4),
                (2.6, 2.6),
            ]);
            line(&[(5.6, 6.2), (10.4, 6.2)]);
            line(&[(8.0, 3.8), (8.0, 8.6)]);
            line(&[(5.6, 10.8), (10.4, 10.8)]);
        }
        Icon::Globe => {
            let mut ring = arc((8.0, 8.0), 6.2, 0.0, 360.0);
            ring.push(ring[0]);
            line(&ring);
            line(&[(1.8, 8.0), (14.2, 8.0)]);
            let mut meridian: Vec<_> = arc((8.0, 8.0), 6.2, -90.0, 270.0)
                .into_iter()
                .map(|(x, y)| (8.0 + (x - 8.0) * 0.45, y))
                .collect();
            meridian.push(meridian[0]);
            line(&meridian);
        }
        Icon::More => {
            for x in [3.2, 8.0, 12.8] {
                painter.circle_filled(at(x, 8.0), 1.3 * unit, color);
            }
        }
        Icon::Expand => {
            line(&[(9.4, 2.6), (13.4, 2.6), (13.4, 6.6)]);
            line(&[(13.4, 2.6), (9.2, 6.8)]);
            line(&[(6.6, 13.4), (2.6, 13.4), (2.6, 9.4)]);
            line(&[(2.6, 13.4), (6.8, 9.2)]);
        }
        Icon::Collapse => {
            line(&[(9.4, 2.8), (9.4, 6.6), (13.2, 6.6)]);
            line(&[(9.4, 6.6), (13.6, 2.4)]);
            line(&[(6.6, 13.2), (6.6, 9.4), (2.8, 9.4)]);
            line(&[(6.6, 9.4), (2.4, 13.6)]);
        }
        Icon::PanelBottom => {
            line(&[
                (2.2, 2.6),
                (13.8, 2.6),
                (13.8, 13.4),
                (2.2, 13.4),
                (2.2, 2.6),
            ]);
            line(&[(2.2, 9.6), (13.8, 9.6)]);
            painter.rect_filled(
                Rect::from_min_max(at(2.6, 10.0), at(13.4, 13.0)),
                0.0,
                color.gamma_multiply(0.5),
            );
        }
        Icon::PanelSide => {
            line(&[
                (2.2, 2.6),
                (13.8, 2.6),
                (13.8, 13.4),
                (2.2, 13.4),
                (2.2, 2.6),
            ]);
            line(&[(9.6, 2.6), (9.6, 13.4)]);
            painter.rect_filled(
                Rect::from_min_max(at(10.0, 3.0), at(13.4, 13.0)),
                0.0,
                color.gamma_multiply(0.5),
            );
        }
        Icon::Book => {
            line(&[(8.0, 3.6), (8.0, 13.6)]);
            line(&[
                (8.0, 3.6),
                (5.0, 2.6),
                (1.8, 3.0),
                (1.8, 12.8),
                (5.0, 12.6),
                (8.0, 13.6),
            ]);
            line(&[
                (8.0, 3.6),
                (11.0, 2.6),
                (14.2, 3.0),
                (14.2, 12.8),
                (11.0, 12.6),
                (8.0, 13.6),
            ]);
        }
        Icon::Warning => {
            line(&[(8.0, 1.8), (14.6, 13.6), (1.4, 13.6), (8.0, 1.8)]);
            line(&[(8.0, 6.0), (8.0, 9.4)]);
            painter.circle_filled(at(8.0, 11.6), 1.0 * unit, color);
        }
        Icon::Wave => {
            for (x, half) in [(2.6, 1.6), (5.3, 4.0), (8.0, 6.0), (10.7, 3.2), (13.4, 1.8)] {
                line(&[(x, 8.0 - half), (x, 8.0 + half)]);
            }
        }
        Icon::Prompt => {
            // A solid chevron, as in the mark, not an outline.
            let points = [
                (3.0, 2.5),
                (7.0, 2.5),
                (13.0, 8.0),
                (7.0, 13.5),
                (3.0, 13.5),
                (9.0, 8.0),
            ];
            painter.add(eframe::egui::Shape::convex_polygon(
                // Two convex halves, so the notch on the left renders correctly.
                [points[0], points[1], points[2], points[5]]
                    .iter()
                    .map(|&(x, y)| at(x, y))
                    .collect(),
                color,
                Stroke::NONE,
            ));
            painter.add(eframe::egui::Shape::convex_polygon(
                [points[5], points[2], points[3], points[4]]
                    .iter()
                    .map(|&(x, y)| at(x, y))
                    .collect(),
                color,
                Stroke::NONE,
            ));
        }
    }
}

/// A focus ring for custom-drawn controls, matching the one egui draws on its own.
pub fn focus_ring(painter: &Painter, rect: Rect, radius: u8, color: Color32) {
    painter.rect_stroke(
        rect.expand(1.5),
        CornerRadius::same(radius + 1),
        Stroke::new(1.5, color),
        StrokeKind::Outside,
    );
}
