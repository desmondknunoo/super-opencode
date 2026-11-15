//! What OpenCode is doing, in words: the live line while a task runs, and the rows finished
//! steps leave in the transcript.
//!
//! A finished step is stored in the transcript as one line, `tool · status · detail · time`,
//! for example `read · completed · src/main.rs · 0.4s`. Sessions saved before details were
//! recorded hold `tool · status`, and the Swift app wrote `Tool: tool (status)`.

use std::path::Path;

use serde_json::Value;

/// Statuses a detailed notice may carry. The short form accepts any single word.
const STATUSES: &[&str] = &["completed", "error", "failed", "running", "pending"];
/// The longest detail kept, in characters.
const DETAIL_LIMIT: usize = 160;

/// One finished step, read back from the transcript.
#[derive(Debug, PartialEq, Eq)]
pub struct Notice<'a> {
    pub tool: &'a str,
    pub status: &'a str,
    /// What it worked on: a file, a command, a search. Empty when unknown.
    pub detail: &'a str,
    /// How long it took, such as `0.4s` or `2m 05s`. Empty when unknown.
    pub took: &'a str,
}

impl Notice<'_> {
    pub fn failed(&self) -> bool {
        matches!(self.status, "error" | "failed")
    }

    /// The step in words, without the time: “Read src/main.rs”.
    pub fn sentence(&self) -> String {
        let verb = Kind::of(self.tool).past(self.tool);
        if self.detail.is_empty() {
            verb
        } else {
            format!("{verb} {}", self.detail)
        }
    }
}

fn is_word(text: &str) -> bool {
    !text.is_empty() && !text.contains(char::is_whitespace)
}

fn is_duration(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_digit())
        && text.ends_with(['s', 'm'])
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || " .hms".contains(c))
}

/// Reads a notice line, or `None` for any other line.
pub fn parse(line: &str) -> Option<Notice<'_>> {
    if let Some(rest) = line.strip_prefix("Tool: ") {
        let (tool, status) = rest.rsplit_once(" (")?;
        let status = status.strip_suffix(')')?;
        return (is_word(tool) && is_word(status)).then_some(Notice {
            tool,
            status,
            detail: "",
            took: "",
        });
    }
    let (tool, rest) = line.split_once(" · ")?;
    let (status, rest) = rest.split_once(" · ").unwrap_or((rest, ""));
    if !is_word(tool) || !is_word(status) {
        return None;
    }
    if rest.is_empty() {
        return Some(Notice {
            tool,
            status,
            detail: "",
            took: "",
        });
    }
    if !STATUSES.contains(&status) {
        return None;
    }
    let (detail, took) = match rest.rsplit_once(" · ") {
        Some((detail, took)) if is_duration(took) => (detail, took),
        _ if is_duration(rest) => ("", rest),
        _ => (rest, ""),
    };
    Some(Notice {
        tool,
        status,
        detail,
        took,
    })
}

/// `text` without its step notices, for copying a response.
pub fn without_steps(text: &str) -> String {
    let kept: Vec<&str> = text.lines().filter(|line| parse(line).is_none()).collect();
    kept.join("\n").trim().to_owned()
}

/// Whether a step's detail names a picture the transcript can show.
pub fn is_image_path(detail: &str) -> bool {
    let lower = detail.to_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".tif", ".tiff",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
}

/// Whether a step's detail is one file, so the row can open it.
pub fn is_single_file(tool: &str, detail: &str) -> bool {
    matches!(
        Kind::of(tool),
        Kind::Read | Kind::Write | Kind::Edit | Kind::List
    ) && !detail.is_empty()
        && !detail.contains(" and ")
        && !detail.contains('“')
}

/// A step's time back in milliseconds: `0.4s`, `12s`, `2m 05s`, `1h 02m`.
pub fn duration_ms(text: &str) -> Option<i64> {
    let mut total = 0.0;
    for part in text.split_whitespace() {
        let (number, unit) = part.split_at(part.find(|c: char| c.is_alphabetic())?);
        let value: f64 = number.parse().ok()?;
        total += value
            * match unit {
                "ms" => 1.0,
                "s" => 1000.0,
                "m" => 60_000.0,
                "h" => 3_600_000.0,
                _ => return None,
            };
    }
    Some(total.round() as i64)
}

