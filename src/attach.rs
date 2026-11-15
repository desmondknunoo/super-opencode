//! Files, folders, and recordings attached to a prompt.
//!
//! OpenCode attaches text files, images, and video with `--file`, up to 10 MiB each. Everything
//! else is turned into text on this computer first: documents are read, audio is transcribed,
//! and a folder outside the project is gathered into one text file. A folder inside the project
//! is named in the message instead, since OpenCode can already read it.

use std::{
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::models;

/// OpenCode refuses attachments larger than this.
pub const LIMIT: u64 = 10 * 1024 * 1024;
/// A gathered folder stops at this much text, so one folder cannot fill the model's context.
const FOLDER_BUDGET: usize = 4 * 1024 * 1024;
const FOLDER_FILES: usize = 300;
const FOLDER_FILE_LIMIT: u64 = 1024 * 1024;
/// Folders a project tool generates or downloads. They are left out of a gathered folder.
const GENERATED: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "__pycache__",
    "venv",
    "coverage",
];

const IMAGES: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "avif",
];
const VIDEO: &[&str] = &["mp4", "m4v", "mov", "mkv", "webm", "avi"];
const AUDIO: &[&str] = &[
    "wav", "mp3", "m4a", "aac", "flac", "ogg", "oga", "aif", "aiff", "caf",
];
const DOCUMENTS: &[&str] = &[
    "pdf", "docx", "doc", "rtf", "odt", "pptx", "odp", "xlsx", "ods", "epub",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Image,
    Video,
    Document,
    Audio,
    Folder,
}

impl Kind {
    pub fn noun(self) -> &'static str {
        match self {
            Self::Text => "text file",
            Self::Image => "image",
            Self::Video => "video",
            Self::Document => "document",
            Self::Audio => "audio file",
            Self::Folder => "folder",
        }
    }
}

/// What a prepared attachment adds to the message.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Debug)]
pub struct Sent {
    /// The name the user knows it by.
    pub name: String,
    /// What was done with it, such as "text from 12 pages" or "transcript, 1:24".
    pub note: String,
    /// The file handed to OpenCode with `--file`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// A folder inside the project, named in the message rather than attached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// The file or folder the user attached, for its preview and for opening it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
}

impl Sent {
    /// The original file or folder. Sessions saved before this was recorded only kept the
    /// file sent to OpenCode, which is the original for images, video, and text.
    pub fn original(&self) -> Option<&Path> {
        self.source.as_deref().or(self.file.as_deref())
    }
}

/// Formats this app can draw a preview of.
const PREVIEWABLE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];

pub fn can_preview(path: &Path) -> bool {
    PREVIEWABLE.contains(&extension(path).as_str())
}

/// "PDF", "PNG", or "Folder", for an attachment's card.
pub fn type_label(path: &Path, kind: Kind) -> String {
    match (kind, extension(path)) {
        (Kind::Folder, _) => "Folder".into(),
        (_, ext) if ext.is_empty() => kind.noun().into(),
        (_, ext) => ext.to_uppercase(),
    }
}

/// Why an attachment could not be prepared.
#[derive(Clone, Debug, PartialEq)]
pub enum Problem {
    /// Audio needs the speech model, which has not been downloaded.
    NeedsSpeechModel,
    Failed(String),
}

impl From<String> for Problem {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Whether `bytes` read as text: no NUL bytes and valid UTF-8, allowing a character cut off at
/// the end of the sample.
fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(error) => error.error_len().is_none() && bytes.len() - error.valid_up_to() < 4,
    }
}

fn sniff(path: &Path) -> std::io::Result<bool> {
    let mut sample = Vec::with_capacity(8192);
    fs::File::open(path)?.take(8192).read_to_end(&mut sample)?;
    Ok(looks_like_text(&sample))
}

pub fn classify(path: &Path) -> Result<Kind, String> {
    let metadata = fs::metadata(path).map_err(|error| format!("Couldn’t open it: {error}"))?;
    if metadata.is_dir() {
        return Ok(Kind::Folder);
    }
    let ext = extension(path);
    let kind = if IMAGES.contains(&ext.as_str()) {
        Kind::Image
    } else if VIDEO.contains(&ext.as_str()) {
        Kind::Video
    } else if AUDIO.contains(&ext.as_str()) {
        Kind::Audio
    } else if DOCUMENTS.contains(&ext.as_str()) {
        Kind::Document
    } else if sniff(path).map_err(|error| format!("Couldn’t read it: {error}"))? {
        Kind::Text
    } else {
        return Err(
            "This file is not text, a document, an image, audio, or video, so no model here can read it."
                .into(),
        );
    };
    Ok(kind)
}

