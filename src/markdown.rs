//! Draws model output: paragraphs, headings, lists, code blocks, links, and tool notices.
//!
//! This covers the Markdown that coding models write most. Anything else is shown as typed.
//! Web addresses and `[text](url)` links open in the browser.

use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Label, Layout, Margin, RichText, ScrollArea, Stroke,
    TextFormat, TextWrapMode, Ui, text::LayoutJob, vec2,
};

use crate::{
    activity::{self, Notice},
    theme::{self, Palette},
};

enum Block<'a> {
    Paragraph(Vec<&'a str>),
    Heading(usize, &'a str),
    Item {
        depth: usize,
        marker: String,
        text: &'a str,
    },
    Code(Vec<&'a str>),
    Tool(Notice<'a>),
    Table {
        header: Vec<String>,
        align: Vec<Align>,
        rows: Vec<Vec<String>>,
    },
    /// Pictures read one after another, shown together as a strip of thumbnails.
    Images(Vec<Notice<'a>>),
}

/// Files the transcript can show and open: where relative paths start, and thumbnails of
/// the pictures steps read, loaded by the caller.
pub struct Files<'a> {
    pub project: Option<&'a std::path::Path>,
    pub thumbs: &'a std::collections::HashMap<std::path::PathBuf, egui::TextureHandle>,
}

/// The pictures a reply's steps read, as paths in the project, for loading thumbnails.
pub fn image_paths(text: &str, project: &std::path::Path) -> Vec<std::path::PathBuf> {
    text.lines()
        .filter_map(activity::parse)
        .filter(|notice| notice.tool == "read" && activity::is_image_path(notice.detail))
        .map(|notice| project.join(notice.detail))
        .collect()
}

/// Folds runs of picture reads into one block each.
fn group_images(blocks: Vec<Block<'_>>) -> Vec<Block<'_>> {
    let mut out: Vec<Block<'_>> = Vec::new();
    for block in blocks {
        match block {
            Block::Tool(notice)
                if notice.tool == "read"
                    && !notice.failed()
                    && activity::is_image_path(notice.detail) =>
            {
                if let Some(Block::Images(run)) = out.last_mut() {
                    run.push(notice);
                } else {
                    out.push(Block::Images(vec![notice]));
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// The cells of a table row: `| a | b |` or `a | b`, with `\|` kept as a bar and bars inside
/// code spans left alone.
fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    let mut out = Vec::new();
    let mut cell = String::new();
    let mut code = false;
    let mut characters = line.chars().peekable();
    while let Some(c) = characters.next() {
        match c {
            '\\' if characters.peek() == Some(&'|') => {
                cell.push('|');
                characters.next();
            }
            '`' => {
                code = !code;
                cell.push(c);
            }
            '|' if !code => out.push(std::mem::take(&mut cell).trim().to_owned()),
            _ => cell.push(c),
        }
    }
    out.push(cell.trim().to_owned());
    out
}

/// The alignments from a table's divider row, such as `|---|:---:|--:|`, or `None` when the
/// line isn't one.
fn divider(line: &str) -> Option<Vec<Align>> {
    let trimmed = line.trim();
    if !trimmed.contains('-') || !trimmed.contains('|') && !trimmed.starts_with('-') {
        return None;
    }
    cells(trimmed)
        .iter()
        .map(|cell| {
            let left = cell.starts_with(':');
            let right = cell.ends_with(':');
            let dashes = cell.trim_matches(':');
            (!dashes.is_empty() && dashes.chars().all(|c| c == '-')).then_some(
                match (left, right) {
                    (true, true) => Align::Center,
                    (false, true) => Align::Max,
                    _ => Align::Min,
                },
            )
        })
        .collect()
}

/// Marker stored for an unordered item. It is drawn as a dot, not typed as this character.
const BULLET: &str = "•";

fn list_item(line: &str) -> Option<(usize, String, &str)> {
    let content = line.trim_start();
    let depth = (line.len() - content.len()) / 2;
    for bullet in ["- ", "* ", "+ "] {
        if let Some(text) = content.strip_prefix(bullet) {
            return Some((depth, BULLET.to_owned(), text));
        }
    }
    let digits = content.chars().take_while(char::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        let rest = &content[digits..];
        if let Some(text) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((depth, format!("{}.", &content[..digits]), text));
        }
    }
    None
}

fn parse(text: &str) -> Vec<Block<'_>> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut code: Option<Vec<&str>> = None;
    let lines: Vec<&str> = text.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if let Some(lines) = &mut code {
            if line.trim_start().starts_with("```") {
                blocks.push(Block::Code(code.take().unwrap_or_default()));
            } else {
                lines.push(line);
            }
            continue;
        }
        // A table: a row of cells, a divider, and the rows that follow.
        if line.trim_start().starts_with('|')
            && let Some(align) = lines.get(index).and_then(|next| divider(next))
        {
            let header = cells(line);
            if header.len() == align.len() {
                index += 1;
                let mut rows = Vec::new();
                while let Some(row) = lines
                    .get(index)
                    .filter(|row| row.trim_start().starts_with('|'))
                {
                    let mut row = cells(row);
                    row.resize(header.len(), String::new());
                    rows.push(row);
                    index += 1;
                }
                if !paragraph.is_empty() {
                    blocks.push(Block::Paragraph(std::mem::take(&mut paragraph)));
                }
                blocks.push(Block::Table {
                    header,
                    align,
                    rows,
                });
                continue;
            }
        }
        let block = if line.trim_start().starts_with("```") {
            code = Some(Vec::new());
            None
        } else if let Some(notice) = activity::parse(line) {
            Some(Block::Tool(notice))
        } else if let Some((depth, marker, text)) = list_item(line) {
            Some(Block::Item {
                depth,
                marker,
                text,
            })
        } else {
            let level = line.chars().take_while(|&c| c == '#').count();
            match line[level..].strip_prefix(' ') {
                Some(title) if (1..=4).contains(&level) => Some(Block::Heading(level, title)),
                _ if line.trim().is_empty() => None,
                _ => {
                    paragraph.push(line);
                    continue;
                }
            }
        };
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(std::mem::take(&mut paragraph)));
        }
        blocks.extend(block);
    }
    if !paragraph.is_empty() {
        blocks.push(Block::Paragraph(paragraph));
    }
    // A fence still open at the end is a code block the model is in the middle of writing.
    if let Some(lines) = code {
        blocks.push(Block::Code(lines));
    }
    blocks
}

struct Formats {
    plain: TextFormat,
    strong: TextFormat,
    code: TextFormat,
    ok: Color32,
    danger: Color32,
    warn: Color32,
}

impl Formats {
    fn new(palette: &Palette, size: f32, color: Color32) -> Self {
        let line_height = Some(size * 1.5);
        Self {
            plain: TextFormat {
                font_id: theme::text(size),
                color,
                line_height,
                ..Default::default()
            },
            strong: TextFormat {
                font_id: theme::strong(size),
                color,
                line_height,
                ..Default::default()
            },
            code: TextFormat {
                font_id: theme::mono(size - 1.5),
                color,
                background: palette.code_ground,
                line_height,
                ..Default::default()
            },
            ok: palette.ok,
            danger: palette.danger,
            warn: palette.warn,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Style {
    Plain,
    Strong,
    Code,
}

#[derive(Debug, PartialEq)]
struct Span {
    text: String,
    style: Style,
    link: Option<String>,
    italic: bool,
    struck: bool,
}

impl Span {
    fn new(text: String, style: Style, link: Option<String>) -> Self {
        Self {
            text,
            style,
            link,
            italic: false,
            struck: false,
        }
    }
}

/// The end of a web address that starts at the beginning of `text`. Punctuation that ends a
/// sentence is left out, and so is a closing bracket the address did not open.
fn address_end(text: &str) -> usize {
    let mut end = text
        .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`'))
        .unwrap_or(text.len());
    loop {
        let candidate = &text[..end];
        let Some(last) = candidate.chars().last() else {
            return 0;
        };
        let unbalanced = |open: char, close: char| {
            last == close && candidate.matches(open).count() < candidate.matches(close).count()
        };
        if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*')
            || unbalanced('(', ')')
            || unbalanced('[', ']')
        {
            end -= last.len_utf8();
        } else {
            return end;
        }
    }
}

fn starts_address(text: &str) -> bool {
    (text.starts_with("https://") || text.starts_with("http://")) && address_end(text) > 8
}

/// Splits one line into styled runs. With `markdown` off, only web addresses are found.
fn spans(text: &str, markdown: bool) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let mut bold = false;
    let mut italic = false;
    let mut struck = false;
    let mut rest = text;
    let style = |bold: bool| if bold { Style::Strong } else { Style::Plain };
    let flush =
        |out: &mut Vec<Span>, plain: &mut String, bold: bool, italic: bool, struck: bool| {
            if !plain.is_empty() {
                out.push(Span {
                    text: std::mem::take(plain),
                    style: style(bold),
                    link: None,
                    italic,
                    struck,
                });
            }
        };
    let mut previous: Option<char> = None;
    while let Some(c) = rest.chars().next() {
        let boundary = previous.is_none_or(|p| !p.is_alphanumeric() && p != '/');
        if boundary && starts_address(rest) {
            let end = address_end(rest);
            flush(&mut out, &mut plain, bold, italic, struck);
            out.push(Span {
                italic,
                struck,
                ..Span::new(
                    rest[..end].to_owned(),
                    style(bold),
                    Some(rest[..end].to_owned()),
                )
            });
            previous = rest[..end].chars().last();
            rest = &rest[end..];
            continue;
        }
        if markdown {
            if let Some(after) = rest.strip_prefix('`')
                && let Some(end) = after.find('`')
            {
                flush(&mut out, &mut plain, bold, italic, struck);
                out.push(Span::new(after[..end].to_owned(), Style::Code, None));
                previous = Some('`');
                rest = &after[end + 1..];
                continue;
            }
            if let Some(after) = rest.strip_prefix("**")
                && (bold || after.contains("**"))
            {
                flush(&mut out, &mut plain, bold, italic, struck);
                bold = !bold;
                previous = Some('*');
                rest = after;
                continue;
            }
            if let Some(after) = rest.strip_prefix("~~")
                && (struck || after.contains("~~"))
            {
                flush(&mut out, &mut plain, bold, italic, struck);
                struck = !struck;
                previous = Some('~');
                rest = after;
                continue;
            }
            // *word* or _word_: it opens before a word and closes after one, so `a * b` and
            // snake_case names stay as typed.
            if (c == '*' || c == '_') && !rest.starts_with("**") {
                let after = &rest[1..];
                let next = after.chars().next();
                let opens = !italic
                    && boundary
                    && next.is_some_and(|n| !n.is_whitespace() && n != c)
                    && after.find(c).is_some_and(|close| {
                        close > 0
                            && !after[..close].ends_with(char::is_whitespace)
                            && after[close + 1..]
                                .chars()
                                .next()
                                .is_none_or(|n| !n.is_alphanumeric())
                    });
                let closes = italic
                    && previous.is_some_and(|p| !p.is_whitespace())
                    && next.is_none_or(|n| !n.is_alphanumeric());
                if opens || closes {
                    flush(&mut out, &mut plain, bold, italic, struck);
                    italic = !italic;
                    previous = Some(c);
                    rest = after;
                    continue;
                }
            }
            if let Some(after) = rest.strip_prefix('[')
                && let Some((label, tail)) = after.split_once("](")
                && !label.is_empty()
                && !label.contains(']')
                && let Some((url, remainder)) = tail.split_once(')')
                && (starts_address(url) || url.starts_with("mailto:"))
                && !url.contains(char::is_whitespace)
            {
                flush(&mut out, &mut plain, bold, italic, struck);
                out.push(Span {
                    italic,
                    struck,
                    ..Span::new(
                        label.replace("**", "").replace('`', ""),
                        style(bold),
                        Some(url.to_owned()),
                    )
                });
                previous = Some(')');
                rest = remainder;
                continue;
            }
        }
        plain.push(c);
        previous = Some(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut out, &mut plain, bold, italic, struck);
    out
}

/// The format for a span: its style, slanted or struck through as marked.
fn format_of(span: &Span, formats: &Formats) -> TextFormat {
    let mut format = match span.style {
        Style::Plain => formats.plain.clone(),
        Style::Strong => formats.strong.clone(),
        Style::Code => formats.code.clone(),
    };
    format.italics = span.italic;
    if span.struck {
        format.strikethrough = Stroke::new(1.0, format.color);
    }
    format
}

/// Appends `text` with status marks such as ✅, ❌, and ⚠ in their colours, so a table of
/// results reads at a glance.
fn append_marked(job: &mut LayoutJob, text: &str, format: &TextFormat, formats: &Formats) {
    let mut start = 0;
    for (at, c) in text.char_indices() {
        let color = match c {
            '✅' | '✔' | '☑' => Some(formats.ok),
            '❌' | '✖' | '✗' | '✘' => Some(formats.danger),
            '⚠' => Some(formats.warn),
            _ => None,
        };
        if let Some(color) = color {
            if at > start {
                job.append(&text[start..at], 0.0, format.clone());
            }
            let mut marked = format.clone();
            marked.color = color;
            job.append(&text[at..at + c.len_utf8()], 0.0, marked);
            start = at + c.len_utf8();
        }
    }
    if start < text.len() {
        job.append(&text[start..], 0.0, format.clone());
    }
}

fn job(spans: &[&Span], formats: &Formats) -> LayoutJob {
    let mut job = LayoutJob::default();
    for span in spans {
        append_marked(&mut job, &span.text, &format_of(span, formats), formats);
    }
    job
}

/// One line of text that wraps, with its links clickable. The line is laid out as one
/// piece of text so links sit on the same baseline as the words around them.
fn line(ui: &mut Ui, palette: &Palette, spans: &[Span], formats: &Formats) {
    if spans.iter().all(|span| span.link.is_none()) {
        let all: Vec<&Span> = spans.iter().collect();
        ui.add(Label::new(job(&all, formats)).wrap());
        return;
    }
    let mut layout = LayoutJob::default();
    let mut links: Vec<(std::ops::Range<usize>, &str)> = Vec::new();
    let mut at = 0;
    for span in spans {
        let mut format = format_of(span, formats);
        let count = span.text.chars().count();
        if let Some(url) = &span.link {
            format.color = palette.accent;
            links.push((at..at + count, url));
        }
        layout.append(&span.text, 0.0, format);
        at += count;
    }
    layout.wrap.max_width = ui.available_width();
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(layout));
    let (rect, response) = ui.allocate_exact_size(galley.size(), egui::Sense::click());
    // The link under the pointer: the character there, when the pointer is on the glyph
    // rather than past the end of a line.
    let hovered = response.hover_pos().and_then(|pointer| {
        let local = pointer - rect.min;
        let cursor = galley.cursor_from_pos(local);
        let index = cursor.index.0;
        let caret = galley.pos_from_cursor(cursor);
        let next = galley.pos_from_cursor(egui::text::CCursor::new(index + 1));
        let within = local.y >= caret.top() - 1.0
            && local.y <= caret.bottom() + 1.0
            && local.x <= next.left().max(caret.left()) + 1.0;
        let probe = if local.x < caret.left() {
            index.saturating_sub(1)
        } else {
            index
        };
        within
            .then(|| links.iter().find(|(range, _)| range.contains(&probe)))
            .flatten()
            .map(|(range, url)| (range.clone(), (*url).to_owned()))
    });
    ui.painter().galley(rect.min, galley.clone(), palette.ink);
    // Each link is underlined just under its baseline: faintly, and fully when hovered.
    for (range, _) in &links {
        let lit = hovered
            .as_ref()
            .is_some_and(|(hovered, _)| hovered == range);
        let color = if lit {
            palette.accent
        } else {
            palette.accent.gamma_multiply(0.45)
        };
        for (x, y) in underlines(&galley, range) {
            ui.painter().hline(
                (rect.min.x + x.start)..=(rect.min.x + x.end),
                rect.min.y + y,
                Stroke::new(1.0, color),
            );
        }
    }
    if let Some((_, url)) = hovered {
        let response = response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(&url);
        if response.clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
    }
}

/// Where to underline the characters in `range`: one horizontal run per row they occupy,
/// as the run's left and right edges and the height just under the baseline.
fn underlines(
    galley: &egui::Galley,
    range: &std::ops::Range<usize>,
) -> Vec<(std::ops::Range<f32>, f32)> {
    let mut runs: Vec<(std::ops::Range<f32>, f32)> = Vec::new();
    let mut index = 0;
    for placed in &galley.rows {
        for (offset, glyph) in placed.row.glyphs.iter().enumerate() {
            if !range.contains(&(index + offset)) || glyph.chr.is_whitespace() && offset == 0 {
                continue;
            }
            let left = placed.pos.x + glyph.pos.x;
            let right = left + glyph.advance_width;
            let y = placed.pos.y + glyph.pos.y + 2.0;
            match runs.last_mut() {
                Some((run, at)) if (*at - y).abs() < 0.5 && (run.end - left).abs() < 1.0 => {
                    run.end = right;
                }
                _ => runs.push((left..right, y)),
            }
        }
        index += placed.char_count_including_newline().0;
    }
    runs
}

/// A prompt as typed: no Markdown, but its web addresses open in the browser.
pub fn plain(ui: &mut Ui, palette: &Palette, text: &str) {
    let formats = Formats::new(palette, theme::READING, palette.ink);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for text_line in text.lines() {
            if text_line.trim().is_empty() {
                ui.add_space(theme::READING * 0.75);
            } else {
                line(ui, palette, &spans(text_line, false), &formats);
            }
        }
    });
}

/// The words of a response as they should be read aloud: no code, tool notices, Markdown
/// marks, or spelled-out web addresses.
pub fn speakable(text: &str) -> String {
    let words = |text: &str| -> String {
        spans(text, true)
            .into_iter()
            .map(|span| match &span.link {
                Some(url) if *url == span.text => "a link".to_owned(),
                _ => span.text,
            })
            .collect()
    };
    let mut out = Vec::new();
    for block in parse(text) {
        match block {
            Block::Paragraph(lines) => out.push(
                lines
                    .iter()
                    .map(|line| words(line))
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            Block::Heading(_, title) => out.push(format!("{}.", words(title))),
            Block::Item { text, .. } => out.push(words(text)),
            Block::Code(_) | Block::Tool(_) | Block::Images(_) => {}
            Block::Table { header, rows, .. } => {
                for row in std::iter::once(&header).chain(&rows) {
                    let said: Vec<String> = row.iter().map(|cell| words(cell)).collect();
                    out.push(format!("{}.", said.join(", ")));
                }
            }
        }
    }
    out.join("\n")
}

/// A Markdown table: the header on a tinted row, lines between rows and columns, cells
/// aligned as the divider row says and wrapped when the table is wider than the column.
fn table(
    ui: &mut Ui,
    palette: &Palette,
    header: &[String],
    align: &[Align],
    rows: &[Vec<String>],
    index: usize,
) {
    const PAD_X: f32 = 12.0;
    const PAD_Y: f32 = 7.0;
    let formats = Formats::new(palette, theme::TEXT, palette.ink);
    let cell_job = |text: &str, strong: bool| {
        let mut spans = spans(text, true);
        if strong {
            for span in &mut spans {
                if span.style == Style::Plain {
                    span.style = Style::Strong;
                }
            }
        }
        let all: Vec<&Span> = spans.iter().collect();
        let mut job = job(&all, &formats);
        for section in &mut job.sections {
            section.format.line_height = Some(theme::TEXT * 1.45);
        }
        job
    };
    let columns = header.len().max(1);
    // Each column's natural width, then a fair share of the room when they don't all fit.
    let natural: Vec<f32> = (0..columns)
        .map(|column| {
            std::iter::once((header.get(column), true))
                .chain(rows.iter().map(|row| (row.get(column), false)))
                .filter_map(|(cell, strong)| cell.map(|cell| (cell, strong)))
                .map(|(cell, strong)| {
                    let mut job = cell_job(cell, strong);
                    job.wrap.max_width = f32::INFINITY;
                    ui.fonts_mut(|fonts| fonts.layout_job(job)).size().x
                })
                .fold(24.0_f32, f32::max)
        })
        .collect();
    let room = (ui.available_width() - columns as f32 * PAD_X * 2.0).max(columns as f32 * 60.0);
    let mut widths = natural.clone();
    if natural.iter().sum::<f32>() > room {
        let mut left = room;
        let mut open: Vec<usize> = (0..columns).collect();
        open.sort_by(|a, b| natural[*a].total_cmp(&natural[*b]));
        let mut remaining = columns;
        for column in open {
            let share = left / remaining as f32;
            widths[column] = natural[column].min(share).max(60.0);
            left -= widths[column];
            remaining -= 1;
        }
    }
    let lay = |text: &str, strong: bool, width: f32| {
        let mut job = cell_job(text, strong);
        job.wrap.max_width = width;
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let mut laid: Vec<Vec<std::sync::Arc<egui::Galley>>> = Vec::new();
    laid.push(
        (0..columns)
            .map(|column| {
                lay(
                    header.get(column).map_or("", String::as_str),
                    true,
                    widths[column],
                )
            })
            .collect(),
    );
    for row in rows {
        laid.push(
            (0..columns)
                .map(|column| {
                    lay(
                        row.get(column).map_or("", String::as_str),
                        false,
                        widths[column],
                    )
                })
                .collect(),
        );
    }
    let heights: Vec<f32> = laid
        .iter()
        .map(|row| row.iter().map(|galley| galley.size().y).fold(0.0, f32::max) + PAD_Y * 2.0)
        .collect();
    let total_width: f32 = widths.iter().map(|width| width + PAD_X * 2.0).sum();
    let total_height: f32 = heights.iter().sum();
    ScrollArea::horizontal()
        .id_salt(("table", index))
        .show(ui, |ui| {
            let (rect, _) =
                ui.allocate_exact_size(vec2(total_width, total_height), egui::Sense::hover());
            let painter = ui.painter();
            let round = CornerRadius::same(8);
            painter.rect_filled(rect, round, palette.raised);
            let header_rect = egui::Rect::from_min_size(rect.min, vec2(total_width, heights[0]));
            painter.rect_filled(
                header_rect,
                CornerRadius {
                    nw: 8,
                    ne: 8,
                    sw: 0,
                    se: 0,
                },
                palette.code_ground,
            );
            let line = Stroke::new(1.0, palette.line);
            let mut y = rect.top();
            for (row_index, row) in laid.into_iter().enumerate() {
                if row_index > 0 {
                    painter.hline(rect.x_range(), y, line);
                }
                let mut x = rect.left();
                for (column, galley) in row.into_iter().enumerate() {
                    let width = widths[column];
                    let offset = match align.get(column).copied().unwrap_or(Align::Min) {
                        Align::Center => (width - galley.size().x) / 2.0,
                        Align::Max => width - galley.size().x,
                        Align::Min => 0.0,
                    };
                    painter.galley(
                        egui::pos2(x + PAD_X + offset.max(0.0), y + PAD_Y),
                        galley,
                        palette.ink,
                    );
                    x += width + PAD_X * 2.0;
                    if column + 1 < columns {
                        painter.vline(x, (y + 0.0)..=(y + heights[row_index]), line);
                    }
                }
                y += heights[row_index];
            }
            painter.rect_stroke(
                rect,
                round,
                Stroke::new(1.0, palette.line_strong),
                egui::StrokeKind::Inside,
            );
        });
}

/// One finished step: “Read src/main.rs  0.4s”, or “Thought for 2.1s”.
/// Pictures read one after another: one row saying how many and how long, then their
/// thumbnails. Returns the picture clicked.
fn images(
    ui: &mut Ui,
    palette: &Palette,
    notices: &[Notice],
    files: &Files,
) -> Option<std::path::PathBuf> {
    let took: i64 = notices
        .iter()
        .filter_map(|notice| activity::duration_ms(notice.took))
        .sum();
    let words = if notices.len() == 1 {
        "Read a picture".to_owned()
    } else {
        format!("Read {} pictures", notices.len())
    };
    let size = vec2(ui.available_width(), 22.0);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) = ui.allocate_exact_size(vec2(10.0, 22.0), egui::Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 2.5, palette.ink_faint);
        ui.add(
            Label::new(
                RichText::new(words)
                    .font(theme::text(theme::TEXT))
                    .color(palette.ink_muted),
            )
            .selectable(false),
        );
        if took > 0 {
            ui.add(
                Label::new(
                    RichText::new(activity::duration(took))
                        .font(theme::text(theme::SMALL))
                        .color(palette.ink_faint),
                )
                .selectable(false),
            );
        }
    });
    ui.add_space(6.0);
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        ui.add_space(18.0);
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for notice in notices {
            let relative = std::path::Path::new(notice.detail);
            let path = match files.project {
                Some(project) if relative.is_relative() => project.join(relative),
                _ => relative.to_path_buf(),
            };
            let (rect, response) = ui.allocate_exact_size(vec2(84.0, 84.0), egui::Sense::click());
            let round = CornerRadius::same(10);
            match files.thumbs.get(&path) {
                Some(texture) => {
                    // Cropped to a square from the middle, like the attachment cards.
                    let full = texture.size_vec2();
                    let side = full.x.min(full.y).max(1.0);
                    let uv = egui::Rect::from_min_size(
                        egui::pos2(
                            (full.x - side) / 2.0 / full.x,
                            (full.y - side) / 2.0 / full.y,
                        ),
                        vec2(side / full.x, side / full.y),
                    );
                    egui::Image::new(texture)
                        .uv(uv)
                        .corner_radius(round)
                        .paint_at(ui, rect);
                }
                None => {
                    ui.painter().rect_filled(rect, round, palette.code_ground);
                    crate::icons::paint(
                        ui.painter(),
                        egui::Rect::from_center_size(rect.center(), vec2(20.0, 20.0)),
                        crate::icons::Icon::Image,
                        palette.ink_faint,
                    );
                }
            }
            let stroke = if response.hovered() {
                Stroke::new(1.5, palette.accent)
            } else {
                Stroke::new(1.0, palette.line)
            };
            ui.painter()
                .rect_stroke(rect, round, stroke, egui::StrokeKind::Inside);
            let name = relative.file_name().map_or_else(
                || notice.detail.to_owned(),
                |name| name.to_string_lossy().into_owned(),
            );
            if response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(name)
                .clicked()
            {
                clicked = Some(path);
            }
        }
    });
    ui.add_space(6.0);
    clicked
}