/// The notice for a message added to a running task, shown as “You added …”.
pub fn added_notice(text: &str) -> String {
    notice("added", "completed", &one_line(text), None)
}

/// The notice line for a finished step. `took` is in milliseconds.
pub fn notice(tool: &str, status: &str, detail: &str, took: Option<i64>) -> String {
    let mut line = format!("{tool} · {status}");
    if !detail.is_empty() {
        line.push_str(" · ");
        line.push_str(detail);
    }
    if let Some(took) = took {
        line.push_str(" · ");
        line.push_str(&duration(took));
    }
    line
}

/// A step's length: `0.4s`, `3.2s`, `12s`, `2m 05s`, `1h 02m`.
pub fn duration(millis: i64) -> String {
    let millis = millis.max(0);
    if millis < 10_000 {
        return format!("{:.1}s", millis as f64 / 1000.0);
    }
    elapsed(millis / 1000)
}

/// Whole seconds as the live line counts them: `0s`, `12s`, `2m 05s`, `1h 02m`.
pub fn elapsed(seconds: i64) -> String {
    let seconds = seconds.max(0);
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Read,
    Write,
    Edit,
    Run,
    Search,
    Find,
    List,
    Fetch,
    WebSearch,
    Agent,
    Plan,
    Skill,
    Ask,
    Browser,
    Thought,
    Added,
    Other,
}

impl Kind {
    fn of(tool: &str) -> Self {
        match tool {
            "read" => Self::Read,
            "write" => Self::Write,
            "edit" | "multiedit" | "apply_patch" | "patch" => Self::Edit,
            "shell" | "bash" => Self::Run,
            "grep" => Self::Search,
            "glob" => Self::Find,
            "list" | "ls" => Self::List,
            "webfetch" | "fetch" => Self::Fetch,
            "websearch" | "codesearch" => Self::WebSearch,
            "task" | "subagent" => Self::Agent,
            "todowrite" | "todoread" | "todo" => Self::Plan,
            "skill" => Self::Skill,
            "question" => Self::Ask,
            "browser" => Self::Browser,
            "reasoning" => Self::Thought,
            "added" => Self::Added,
            _ => Self::Other,
        }
    }

    fn past(self, tool: &str) -> String {
        match self {
            Self::Read => "Read",
            Self::Write => "Wrote",
            Self::Edit => "Edited",
            Self::Run => "Ran",
            Self::Search => "Searched for",
            Self::Find => "Looked for",
            Self::List => "Listed",
            Self::Fetch => "Fetched",
            Self::WebSearch => "Searched the web for",
            Self::Agent => "Ran a subagent:",
            Self::Plan => "Updated the plan",
            Self::Skill => "Loaded the skill",
            Self::Ask => "Asked a question",
            Self::Browser => "Used the browser",
            Self::Thought => "Thought",
            Self::Added => "You added",
            Self::Other => return format!("Used {tool}"),
        }
        .to_owned()
    }

    fn present(self, tool: &str) -> String {
        match self {
            Self::Read => "Reading",
            Self::Write => "Writing",
            Self::Edit => "Editing",
            Self::Run => "Running",
            Self::Search => "Searching for",
            Self::Find => "Looking for",
            Self::List => "Listing",
            Self::Fetch => "Fetching",
            Self::WebSearch => "Searching the web for",
            Self::Agent => "Running a subagent:",
            Self::Plan => "Updating the plan",
            Self::Skill => "Loading the skill",
            Self::Ask => "Asking a question",
            Self::Browser => "Using the browser",
            Self::Thought => "Thinking",
            Self::Added => "Adding your message",
            Self::Other => return format!("Using {tool}"),
        }
        .to_owned()
    }

    /// While the model is still writing the tool's input, before it is known.
    fn preparing(self, tool: &str) -> String {
        match self {
            Self::Run => "Writing a command".to_owned(),
            Self::Edit => "Writing an edit".to_owned(),
            Self::Write => "Writing a file".to_owned(),
            Self::Read => "Choosing a file to read".to_owned(),
            Self::Search | Self::Find => "Writing a search".to_owned(),
            Self::Plan => "Updating the plan".to_owned(),
            Self::Agent => "Briefing a subagent".to_owned(),
            _ => format!("Preparing {tool}"),
        }
    }
}