/// Why the selected model cannot take this attachment, and the first model that can.
pub fn refusal(kind: Kind, path: &Path, model: &str) -> Option<(String, Option<usize>)> {
    let ext = extension(path);
    let accepts = |model: &str| {
        let Some(model) = models::find(model) else {
            return !matches!(kind, Kind::Image | Kind::Video);
        };
        match kind {
            Kind::Image => model.images.contains(&ext.as_str()),
            Kind::Video => model.video.contains(&ext.as_str()),
            _ => true,
        }
    };
    if accepts(model) {
        return None;
    }
    let alternative = models::all().iter().position(|entry| accepts(&entry.id));
    let upper = ext.to_uppercase();
    let message = match alternative {
        Some(index) => format!(
            "{} can’t read {upper} {}s. {} can.",
            models::name_of(model),
            kind.noun(),
            models::at(index).name
        ),
        None if kind == Kind::Image => {
            format!("No model here reads {upper} images. Export it as PNG or JPEG.")
        }
        None => format!("No model here reads {upper} video."),
    };
    Some((message, alternative))
}

/// Gives the prepared attachment a size or length, for its label.
fn size_note(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1_048_575 => format!("{:.0} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

fn word_note(text: &str) -> String {
    let words = text.split_whitespace().count();
    match words {
        0..=999 => format!("{words} words"),
        _ => format!("{:.1}k words", words as f64 / 1000.0),
    }
}

fn within_limit(path: &Path) -> Result<u64, String> {
    let size = fs::metadata(path)
        .map_err(|error| format!("Couldn’t read it: {error}"))?
        .len();
    if size > LIMIT {
        return Err(format!(
            "It is {}. OpenCode accepts attachments up to 10 MB.",
            size_note(size)
        ));
    }
    Ok(size)
}

/// Writes text made from `source` into `workdir`, named after the original.
fn write_text(workdir: &Path, source: &Path, text: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(workdir).map_err(|error| format!("Couldn’t save the text: {error}"))?;
    let path = workdir.join(format!("{}.txt", display_name(source)));
    fs::write(&path, text).map_err(|error| format!("Couldn’t save the text: {error}"))?;
    if text.len() as u64 > LIMIT {
        return Err("The text made from it is over OpenCode’s 10 MB limit.".into());
    }
    Ok(path)
}

/// Prepares one attachment. Runs on a worker thread: documents, audio, and folders can take
/// a few seconds.
pub fn prepare(
    kind: Kind,
    source: &Path,
    project: Option<&Path>,
    workdir: &Path,
    speech_model: Option<&Path>,
    language: Option<&str>,
) -> Result<Sent, Problem> {
    let name = display_name(source);
    let sent = |note: String, file: PathBuf| Sent {
        name: name.clone(),
        note,
        file: Some(file),
        reference: None,
        source: Some(source.to_path_buf()),
    };
    match kind {
        Kind::Text => {
            let size = within_limit(source)?;
            Ok(sent(size_note(size), source.to_path_buf()))
        }
        Kind::Image | Kind::Video => {
            let size = within_limit(source)?;
            Ok(sent(
                format!("{}, {}", kind.noun(), size_note(size)),
                source.to_path_buf(),
            ))
        }
        Kind::Document => {
            let text = document_text(source)?;
            let file = write_text(workdir, source, &text)?;
            Ok(sent(format!("text, {}", word_note(&text)), file))
        }
        Kind::Audio => {
            let model = speech_model.ok_or(Problem::NeedsSpeechModel)?;
            let (samples, seconds) = crate::speech::decode_file(source)?;
            let text = crate::speech::transcribe(model, &samples, language)?;
            if text.trim().is_empty() {
                return Err(Problem::Failed("No speech was found in it.".into()));
            }
            let file = write_text(workdir, source, &text)?;
            let length = format!("{}:{:02}", seconds / 60, seconds % 60);
            Ok(sent(format!("transcript of {length}"), file))
        }
        Kind::Folder => {
            let folder = source
                .canonicalize()
                .map_err(|error| format!("Couldn’t open it: {error}"))?;
            if let Some(project) = project.and_then(|path| path.canonicalize().ok())
                && let Ok(inside) = folder.strip_prefix(&project)
            {
                let reference = if inside.as_os_str().is_empty() {
                    ".".to_owned()
                } else {
                    inside.display().to_string()
                };
                return Ok(Sent {
                    name,
                    note: "in this project".into(),
                    file: None,
                    reference: Some(reference),
                    source: Some(folder),
                });
            }
            let (text, files) = gather(&folder)?;
            let file = write_text(workdir, source, &text)?;
            let plural = if files == 1 { "" } else { "s" };
            Ok(sent(format!("{files} text file{plural}"), file))
        }
    }
}

/// The lines added to the message so the model knows what it was given and how.
pub fn describe(attachments: &[Sent]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let mut text = String::from("\n\nAttached:");
    for item in attachments {
        match &item.reference {
            Some(path) => text.push_str(&format!(
                "\n- Folder `{path}` in this project. Read it with your tools."
            )),
            None => text.push_str(&format!("\n- {} ({})", item.name, item.note)),
        }
    }
    text
}

/// Every text file under `folder`, each under a header with its path.
fn gather(folder: &Path) -> Result<(String, usize), String> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    let mut skipped = 0usize;
    while let Some(directory) = pending.pop() {
        let mut entries: Vec<_> = fs::read_dir(&directory)
            .map_err(|error| format!("Couldn’t read {}: {error}", directory.display()))?
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries.into_iter().rev() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || GENERATED.contains(&name.as_str()) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    let mut text = String::new();
    let mut included = 0usize;
    let mut over_budget = 0usize;
    for path in &found {
        let small = fs::metadata(path).is_ok_and(|meta| meta.len() <= FOLDER_FILE_LIMIT);
        let readable = small
            && !IMAGES
                .iter()
                .chain(VIDEO)
                .chain(AUDIO)
                .chain(DOCUMENTS)
                .any(|ext| *ext == extension(path))
            && sniff(path).unwrap_or(false);
        if !readable {
            skipped += 1;
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            skipped += 1;
            continue;
        };
        if included >= FOLDER_FILES || text.len() + content.len() > FOLDER_BUDGET {
            over_budget += 1;
            continue;
        }
        let relative = path.strip_prefix(folder).unwrap_or(path);
        text.push_str(&format!("=== {} ===\n{content}\n\n", relative.display()));
        included += 1;
    }
    if included == 0 {
        return Err("It has no text files to send.".into());
    }
    let mut header = format!("Folder {} ({included} text files", display_name(folder));
    if skipped > 0 {
        header.push_str(&format!("; {skipped} binary or large files left out"));
    }
    if over_budget > 0 {
        header.push_str(&format!(
            "; {over_budget} more not included to stay under 4 MB"
        ));
    }
    header.push_str(")\n\n");
    Ok((header + &text, included))
}

// ---------------------------------------------------------------------------------------------
// Documents

fn document_text(path: &Path) -> Result<String, String> {
    let ext = extension(path);
    let bytes = fs::read(path).map_err(|error| format!("Couldn’t read it: {error}"))?;
    let text = match ext.as_str() {
        "pdf" => pdf(&bytes)?,
        "docx" => word(&bytes)?,
        "pptx" => slides(&bytes)?,
        "xlsx" => workbook(&bytes)?,
        "odt" | "odp" | "ods" => open_document(&bytes)?,
        "epub" => epub(&bytes)?,
        "rtf" => rtf(&String::from_utf8_lossy(&bytes)),
        "doc" => legacy_word(path)?,
        _ => return Err(format!("{} files can’t be read here.", ext.to_uppercase())),
    };
    let text = tidy(&text);
    if text.trim().is_empty() {
        return Err(if ext == "pdf" {
            "This PDF has no text layer; its pages may be scanned images. Export the pages as PNG and attach those to a model that reads images.".into()
        } else {
            "It contains no text.".into()
        });
    }
    Ok(text)
}

fn pdf(bytes: &[u8]) -> Result<String, String> {
    // The PDF reader panics on some malformed files; treat that as unreadable.
    let result = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes));
    match result {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(error)) => Err(format!("Couldn’t read the PDF: {error}")),
        Err(_) => Err("Couldn’t read the PDF. It may be damaged or encrypted.".into()),
    }
}