/// One finished step's row. With `opens`, the row opens its file when clicked; returns
/// whether it was.
fn step(ui: &mut Ui, palette: &Palette, notice: &Notice, full: bool, opens: bool) -> bool {
    let failed = notice.failed();
    let thought = notice.tool == "reasoning";
    let mut job = LayoutJob::default();
    let plain = |color| TextFormat {
        font_id: theme::text(theme::TEXT),
        color,
        valign: Align::Center,
        ..Default::default()
    };
    if thought {
        let words = if notice.took.is_empty() {
            "Thought".to_owned()
        } else {
            format!("Thought for {}", notice.took)
        };
        job.append(&words, 0.0, plain(palette.ink_faint));
    } else {
        let sentence = notice.sentence();
        let verb_end = sentence.len() - notice.detail.len();
        let verb = sentence[..verb_end].trim_end();
        job.append(
            verb,
            0.0,
            plain(if failed {
                palette.danger
            } else {
                palette.ink_muted
            }),
        );
        if !notice.detail.is_empty() {
            job.append(
                notice.detail,
                6.0,
                TextFormat {
                    font_id: theme::mono(theme::CODE),
                    color: palette.ink,
                    valign: Align::Center,
                    ..Default::default()
                },
            );
        }
        if !notice.took.is_empty() {
            job.append(
                notice.took,
                10.0,
                TextFormat {
                    font_id: theme::text(theme::SMALL),
                    color: palette.ink_faint,
                    valign: Align::Center,
                    ..Default::default()
                },
            );
        }
        if failed {
            job.append(
                "failed",
                10.0,
                TextFormat {
                    font_id: theme::text(theme::SMALL),
                    color: palette.danger,
                    valign: Align::Center,
                    ..Default::default()
                },
            );
        }
    }
    if !full {
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('…');
    }
    job.wrap.break_anywhere = true;
    let size = vec2(ui.available_width(), 22.0);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) = ui.allocate_exact_size(vec2(10.0, 22.0), egui::Sense::hover());
        let mark = if failed {
            palette.danger
        } else if thought {
            palette.line_strong
        } else {
            palette.ink_faint
        };
        ui.painter().circle_filled(rect.center(), 2.5, mark);
        let mode = if full {
            TextWrapMode::Wrap
        } else {
            TextWrapMode::Truncate
        };
        let sense = if opens {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let shown = ui.add(
            Label::new(job)
                .wrap_mode(mode)
                .selectable(full && !opens)
                .sense(sense),
        );
        let shown = if opens {
            shown.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            shown
        };
        let shown = if notice.detail.is_empty() {
            shown
        } else if opens {
            shown.on_hover_text(format!("Open {}", notice.detail))
        } else {
            shown.on_hover_text(notice.detail)
        };
        opens && shown.clicked()
    })
    .inner
}