/// What the live line says while `tool` runs on `detail`.
pub fn doing(tool: &str, detail: &str) -> String {
    let verb = Kind::of(tool).present(tool);
    if detail.is_empty() {
        verb
    } else {
        format!("{verb} {detail}")
    }
}

/// What the live line says while the model writes `tool`'s input.
pub fn preparing(tool: &str) -> String {
    Kind::of(tool).preparing(tool)
}

/// One line of at most [`DETAIL_LIMIT`] characters, with runs of white space made single.
fn one_line(text: &str) -> String {
    let mut words = text.split_whitespace();
    let mut line = words.next().unwrap_or_default().to_owned();
    for word in words {
        line.push(' ');
        line.push_str(word);
    }
    if line.chars().count() > DETAIL_LIMIT {
        line = line.chars().take(DETAIL_LIMIT - 1).collect::<String>() + "…";
    }
    // The separator would split the notice in the wrong place.
    line.replace(" · ", " ⋅ ")
}

/// `path` relative to the project when it is inside it.
fn shown_path(path: &str, project: Option<&Path>) -> String {
    let relative = project
        .and_then(|project| Path::new(path).strip_prefix(project).ok())
        .map(|inside| inside.to_string_lossy().into_owned())
        .filter(|inside| !inside.is_empty());
    relative.unwrap_or_else(|| path.to_owned())
}

/// The files an `apply_patch` input changes.
fn patched_files(patch: &str) -> Vec<&str> {
    patch
        .lines()
        .filter_map(|line| {
            ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
                .iter()
                .find_map(|marker| line.strip_prefix(marker))
        })
        .map(str::trim)
        .collect()
}

/// What a tool works on, from its input: the file, the command, the search.
pub fn detail(tool: &str, input: &Value, project: Option<&Path>) -> String {
    let text = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| input.get(*key).and_then(Value::as_str))
            .filter(|value| !value.trim().is_empty())
    };
    let path = || text(&["path", "filePath", "file_path", "file"]);
    let detail = match Kind::of(tool) {
        Kind::Read | Kind::Write | Kind::List => path()
            .map(|path| shown_path(path, project))
            .unwrap_or_default(),
        Kind::Edit => match text(&["patchText", "patch"]) {
            Some(patch) => {
                let files = patched_files(patch);
                match files.as_slice() {
                    [] => String::new(),
                    [one] => shown_path(one, project),
                    [first, rest @ ..] => {
                        format!("{} and {} more", shown_path(first, project), rest.len())
                    }
                }
            }
            None => path()
                .map(|path| shown_path(path, project))
                .unwrap_or_default(),
        },
        Kind::Run => text(&["command", "cmd"]).unwrap_or_default().to_owned(),
        Kind::Search => match text(&["pattern", "query"]) {
            Some(pattern) => {
                let mut found = format!("“{pattern}”");
                if let Some(within) = text(&["include", "glob"]).or_else(path) {
                    found.push_str(&format!(" in {}", shown_path(within, project)));
                }
                found
            }
            None => String::new(),
        },
        Kind::Find => text(&["pattern", "glob"]).unwrap_or_default().to_owned(),
        Kind::Fetch => text(&["url"]).unwrap_or_default().to_owned(),
        Kind::WebSearch => text(&["query"])
            .map(|query| format!("“{query}”"))
            .unwrap_or_default(),
        Kind::Agent => text(&["description", "prompt"])
            .unwrap_or_default()
            .to_owned(),
        Kind::Skill => text(&["name", "skill"]).unwrap_or_default().to_owned(),
        Kind::Plan | Kind::Ask | Kind::Browser | Kind::Thought | Kind::Added | Kind::Other => {
            String::new()
        }
    };
    one_line(&detail)
}

/// The notice for one `tool_use` part from `opencode run --format json`.
pub fn tool_notice(part: &Value, project: Option<&Path>) -> String {
    let tool = part["tool"].as_str().unwrap_or("tool");
    let state = &part["state"];
    let status = state["status"].as_str().unwrap_or("completed");
    let took = took(&state["time"]);
    notice(tool, status, &detail(tool, &state["input"], project), took)
}

/// The notice for a finished `reasoning` part.
pub fn reasoning_notice(part: &Value) -> String {
    notice("reasoning", "completed", "", took(&part["time"]))
}