type Archive<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

fn archive(bytes: &[u8]) -> Result<Archive<'_>, String> {
    zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "It is not a valid document file.".to_owned())
}

fn entry(archive: &mut Archive<'_>, name: &str) -> Result<String, String> {
    let mut file = archive
        .by_name(name)
        .map_err(|_| format!("The document has no {name}."))?;
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|error| format!("Couldn’t read {name}: {error}"))?;
    Ok(text)
}

/// Entries named `prefix<N>.xml`, in numeric order.
fn numbered(archive: &Archive<'_>, prefix: &str) -> Vec<String> {
    let mut names: Vec<(u32, String)> = archive
        .file_names()
        .filter_map(Result::ok)
        .filter_map(|name| {
            let number = name
                .strip_prefix(prefix)?
                .strip_suffix(".xml")?
                .parse()
                .ok()?;
            Some((number, name.into_owned()))
        })
        .collect();
    names.sort();
    names.into_iter().map(|(_, name)| name).collect()
}

/// How to turn one XML vocabulary into text.
struct Rules<'a> {
    /// Only text inside these elements is kept. `None` keeps all text.
    keep: Option<&'a [&'a str]>,
    /// The end of these elements starts a new line.
    blocks: &'a [&'a str],
    /// These empty elements are line breaks.
    breaks: &'a [&'a str],
    /// These empty elements are tabs, and the end of these cells too.
    tabs: &'a [&'a str],
    /// Text inside these elements is dropped.
    skip: &'a [&'a str],
    /// Runs of whitespace in text become one space, as in HTML.
    collapse: bool,
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