/// Draws `text` at the reading size across the available width. `transcript` decides which
/// step rows show and how much of each. Returns a file the reader clicked, to open.
pub fn show(
    ui: &mut Ui,
    palette: &Palette,
    text: &str,
    transcript: crate::storage::Transcript,
    files: &Files,
) -> Option<std::path::PathBuf> {
    use crate::storage::Transcript;
    let formats = Formats::new(palette, theme::READING, palette.ink);
    let mut previous_was_tool = false;
    let mut previous_was_table = false;
    let mut clicked = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    let blocks = group_images(parse(text)).into_iter().filter(|block| {
        // Normal leaves out the rows that only say how long the model thought.
        !(transcript == Transcript::Normal
            && matches!(block, Block::Tool(notice) if notice.tool == "reasoning"))
    });
    for (index, block) in blocks.enumerate() {
        let is_tool = matches!(block, Block::Tool(_) | Block::Images(_));
        if index > 0 {
            ui.add_space(if is_tool && previous_was_tool {
                2.0
            } else if matches!(block, Block::Table { .. }) || previous_was_table {
                16.0
            } else if matches!(block, Block::Heading(..)) {
                18.0
            } else if matches!(block, Block::Item { .. }) {
                5.0
            } else {
                10.0
            });
        }
        previous_was_tool = is_tool;
        previous_was_table = matches!(block, Block::Table { .. });
        match block {
            Block::Table {
                header,
                align,
                rows,
            } => table(ui, palette, &header, &align, &rows, index),
            Block::Paragraph(lines) => {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for text_line in lines {
                        line(ui, palette, &spans(text_line, true), &formats);
                    }
                });
            }
            Block::Heading(level, title) => {
                let size = if level <= 2 {
                    theme::READING + 2.0
                } else {
                    theme::READING
                };
                ui.add(
                    Label::new(
                        RichText::new(title)
                            .font(theme::strong(size))
                            .color(palette.ink),
                    )
                    .wrap(),
                );
            }
            Block::Item {
                depth,
                marker,
                text,
            } => {
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(depth.min(4) as f32 * 16.0);
                    // The marker is laid out like the item's text, so it shares its baseline.
                    let gutter = if marker == BULLET { 20.0 } else { 26.0 };
                    let (rect, _) = ui.allocate_exact_size(
                        vec2(gutter, theme::READING * 1.5),
                        egui::Sense::hover(),
                    );
                    let mut format = formats.plain.clone();
                    format.color = palette.ink_muted;
                    let shown = if marker == BULLET {
                        "x".to_owned()
                    } else {
                        marker.clone()
                    };
                    let galley = ui.fonts_mut(|fonts| {
                        fonts.layout_job(LayoutJob::single_section(shown, format))
                    });
                    let baseline = galley
                        .rows
                        .first()
                        .and_then(|row| row.row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y))
                        .unwrap_or(theme::READING * 1.1);
                    if marker == BULLET {
                        ui.painter().circle_filled(
                            egui::pos2(
                                rect.left() + 6.0,
                                rect.top() + baseline - theme::READING * 0.3,
                            ),
                            2.3,
                            palette.ink_muted,
                        );
                    } else {
                        let width = galley.size().x;
                        ui.painter().galley(
                            egui::pos2(rect.right() - 8.0 - width, rect.top()),
                            galley,
                            palette.ink_muted,
                        );
                    }
                    ui.vertical(|ui| line(ui, palette, &spans(text, true), &formats));
                });
            }
            Block::Code(lines) => {
                Frame::new()
                    .fill(palette.code_ground)
                    .stroke(Stroke::new(1.0, palette.line))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(12, 9))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ScrollArea::horizontal()
                            .id_salt(("code", index))
                            .show(ui, |ui| {
                                ui.add(
                                    Label::new(
                                        RichText::new(lines.join("\n"))
                                            .font(theme::mono(theme::CODE))
                                            .color(palette.ink),
                                    )
                                    .wrap_mode(TextWrapMode::Extend),
                                );
                            });
                    });
            }
            Block::Tool(notice) => {
                let opens = activity::is_single_file(notice.tool, notice.detail);
                if step(
                    ui,
                    palette,
                    &notice,
                    transcript == Transcript::Verbose,
                    opens,
                ) {
                    let relative = std::path::Path::new(notice.detail);
                    clicked = Some(match files.project {
                        Some(project) if relative.is_relative() => project.join(relative),
                        _ => relative.to_path_buf(),
                    });
                }
            }
            Block::Images(notices) => {
                if let Some(path) = images(ui, palette, &notices, files) {
                    clicked = Some(path);
                }
            }
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_output_into_blocks() {
        let blocks = parse(
            "Intro line\nsecond line\n\n## Plan\n- one\n2. two\n```rust\nlet x = 1;\n```\nread · completed\n```\nstill writing",
        );
        let kinds: Vec<&str> = blocks
            .iter()
            .map(|block| match block {
                Block::Paragraph(lines) if lines.len() == 2 => "paragraph",
                Block::Paragraph(_) => "short paragraph",
                Block::Heading(2, "Plan") => "heading",
                Block::Heading(..) => "other heading",
                Block::Item { marker, .. } if marker == BULLET => "bullet",
                Block::Item { .. } => "numbered",
                Block::Code(lines) if lines == &["let x = 1;"] => "code",
                Block::Code(_) => "open code",
                Block::Tool(_) => "tool",
                Block::Table { .. } => "table",
                Block::Images(_) => "images",
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "paragraph",
                "heading",
                "bullet",
                "numbered",
                "code",
                "tool",
                "open code"
            ]
        );
    }

    #[test]
    fn tables_are_read_with_their_alignment() {
        let blocks = parse(
            "All tests pass:\n\n| Test | Result |\n|---|:--:|\n| Rust build | ✅ |\n| Vault read `a|b` | ✅ 2 accounts |\nAfter the table.",
        );
        let Block::Table {
            header,
            align,
            rows,
        } = &blocks[1]
        else {
            panic!("no table");
        };
        assert_eq!(header, &["Test", "Result"]);
        assert_eq!(align, &[Align::Min, Align::Center]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][0], "Vault read `a|b`");
        assert!(matches!(blocks[2], Block::Paragraph(_)));
        // A lone row of bars without a divider stays text.
        assert!(matches!(parse("| not | a table |")[0], Block::Paragraph(_)));
    }

    #[test]
    fn italics_and_strikes_need_words_inside() {
        let found = spans(
            "Unlock *Fluid Platform* and _Fluid_ but not snake_case_name or 2 * 3 * 4, ~~old~~",
            true,
        );
        let italic: Vec<&str> = found
            .iter()
            .filter(|span| span.italic)
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(italic, ["Fluid Platform", "Fluid"]);
        assert!(
            found
                .iter()
                .any(|span| span.text.contains("snake_case_name or 2 * 3 * 4"))
        );
        assert!(found.iter().any(|span| span.struck && span.text == "old"));
    }

    fn texts(spans: &[Span]) -> Vec<(&str, Style, Option<&str>)> {
        spans
            .iter()
            .map(|span| (span.text.as_str(), span.style, span.link.as_deref()))
            .collect()
    }

    #[test]
    fn unmatched_markers_stay_as_typed() {
        let found = spans("run `cargo test` on **all** of *.rs and 2 ** 3", true);
        let text: String = found.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(text, "run cargo test on all of *.rs and 2 ** 3");
        assert_eq!(found[1].style, Style::Code);
        assert_eq!(found[3].style, Style::Strong);
    }

    #[test]
    fn links_are_found_and_trimmed() {
        assert_eq!(
            texts(&spans(
                "See https://opencode.ai/docs/, or (https://a.dev/x_(y)).",
                true
            )),
            [
                ("See ", Style::Plain, None),
                (
                    "https://opencode.ai/docs/",
                    Style::Plain,
                    Some("https://opencode.ai/docs/")
                ),
                (", or (", Style::Plain, None),
                (
                    "https://a.dev/x_(y)",
                    Style::Plain,
                    Some("https://a.dev/x_(y)")
                ),
                (").", Style::Plain, None),
            ]
        );
        assert_eq!(
            texts(&spans(
                "Read [the docs](https://opencode.ai/docs) now",
                true
            )),
            [
                ("Read ", Style::Plain, None),
                ("the docs", Style::Plain, Some("https://opencode.ai/docs")),
                (" now", Style::Plain, None),
            ]
        );
        // Not a link: no address, or Markdown off.
        assert_eq!(spans("[x](not a url) and xhttps://a.dev", true).len(), 1);
        assert_eq!(
            texts(&spans("**[docs](https://a.dev)**", false)),
            [
                ("**[docs](", Style::Plain, None),
                ("https://a.dev", Style::Plain, Some("https://a.dev")),
                (")**", Style::Plain, None),
            ]
        );
    }

    #[test]
    fn speech_skips_code_tools_and_addresses() {
        let text = "read · completed\n## Done\nFixed **two** bugs, see https://a.dev/x.\n```rust\nlet x = 1;\n```\n- [Notes](https://a.dev/n)";
        assert_eq!(speakable(text), "Done.\nFixed two bugs, see a link.\nNotes");
    }
}