fn took(time: &Value) -> Option<i64> {
    let start = time["start"].as_i64()?;
    let end = time["end"].as_i64()?;
    Some(end - start)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_every_notice_format() {
        let short = parse("read · completed").unwrap();
        assert_eq!(
            (short.tool, short.status, short.detail),
            ("read", "completed", "")
        );
        let swift = parse("Tool: bash (running)").unwrap();
        assert_eq!((swift.tool, swift.status), ("bash", "running"));
        let full = parse("shell · completed · cargo test · 12s").unwrap();
        assert_eq!((full.detail, full.took), ("cargo test", "12s"));
        let thought = parse("reasoning · completed · 0.7s").unwrap();
        assert_eq!((thought.detail, thought.took), ("", "0.7s"));
        let untimed = parse("read · completed · src/main.rs").unwrap();
        assert_eq!((untimed.detail, untimed.took), ("src/main.rs", ""));
        assert!(parse("A sentence · with a dot").is_none());
        assert!(parse("GLM · High · Plan mode").is_none());
        assert!(parse("Tool: unfinished").is_none());
    }

    #[test]
    fn notices_survive_a_round_trip() {
        let line = notice("grep", "completed", "“total” in src", Some(2400));
        assert_eq!(line, "grep · completed · “total” in src · 2.4s");
        let read = parse(&line).unwrap();
        assert_eq!(read.sentence(), "Searched for “total” in src");
        assert_eq!(read.took, "2.4s");
    }

    #[test]
    fn copies_leave_out_steps() {
        let output =
            "Looking.\nread · completed · a.rs · 0.1s\nreasoning · completed · 1.0s\nDone.";
        assert_eq!(without_steps(output), "Looking.\nDone.");
    }

    #[test]
    fn step_times_read_back() {
        assert_eq!(duration_ms("0.5s"), Some(500));
        assert_eq!(duration_ms("2m 05s"), Some(125_000));
        assert_eq!(duration_ms("1h 02m"), Some(3_720_000));
        assert!(is_image_path(
            "ui/ChatGPT Image Sep 27, 2026, 01_10_35 PM-6.png"
        ));
        assert!(is_single_file("read", "src/main.rs"));
        assert!(!is_single_file("apply_patch", "a.rs and 2 more"));
        assert!(!is_single_file("grep", "“total” in src"));
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(420), "0.4s");
        assert_eq!(duration(3_250), "3.2s");
        assert_eq!(duration(12_000), "12s");
        assert_eq!(duration(125_000), "2m 05s");
        assert_eq!(duration(3_720_000), "1h 02m");
        assert!(is_duration("2m 05s") && is_duration("0.4s") && !is_duration("src"));
    }

    #[test]
    fn details_come_from_the_tool_input() {
        let project = Path::new("/work/app");
        let part = json!({"tool": "read", "state": {"status": "completed",
            "input": {"path": "/work/app/src/billing.py"},
            "time": {"start": 1000, "end": 1406}}});
        assert_eq!(
            tool_notice(&part, Some(project)),
            "read · completed · src/billing.py · 0.4s"
        );
        assert_eq!(
            detail("shell", &json!({"command": "cargo test\n  --locked"}), None),
            "cargo test --locked"
        );
        assert_eq!(
            detail(
                "grep",
                &json!({"pattern": "total", "include": "*.rs"}),
                None
            ),
            "“total” in *.rs"
        );
        let patch = "*** Begin Patch\n*** Update File: /work/app/a.rs\n*** Add File: b.rs\n";
        assert_eq!(
            detail("apply_patch", &json!({"patchText": patch}), Some(project)),
            "a.rs and 1 more"
        );
        assert_eq!(doing("shell", "ls"), "Running ls");
        assert_eq!(preparing("edit"), "Writing an edit");
        assert_eq!(
            parse("frobnicate · completed").unwrap().sentence(),
            "Used frobnicate"
        );
    }

    #[test]
    fn long_details_are_cut_and_never_split_the_notice() {
        let long = "x".repeat(400);
        assert_eq!(
            detail("shell", &json!({"command": long}), None)
                .chars()
                .count(),
            DETAIL_LIMIT
        );
        let line = notice(
            "shell",
            "completed",
            &detail("shell", &json!({"command": "echo a · b"}), None),
            Some(10),
        );
        let read = parse(&line).unwrap();
        assert_eq!((read.detail, read.took), ("echo a ⋅ b", "0.0s"));
    }
}