fn unescape(text: &str, out: &mut String) {
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        match rest[1..].find(';').filter(|&end| end <= 10) {
            Some(end) if entity(&rest[1..=end]).is_some() => {
                out.extend(entity(&rest[1..=end]));
                rest = &rest[end + 2..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
}

fn xml_text(xml: &str, rules: &Rules) -> String {
    let mut out = String::new();
    let mut keep_depth = 0usize;
    let mut skip_depth = 0usize;
    let mut rest = xml;
    let push_text = |out: &mut String, text: &str, keep_depth: usize, skip_depth: usize| {
        if skip_depth > 0 || (rules.keep.is_some() && keep_depth == 0) {
            return;
        }
        if rules.collapse {
            let mut decoded = String::new();
            unescape(text, &mut decoded);
            for (index, word) in decoded.split_whitespace().enumerate() {
                let starts_line = out.is_empty() || out.ends_with(['\n', '\t', ' ']);
                if index > 0 || (decoded.starts_with(char::is_whitespace) && !starts_line) {
                    out.push(' ');
                }
                out.push_str(word);
            }
            if decoded.ends_with(char::is_whitespace) && !decoded.trim().is_empty() {
                out.push(' ');
            }
        } else {
            unescape(text, out);
        }
    };
    while let Some(lt) = rest.find('<') {
        push_text(&mut out, &rest[..lt], keep_depth, skip_depth);
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        if let Some(after) = rest.strip_prefix("<![CDATA[") {
            let end = after.find("]]>").unwrap_or(after.len());
            if skip_depth == 0 && (rules.keep.is_none() || keep_depth > 0) {
                out.push_str(&after[..end]);
            }
            rest = after.get(end + 3..).unwrap_or("");
            continue;
        }
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        let closing = tag.starts_with('/');
        let empty = tag.ends_with('/');
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or_default();
        let name = if rules.collapse {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        };
        let is = |list: &[&str]| list.contains(&name.as_str());
        if is(rules.skip) {
            if closing {
                skip_depth = skip_depth.saturating_sub(1);
            } else if !empty {
                skip_depth += 1;
            }
            continue;
        }
        if rules.keep.is_some_and(|keep| keep.contains(&name.as_str())) {
            if closing {
                keep_depth = keep_depth.saturating_sub(1);
            } else if !empty {
                keep_depth += 1;
            }
        }
        if skip_depth > 0 {
            continue;
        }
        if is(rules.breaks) && !closing {
            out.push('\n');
        } else if is(rules.tabs) && (empty || closing) {
            out.push('\t');
        } else if is(rules.blocks) && (closing || (rules.collapse && !empty)) {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
        } else if name == "text:s" {
            // ODF spaces: <text:s text:c="3"/>.
            let count = tag
                .split("text:c=\"")
                .nth(1)
                .and_then(|value| value.split('"').next())
                .and_then(|value| value.parse().ok())
                .unwrap_or(1usize);
            out.extend(std::iter::repeat_n(' ', count.min(64)));
        }
    }
    push_text(&mut out, rest, keep_depth, skip_depth);
    out
}

/// Trims trailing spaces and leaves at most one blank line between paragraphs.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blank += 1;
            if blank > 1 || out.is_empty() {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_owned()
}

fn word(bytes: &[u8]) -> Result<String, String> {
    let mut archive = archive(bytes)?;
    let xml = entry(&mut archive, "word/document.xml")?;
    Ok(xml_text(
        &xml,
        &Rules {
            keep: Some(&["w:t"]),
            blocks: &["w:p"],
            breaks: &["w:br", "w:cr"],
            tabs: &["w:tab"],
            skip: &["w:instrText", "w:delText"],
            collapse: false,
        },
    ))
}

fn slides(bytes: &[u8]) -> Result<String, String> {
    let mut archive = archive(bytes)?;
    let mut text = String::new();
    for (index, name) in numbered(&archive, "ppt/slides/slide").iter().enumerate() {
        let xml = entry(&mut archive, name)?;
        let body = xml_text(
            &xml,
            &Rules {
                keep: Some(&["a:t"]),
                blocks: &["a:p"],
                breaks: &["a:br"],
                tabs: &[],
                skip: &[],
                collapse: false,
            },
        );
        text.push_str(&format!("Slide {}\n{}\n\n", index + 1, body.trim()));
    }
    Ok(text)
}

/// The value of attribute `name` in a start tag, unescaped.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let end = tag[start..].find('"')? + start;
    let mut value = String::new();
    unescape(&tag[start..end], &mut value);
    Some(value)
}

/// Each sheet as tab-separated rows. Cells hold shared strings, inline strings, or values.
fn workbook(bytes: &[u8]) -> Result<String, String> {
    let mut archive = archive(bytes)?;
    let shared: Vec<String> = match entry(&mut archive, "xl/sharedStrings.xml") {
        Ok(xml) => xml
            .split("<si>")
            .skip(1)
            .chain(xml.split("<si ").skip(1))
            .map(|item| {
                let item = item.split("</si>").next().unwrap_or_default();
                xml_text(
                    item,
                    &Rules {
                        keep: Some(&["t"]),
                        blocks: &[],
                        breaks: &[],
                        tabs: &[],
                        skip: &["rPh"],
                        collapse: false,
                    },
                )
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    let mut text = String::new();
    for (index, name) in numbered(&archive, "xl/worksheets/sheet").iter().enumerate() {
        let xml = entry(&mut archive, name)?;
        text.push_str(&format!("Sheet {}\n", index + 1));
        for row in xml.split("<row").skip(1) {
            let row = row.split("</row>").next().unwrap_or_default();
            let mut cells = Vec::new();
            for cell in row.split("<c ").skip(1) {
                let (tag, body) = cell.split_once('>').unwrap_or((cell, ""));
                if tag.ends_with('/') {
                    continue;
                }
                let body = body.split("</c>").next().unwrap_or_default();
                let value = |element: &str| {
                    body.split(&format!("<{element}>"))
                        .nth(1)
                        .and_then(|rest| rest.split(&format!("</{element}>")).next())
                        .map(|raw| {
                            let mut out = String::new();
                            unescape(raw, &mut out);
                            out
                        })
                };
                let content = match attribute(tag, "t").as_deref() {
                    Some("s") => value("v")
                        .and_then(|index| index.trim().parse::<usize>().ok())
                        .and_then(|index| shared.get(index).cloned()),
                    Some("inlineStr") => Some(xml_text(
                        body,
                        &Rules {
                            keep: Some(&["t"]),
                            blocks: &[],
                            breaks: &[],
                            tabs: &[],
                            skip: &[],
                            collapse: false,
                        },
                    )),
                    _ => value("v"),
                };
                cells.push(content.unwrap_or_default().replace(['\t', '\n'], " "));
            }
            while cells.last().is_some_and(String::is_empty) {
                cells.pop();
            }
            if !cells.is_empty() {
                text.push_str(&cells.join("\t"));
                text.push('\n');
            }
        }
        text.push('\n');
    }
    Ok(text)
}

fn open_document(bytes: &[u8]) -> Result<String, String> {
    let mut archive = archive(bytes)?;
    let xml = entry(&mut archive, "content.xml")?;
    // Inside a spreadsheet cell, a paragraph end is not a new row.
    let spreadsheet = xml.contains("<office:spreadsheet");
    Ok(xml_text(
        &xml,
        &Rules {
            keep: None,
            blocks: if spreadsheet {
                &["table:table-row", "table:table"]
            } else {
                &["text:p", "text:h", "text:list-item", "table:table-row"]
            },
            breaks: &["text:line-break"],
            tabs: &["text:tab", "table:table-cell"],
            skip: &["office:annotation", "text:note-citation"],
            collapse: false,
        },
    ))
}

const HTML_RULES: Rules<'static> = Rules {
    keep: None,
    blocks: &[
        "p",
        "div",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "li",
        "tr",
        "section",
        "blockquote",
        "pre",
        "title",
    ],
    breaks: &["br"],
    tabs: &["td", "th"],
    skip: &["script", "style", "head"],
    collapse: true,
};

/// The book's chapters in reading order, from the package's spine.
fn epub(bytes: &[u8]) -> Result<String, String> {
    let mut archive = archive(bytes)?;
    let container = entry(&mut archive, "META-INF/container.xml")?;
    let package = container
        .split("full-path=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .ok_or("The book has no package file.")?
        .to_owned();
    let opf = entry(&mut archive, &package)?;
    let base = package.rsplit_once('/').map_or("", |(folder, _)| folder);
    let items: Vec<(String, String)> = opf
        .split("<item ")
        .skip(1)
        .filter_map(|tag| {
            let tag = format!(" {}", tag.split('>').next()?);
            Some((attribute(&tag, "id")?, attribute(&tag, "href")?))
        })
        .collect();
    let mut text = String::new();
    for reference in opf.split("<itemref ").skip(1) {
        let tag = format!(" {}", reference.split('>').next().unwrap_or_default());
        let Some(id) = attribute(&tag, "idref") else {
            continue;
        };
        let Some((_, href)) = items.iter().find(|(item, _)| *item == id) else {
            continue;
        };
        let path = if base.is_empty() {
            href.clone()
        } else {
            format!("{base}/{href}")
        };
        if let Ok(chapter) = entry(&mut archive, &path) {
            text.push_str(&xml_text(&chapter, &HTML_RULES));
            text.push_str("\n\n");
        }
    }
    Ok(text)
}

/// Plain text from RTF: groups that hold no text are dropped, control words become the
/// characters they stand for, and the rest is the text itself.
fn rtf(source: &str) -> String {
    const DESTINATIONS: &[&str] = &[
        "fonttbl",
        "colortbl",
        "stylesheet",
        "info",
        "pict",
        "header",
        "footer",
        "listtable",
        "listoverridetable",
        "rsidtbl",
        "generator",
        "xmlnstbl",
        "themedata",
        "colorschememapping",
        "latentstyles",
        "datastore",
        "fldinst",
        "object",
    ];
    let mut out = String::new();
    let mut chars = source.chars().peekable();
    // For each open group: whether its text is dropped.
    let mut groups: Vec<bool> = Vec::new();
    let mut dropped = false;
    let mut unicode_skip = 1usize;
    let mut pending_skip = 0usize;
    while let Some(c) = chars.next() {
        match c {
            '{' => groups.push(dropped),
            '}' => dropped = groups.pop().unwrap_or(false),
            '\\' => {
                let Some(&next) = chars.peek() else { break };
                if !next.is_ascii_alphabetic() {
                    chars.next();
                    match next {
                        '*' => dropped = true,
                        '\'' => {
                            let hex: String = chars.by_ref().take(2).collect();
                            if !dropped && let Ok(byte) = u8::from_str_radix(&hex, 16) {
                                if pending_skip > 0 {
                                    pending_skip -= 1;
                                } else {
                                    // Windows-1252 covers most RTF written in Western languages.
                                    out.push(char::from(byte));
                                }
                            }
                        }
                        '~' if !dropped => out.push(' '),
                        // A backslash before a line break is a paragraph break.
                        '\n' | '\r' if !dropped => out.push('\n'),
                        '{' | '}' | '\\' if !dropped => out.push(next),
                        _ => {}
                    }
                    continue;
                }
                let mut word = String::new();
                while let Some(&letter) = chars.peek() {
                    if letter.is_ascii_alphabetic() {
                        word.push(letter);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let mut number = String::new();
                if chars.peek() == Some(&'-') {
                    number.push('-');
                    chars.next();
                }
                while let Some(&digit) = chars.peek() {
                    if digit.is_ascii_digit() {
                        number.push(digit);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
                let value: Option<i32> = number.parse().ok();
                if DESTINATIONS.contains(&word.as_str()) {
                    dropped = true;
                    continue;
                }
                if dropped {
                    continue;
                }
                match word.as_str() {
                    "par" | "line" | "row" | "sect" | "page" => out.push('\n'),
                    "tab" | "cell" => out.push('\t'),
                    "emdash" => out.push('—'),
                    "endash" => out.push('–'),
                    "bullet" => out.push('•'),
                    "lquote" => out.push('‘'),
                    "rquote" => out.push('’'),
                    "ldblquote" => out.push('“'),
                    "rdblquote" => out.push('”'),
                    "uc" => unicode_skip = value.unwrap_or(1).max(0) as usize,
                    "u" => {
                        if let Some(code) = value {
                            let code = if code < 0 { code + 65536 } else { code } as u32;
                            out.extend(char::from_u32(code));
                            pending_skip = unicode_skip;
                        }
                    }
                    _ => {}
                }
            }
            '\r' | '\n' => {}
            _ if dropped => {}
            _ if pending_skip > 0 => pending_skip -= 1,
            _ => out.push(c),
        }
    }
    out
}

/// Word 97–2003 files have no portable format to read. macOS ships a converter for them.
fn legacy_word(path: &Path) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/textutil")
            .args(["-convert", "txt", "-stdout"])
            .arg(path)
            .output()
            .map_err(|error| format!("Couldn’t read the Word file: {error}"))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        Err("Couldn’t read the Word file. Save it as .docx or PDF and attach that.".into())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err("Word 97–2003 (.doc) files can’t be read here. Save it as .docx or PDF and attach that.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zipped(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_word_paragraphs_tabs_and_entities() {
        let xml = r#"<?xml version="1.0"?><w:document><w:body>
<w:p><w:r><w:t>Invoice &amp; total</w:t></w:r><w:r><w:tab/><w:t xml:space="preserve"> 12</w:t></w:r></w:p>
<w:p><w:r><w:instrText>HYPERLINK x</w:instrText><w:t>Second</w:t><w:br/><w:t>line</w:t></w:r></w:p>
</w:body></w:document>"#;
        let text = tidy(&word(&zipped(&[("word/document.xml", xml)])).unwrap());
        assert_eq!(text, "Invoice & total\t 12\nSecond\nline");
    }

    #[test]
    fn reads_slides_in_numeric_order() {
        let slide = |text: &str| format!("<p:sld><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:sld>");
        let bytes = zipped(&[
            ("ppt/slides/slide10.xml", &slide("Ten")),
            ("ppt/slides/slide2.xml", &slide("Two")),
        ]);
        assert_eq!(
            tidy(&slides(&bytes).unwrap()),
            "Slide 1\nTwo\n\nSlide 2\nTen"
        );
    }

    #[test]
    fn reads_spreadsheet_cells_as_tab_separated_rows() {
        let shared =
            r#"<sst><si><t>Name</t></si><si><r><t>Ada </t></r><r><t>L.</t></r></si></sst>"#;
        let sheet = r#"<worksheet><sheetData>
<row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="inlineStr"><is><t>Paid</t></is></c></row>
<row r="2"><c r="A2" t="s"><v>1</v></c><c r="B2"><v>42.5</v></c><c r="C2"/></row>
</sheetData></worksheet>"#;
        let bytes = zipped(&[
            ("xl/sharedStrings.xml", shared),
            ("xl/worksheets/sheet1.xml", sheet),
        ]);
        assert_eq!(
            tidy(&workbook(&bytes).unwrap()),
            "Sheet 1\nName\tPaid\nAda L.\t42.5"
        );
    }

    #[test]
    fn reads_open_document_text() {
        let xml = r#"<office:document-content><office:body><office:text>
<text:h>Title</text:h><text:p>One<text:s text:c="2"/>two<text:tab/>three</text:p>
<office:annotation><text:p>hidden note</text:p></office:annotation>
</office:text></office:body></office:document-content>"#;
        let text = tidy(&open_document(&zipped(&[("content.xml", xml)])).unwrap());
        assert_eq!(text, "Title\nOne  two\tthree");
    }

    #[test]
    fn reads_an_epub_in_spine_order() {
        let container = r#"<container><rootfiles><rootfile full-path="OEBPS/book.opf"/></rootfiles></container>"#;
        let opf = r#"<package><manifest><item id="b" href="b.xhtml"/><item id="a" href="a.xhtml"/></manifest>
<spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#;
        let page = |text: &str| {
            format!(
                "<html><head><title>x</title><style>p{{}}</style></head><body><h1>{text}</h1><p>Body   text &mdash; here</p></body></html>"
            )
        };
        let bytes = zipped(&[
            ("META-INF/container.xml", container),
            ("OEBPS/book.opf", opf),
            ("OEBPS/a.xhtml", &page("First")),
            ("OEBPS/b.xhtml", &page("Second")),
        ]);
        let text = tidy(&epub(&bytes).unwrap());
        assert!(text.starts_with("First\nBody text"), "{text}");
        assert!(text.contains("\n\nSecond\n"));
        assert!(!text.contains("p{}"));
    }

    #[test]
    fn reads_rtf_text_and_drops_tables_of_fonts() {
        let source = r"{\rtf1\ansi{\fonttbl{\f0 Helvetica;}}{\*\generator Writer;}\f0 Caf\'e9 \b bold\b0\par Second\tab col \u8364?5\par}";
        assert_eq!(tidy(&rtf(source)), "Café bold\nSecond\tcol €5");
        let mac = "{\\rtf1 One\\\nTwo}";
        assert_eq!(tidy(&rtf(mac)), "One\nTwo");
    }

    #[test]
    fn pdf_text_is_extracted() {
        // A one-page PDF with the cross-reference offsets computed, as a writer would.
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
            "<< /Length 40 >>\nstream\nBT /F1 18 Tf 20 100 Td (Hello PDF) Tj ET\nendstream",
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        ];
        let mut pdf_bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf_bytes.len());
            pdf_bytes.extend(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf_bytes.len();
        pdf_bytes
            .extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for offset in offsets {
            pdf_bytes.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf_bytes.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        let text = pdf(&pdf_bytes).unwrap();
        assert!(text.contains("Hello PDF"), "{text:?}");
        assert!(pdf(b"not a pdf").is_err());
    }

    #[test]
    fn text_is_recognised_by_content() {
        assert!(looks_like_text("plain ✓".as_bytes()));
        // A character cut off by the sample size still counts as text.
        assert!(looks_like_text(&"✓".as_bytes()[..2]));
        assert!(!looks_like_text(b"\x89PNG\r\n\x1a\n\0\0"));
        assert!(!looks_like_text(&[0xff, 0xfe, 0x41, 0x42]));
    }

    #[test]
    fn models_are_matched_to_what_they_read() {
        use crate::models::{BACKUP, DEFAULT as BUILTIN};
        let png = Path::new("shot.png");
        assert!(refusal(Kind::Image, png, BUILTIN).is_none());
        let (message, alternative) = refusal(Kind::Image, png, BACKUP).unwrap();
        assert_eq!(
            message,
            "North Mini Code can’t read PNG images. GLM 5.3 Flash can."
        );
        assert_eq!(alternative, Some(0));
        let webp = refusal(Kind::Image, Path::new("a.webp"), BUILTIN).unwrap();
        assert_eq!(webp.1, Some(1), "DeepSeek V4.1 Flash reads WebP");
        assert!(
            refusal(Kind::Image, Path::new("a.heic"), BUILTIN)
                .unwrap()
                .1
                .is_none()
        );
        assert!(refusal(Kind::Document, Path::new("a.pdf"), BACKUP).is_none());
        assert!(refusal(Kind::Video, Path::new("a.mov"), BUILTIN).is_none());
    }

    #[test]
    fn folders_are_referenced_inside_the_project_and_gathered_outside() {
        let project = tempfile::tempdir().unwrap();
        let inside = project.path().join("src");
        fs::create_dir_all(inside.join("node_modules")).unwrap();
        fs::write(inside.join("main.rs"), "fn main() {}").unwrap();
        fs::write(inside.join("node_modules").join("x.js"), "skip").unwrap();
        fs::write(inside.join("logo.png"), b"\x89PNG\0").unwrap();
        let work = tempfile::tempdir().unwrap();

        let referenced = prepare(
            Kind::Folder,
            &inside,
            Some(project.path()),
            work.path(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(referenced.reference.as_deref(), Some("src"));
        assert!(referenced.file.is_none());

        let elsewhere = tempfile::tempdir().unwrap();
        let gathered = prepare(
            Kind::Folder,
            &inside,
            Some(elsewhere.path()),
            work.path(),
            None,
            None,
        )
        .unwrap();
        let text = fs::read_to_string(gathered.file.unwrap()).unwrap();
        assert!(text.contains("=== main.rs ===\nfn main() {}"), "{text}");
        assert!(!text.contains("skip"));
        assert!(text.contains("1 binary or large files left out"));
        assert_eq!(gathered.note, "1 text file");

        let message = describe(&[referenced]);
        assert!(message.contains("Folder `src` in this project"));
    }

    /// Reads documents made by real converters. Run with the folder of fixtures:
    /// `SUPER_OPENCODE_FIXTURES=<folder> cargo test real_documents -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_documents() {
        let folder = std::env::var("SUPER_OPENCODE_FIXTURES").expect("fixture folder");
        for name in ["plan.docx", "plan.odt", "plan.rtf", "plan.doc", "plan.pdf"] {
            let path = Path::new(&folder).join(name);
            let text = document_text(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
            println!(
                "--- {name}: {:?}",
                text.chars().take(90).collect::<String>()
            );
            assert!(text.contains("HERON"), "{name}: {text}");
        }
        // AAC in M4A is what Voice Memos, QuickTime, and phones record.
        for name in ["memo-aac.m4a", "memo.wav"] {
            let (samples, seconds) = crate::speech::decode_file(&Path::new(&folder).join(name))
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            println!(
                "--- {name}: {} samples at 16 kHz, {seconds} s",
                samples.len()
            );
            assert!(samples.len() > 16_000);
        }
    }

    #[test]
    fn audio_waits_for_the_speech_model() {
        let work = tempfile::tempdir().unwrap();
        let problem = prepare(
            Kind::Audio,
            Path::new("memo.m4a"),
            None,
            work.path(),
            None,
            None,
        );
        assert_eq!(problem, Err(Problem::NeedsSpeechModel));
    }
}
