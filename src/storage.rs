use std::{
    fs, io,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::NamedTempFile;
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct Turn {
    pub id: Uuid,
    pub prompt: String,
    pub model: String,
    pub output: String,
    /// Files and folders sent with the prompt. Sessions saved before attachments existed
    /// have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::attach::Sent>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: Uuid,
    pub project_path: PathBuf,
    #[serde(rename = "routeID", alias = "routeId")]
    pub route_id: String,
    pub server_session: Option<String>,
    pub turns: Vec<Turn>,
    pub updated: f64,
    /// Tokens in the conversation after the last step: what the next request starts from.
    /// Zero before the first step, and for sessions saved before this was recorded.
    #[serde(default)]
    pub context_tokens: u64,
    /// Steps and tokens this session used.
    #[serde(default)]
    pub spent: Spent,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spent {
    pub steps: u64,
    /// Tokens the model read, cached or not.
    pub read: u64,
    /// Tokens the model wrote, reasoning included.
    pub written: u64,
}

/// The token counts of one finished step, from OpenCode's `step_finish` event.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct StepTokens {
    pub input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl StepTokens {
    pub fn of(part: &Value) -> Self {
        let tokens = &part["tokens"];
        let count = |value: &Value| value.as_u64().unwrap_or(0);
        Self {
            input: count(&tokens["input"]),
            output: count(&tokens["output"]),
            reasoning: count(&tokens["reasoning"]),
            cache_read: count(&tokens["cache"]["read"]),
            cache_write: count(&tokens["cache"]["write"]),
        }
    }

    /// The conversation's size after this step. OpenCode counts cached input apart from
    /// `input`, so the prompt is all three; the reply joins the conversation too. Reasoning
    /// is left out because providers drop it from later requests.
    pub fn context(&self) -> u64 {
        self.input + self.cache_read + self.cache_write + self.output
    }
}

impl Session {
    /// Records a finished step.
    pub fn add_step(&mut self, step: StepTokens) {
        if step.context() > 0 {
            self.context_tokens = step.context();
        }
        self.spent.steps += 1;
        self.spent.read += step.input + step.cache_read + step.cache_write;
        self.spent.written += step.output + step.reasoning;
    }
}

impl Session {
    pub fn title(&self) -> &str {
        self.turns
            .first()
            .map_or("Untitled session", |turn| &turn.prompt)
    }

    pub fn revision_time() -> f64 {
        // Keep compatibility with Date's reference epoch in the original macOS app.
        Utc::now().timestamp_millis() as f64 / 1000.0 - REFERENCE_EPOCH
    }

    /// Seconds since this session was last saved.
    pub fn age(&self) -> f64 {
        (Self::revision_time() - self.updated).max(0.0)
    }
}

/// Seconds between the Unix epoch and 2001-01-01, the epoch the Swift app stored dates in.
const REFERENCE_EPOCH: f64 = 978_307_200.0;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scale {
    Smaller,
    #[default]
    Normal,
    Larger,
    Largest,
}

impl Scale {
    pub fn factor(self) -> f32 {
        match self {
            Self::Smaller => 0.9,
            Self::Normal => 1.0,
            Self::Larger => 1.15,
            Self::Largest => 1.3,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pace {
    Slower,
    #[default]
    Normal,
    Faster,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub executable: Option<PathBuf>,
    pub project: Option<PathBuf>,
    pub fallback: bool,
    pub appearance: Appearance,
    /// Read each response aloud when it finishes.
    pub read_aloud: bool,
    /// The model new sessions start on.
    pub default_model: String,
    /// The model a rate-limited task continues on.
    pub backup_model: String,
    /// Stop a task after this many minutes. Zero means no limit.
    pub time_limit_minutes: u32,
    /// Let OpenCode approve its own requests to run commands and edit files.
    pub auto_approve: bool,
    /// Ask for attention when a task finishes while the window is in the background.
    pub notify: bool,
    /// Send with Command+Return (Ctrl+Enter) and let Return add a new line.
    pub send_with_modifier: bool,
    pub scale: Scale,
    /// The language dictation listens for. `None` detects it.
    pub dictation_language: Option<String>,
    pub reading_pace: Pace,
    /// The effort level chosen for each model, by model ID. A model not listed runs at its
    /// default.
    pub efforts: std::collections::BTreeMap<String, String>,
    /// Whose record the Usage page shows.
    pub usage_source: crate::history::Source,
    /// Run every task on the chosen model, or let Auto pick one for each task.
    pub model_mode: crate::auto::Mode,
    /// The models Auto picks from.
    pub auto_models: crate::auto::Models,
    /// The tutorial has been closed once, so it no longer opens by itself.
    pub tutorial_seen: bool,
    /// Open web links in the in-app browser instead of the default browser.
    pub links_in_app: bool,
    /// Show HTML files OpenCode writes or edits in the in-app browser.
    pub auto_open_html: bool,
    /// Animations: following the system, fewer, or all.
    pub motion: Motion,
    /// How much of a task's working the transcript shows.
    pub transcript: Transcript,
    /// How wide the conversation column is.
    pub width: Width,
    /// Keep the computer from going to sleep while a task runs.
    pub keep_awake: bool,
    /// How many tasks may run at once, across sessions and projects.
    pub max_tasks: u8,
    /// The terminal docks at the side, under the pane, instead of at the bottom.
    pub terminal_side: bool,
}

/// How much of a task's working a reply shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Transcript {
    /// Replies and the steps taken.
    #[default]
    Normal,
    /// Also how long the model thought before each step.
    Thinking,
    /// Also each step's full command or path, wrapped instead of cut.
    Verbose,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Width {
    #[default]
    Narrow,
    Wide,
    Full,
}

impl Width {
    pub fn points(self) -> f32 {
        match self {
            Self::Narrow => 760.0,
            Self::Wide => 980.0,
            Self::Full => f32::INFINITY,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Motion {
    #[default]
    System,
    Reduced,
    Full,
}

impl Settings {
    /// Whether animations should keep still: chosen here, or asked for in the system's
    /// accessibility settings.
    pub fn reduce_motion(&self) -> bool {
        match self.motion {
            Motion::Reduced => true,
            Motion::Full => false,
            Motion::System => system_reduces_motion(),
        }
    }
}

/// The system's Reduce motion setting, read once. Only macOS reports it here.
fn system_reduces_motion() -> bool {
    static REDUCED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *REDUCED.get_or_init(|| {
        cfg!(target_os = "macos")
            && std::process::Command::new("defaults")
                .args(["read", "com.apple.universalaccess", "reduceMotion"])
                .stderr(std::process::Stdio::null())
                .output()
                .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).trim() == "1")
    })
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            executable: None,
            project: None,
            fallback: false,
            appearance: Appearance::System,
            read_aloud: false,
            default_model: crate::models::DEFAULT.into(),
            backup_model: crate::models::BACKUP.into(),
            time_limit_minutes: 0,
            auto_approve: false,
            notify: true,
            send_with_modifier: false,
            scale: Scale::Normal,
            dictation_language: None,
            reading_pace: Pace::Normal,
            efforts: Default::default(),
            usage_source: Default::default(),
            model_mode: Default::default(),
            auto_models: Default::default(),
            tutorial_seen: false,
            links_in_app: false,
            auto_open_html: false,
            motion: Motion::System,
            transcript: Transcript::Normal,
            width: Width::Narrow,
            keep_awake: false,
            max_tasks: 3,
            terminal_side: false,
        }
    }
}

pub struct Store {
    directory: PathBuf,
}

impl Store {
    pub fn open() -> io::Result<Self> {
        let root = dirs::data_dir()
            .ok_or_else(|| io::Error::other("Application data folder unavailable"))?;
        let directory = root.join("Super OpenCode");
        fs::create_dir_all(directory.join("sessions"))?;
        Ok(Self { directory })
    }

    /// The folder that holds settings, saved sessions, and the usage database.
    pub fn location(&self) -> &Path {
        &self.directory
    }

    pub fn settings(&self) -> io::Result<Settings> {
        Ok(read_optional(&self.directory.join("settings.json"))?.unwrap_or_default())
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        write_json(&self.directory.join("settings.json"), settings)
    }

    pub fn sessions(&self) -> io::Result<Vec<Session>> {
        let mut sessions: Vec<Session> =
            read_optional(&self.directory.join("sessions.json"))?.unwrap_or_default();
        for file in fs::read_dir(self.directory.join("sessions"))? {
            let path = file?.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let saved: Session = read_optional(&path)?
                    .ok_or_else(|| io::Error::other("Session moved while loading"))?;
                sessions.retain(|previous| previous.id != saved.id);
                sessions.push(saved);
            }
        }
        sessions.sort_by(|a, b| b.updated.total_cmp(&a.updated));
        Ok(sessions)
    }

    pub fn save(&self, session: &Session) -> io::Result<()> {
        write_json(
            &self
                .directory
                .join("sessions")
                .join(format!("{}.json", session.id)),
            session,
        )
    }

    fn usage(&self) -> rusqlite::Result<Connection> {
        let db = Connection::open(self.directory.join("usage.sqlite3"))?;
        db.busy_timeout(std::time::Duration::from_secs(2))?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS steps (
            id TEXT PRIMARY KEY, day TEXT, model TEXT, input INTEGER, output INTEGER,
            reasoning INTEGER, cache_read INTEGER, cache_write INTEGER
        );",
        )?;
        // Steps recorded since November 2026 also keep when they finished, in milliseconds
        // since the Unix epoch, and their OpenCode session. `day` stays the UTC date.
        for (name, kind) in [("at", "INTEGER"), ("session", "TEXT")] {
            let present: i64 = db.query_row(
                "SELECT COUNT(*) FROM pragma_table_info('steps') WHERE name = ?",
                [name],
                |row| row.get(0),
            )?;
            if present == 0 {
                db.execute_batch(&format!("ALTER TABLE steps ADD COLUMN {name} {kind}"))?;
            }
        }
        Ok(db)
    }

    pub fn record(&self, model: &str, millis: i64, part: &Value) -> rusqlite::Result<()> {
        let (Some(id), Some(date)) = (part["id"].as_str(), DateTime::from_timestamp_millis(millis))
        else {
            return Ok(());
        };
        let tokens = &part["tokens"];
        self.usage()?.execute(
            "INSERT OR IGNORE INTO steps
            (id, day, model, input, output, reasoning, cache_read, cache_write, at, session)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                id,
                date.date_naive().to_string(),
                model,
                tokens["input"].as_u64().unwrap_or(0),
                tokens["output"].as_u64().unwrap_or(0),
                tokens["reasoning"].as_u64().unwrap_or(0),
                tokens["cache"]["read"].as_u64().unwrap_or(0),
                tokens["cache"]["write"].as_u64().unwrap_or(0),
                millis,
                part["sessionID"].as_str(),
            ],
        )?;
        Ok(())
    }

    /// The usage database, for reading on another thread.
    pub fn usage_file(&self) -> rusqlite::Result<PathBuf> {
        // Opening it first creates the table and its newer columns.
        self.usage()?;
        Ok(self.directory.join("usage.sqlite3"))
    }

    /// Deletes every recorded step.
    pub fn clear_usage(&self) -> rusqlite::Result<()> {
        self.usage()?.execute("DELETE FROM steps", [])?;
        Ok(())
    }

    pub fn usage_today(&self) -> rusqlite::Result<Vec<Usage>> {
        let db = self.usage()?;
        let mut statement = db.prepare(
            "SELECT model, COUNT(*), SUM(input), SUM(output),
            SUM(reasoning), SUM(cache_read), SUM(cache_write)
            FROM steps WHERE COALESCE(date(at / 1000, 'unixepoch', 'localtime'), day) = ?
            GROUP BY model ORDER BY model",
        )?;
        statement
            .query_map([chrono::Local::now().date_naive().to_string()], |row| {
                Ok(Usage {
                    model: row.get(0)?,
                    steps: row.get(1)?,
                    input: row.get(2)?,
                    output: row.get(3)?,
                    reasoning: row.get(4)?,
                    cache_read: row.get(5)?,
                    cache_write: row.get(6)?,
                })
            })?
            .collect()
    }
}

pub struct Usage {
    pub model: String,
    pub steps: i64,
    pub input: i64,
    pub output: i64,
    pub reasoning: i64,
    pub cache_read: i64,
    pub cache_write: i64,
}

impl Usage {
    /// Tokens the model read and wrote, leaving out cache activity.
    pub fn tokens(&self) -> i64 {
        self.input + self.output + self.reasoning
    }
}

fn read_optional<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    match fs::File::open(path) {
        Ok(file) => serde_json::from_reader(io::BufReader::new(file))
            .map(Some)
            .map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_json(path: &Path, data: &impl Serialize) -> io::Result<()> {
    let mut temp = NamedTempFile::new_in(path.parent().expect("storage file has a parent"))?;
    serde_json::to_writer(temp.as_file_mut(), data).map_err(io::Error::other)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_set_the_context_size_and_add_up_per_session() {
        // The shape OpenCode 2.3 reports in step_finish.
        let first = StepTokens::of(&serde_json::json!({
            "tokens": {"input": 9067, "output": 3, "reasoning": 28, "cache": {"read": 0, "write": 0}}
        }));
        let second = StepTokens::of(&serde_json::json!({
            "tokens": {"input": 812, "output": 140, "reasoning": 0, "cache": {"read": 9070, "write": 0}}
        }));
        let mut session: Session = serde_json::from_value(serde_json::json!({
            "id": "DDB2877A-AE21-437B-954A-B49084725826", "projectPath": "/tmp/p",
            "routeID": "opencode/glm-5.3-flash-free", "serverSession": null,
            "turns": [], "updated": 0.0
        }))
        .unwrap();
        assert_eq!(
            session.context_tokens, 0,
            "older sessions load with nothing recorded"
        );
        session.add_step(first);
        assert_eq!(session.context_tokens, 9070);
        session.add_step(second);
        assert_eq!(session.context_tokens, 812 + 9070 + 140);
        assert_eq!(
            session.spent,
            Spent {
                steps: 2,
                read: 9067 + 812 + 9070,
                written: 3 + 28 + 140
            }
        );
    }

    #[test]
    fn resumes_swift_history_and_keeps_the_latest_revision() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store {
            directory: directory.path().into(),
        };
        fs::create_dir(store.directory.join("sessions")).unwrap();
        let id = "DDB2877A-AE21-437B-954A-B49084725826";
        let legacy = serde_json::json!([{
            "id": id,
            "projectPath": "/tmp/project",
            "routeID": "opencode/glm-5.3-flash-free",
            "serverSession": "existing-opencode-session",
            "updated": 811000000.5,
            "turns": [{
                "id": "7EDC934E-5EBB-48AD-8E1E-FAC1220CF699",
                "prompt": "Check this project",
                "model": "GLM 5.3 Flash",
                "output": "Done"
            }]
        }]);
        write_json(&store.directory.join("sessions.json"), &legacy).unwrap();
        let mut prior = store.sessions().unwrap().remove(0);
        assert_eq!(
            prior.server_session.as_deref(),
            Some("existing-opencode-session")
        );
        prior.turns[0].output = "Updated transcript".into();
        store.save(&prior).unwrap();
        let sessions = store.sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].turns[0].output, "Updated transcript");
    }
}
