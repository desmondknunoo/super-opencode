//! Usage over time, for the Usage page and `--usage`: a year of days, streaks, daily totals
//! by model, and the sessions that used the most.
//!
//! Two records can be read. Super OpenCode's own lists the steps of tasks it ran. OpenCode's
//! database on this computer lists every reply it produced, from any app or the terminal,
//! including the tasks Super OpenCode ran. Each is read on its own, so nothing counts twice.
//!
//! Tokens are what the model read fresh and wrote, leaving out cache reads and writes, the
//! same count as the status bar.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use chrono::{DateTime, Datelike, Days, Local, NaiveDate};
use rusqlite::{Connection, OpenFlags};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// Tasks run in Super OpenCode.
    App,
    /// Everything OpenCode recorded on this computer, including Super OpenCode's tasks.
    #[default]
    OpenCode,
}

/// One model reply or step.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// When it finished, in milliseconds since the Unix epoch.
    pub at: i64,
    /// The local day it finished on.
    pub day: NaiveDate,
    pub model: String,
    /// OpenCode's session ID, when recorded.
    pub session: Option<String>,
    pub tokens: u64,
    /// What OpenCode recorded as the charge, in US dollars.
    pub cost: f64,
}

/// A session OpenCode recorded: its title and folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recorded {
    pub title: String,
    pub directory: String,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    pub source: Source,
    /// Oldest first.
    pub steps: Vec<Step>,
    /// OpenCode's session titles, by session ID. Empty for Super OpenCode's record.
    pub sessions: HashMap<String, Recorded>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Day {
    pub tokens: u64,
    pub steps: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub tokens: u64,
    pub steps: u64,
    pub active_days: u32,
    /// Days in a row up to today, or up to yesterday when today has nothing yet.
    pub current_streak: u32,
    pub longest_streak: u32,
    pub busiest: Option<(NaiveDate, u64)>,
    pub first: Option<NaiveDate>,
    pub last_active: Option<NaiveDate>,
    pub cost: f64,
}

/// Daily totals for a chart, split by the models that used the most.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Daily {
    pub days: Vec<NaiveDate>,
    /// Model IDs, the busiest first, then `None` for every other model together.
    pub series: Vec<Option<String>>,
    /// Tokens for each day, in the order of `series`.
    pub values: Vec<Vec<u64>>,
}

impl Daily {
    pub fn total(&self) -> u64 {
        self.values.iter().flatten().sum()
    }

    pub fn series_total(&self, index: usize) -> u64 {
        self.values.iter().map(|day| day[index]).sum()
    }
}

/// How much one session used in a period.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionUse {
    pub id: String,
    pub tokens: u64,
    pub steps: u32,
    pub last: i64,
    pub model: String,
}

/// The models a chart names. The rest are drawn together as “Other models”.
pub const NAMED_SERIES: usize = 5;

pub fn local_day(millis: i64) -> Option<NaiveDate> {
    DateTime::from_timestamp_millis(millis).map(|time| time.with_timezone(&Local).date_naive())
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Where OpenCode keeps its database: what `opencode debug paths` reports, else its default.
pub fn opencode_database(binary: Option<&Path>) -> Option<PathBuf> {
    let reported = binary.and_then(|binary| {
        let output = Command::new(binary)
            .args(["debug", "paths"])
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| {
                line.strip_prefix("db")
                    .map(|rest| PathBuf::from(rest.trim()))
            })
    });
    let fallback = || {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
            .map(|data| data.join("opencode").join("opencode.db"))
    };
    reported.or_else(fallback).filter(|path| path.is_file())
}

impl History {
    pub fn load(source: Source, app: &Path, opencode: Option<&Path>) -> Result<Self, String> {
        match source {
            Source::App => Ok(Self {
                source,
                steps: from_app(app).map_err(|error| format!("Couldn’t read usage: {error}"))?,
                sessions: HashMap::new(),
            }),
            Source::OpenCode => {
                let path = opencode
                    .ok_or("OpenCode’s database wasn’t found on this computer.".to_owned())?;
                let (steps, sessions) = from_opencode(path)
                    .map_err(|error| format!("Couldn’t read OpenCode’s history: {error}"))?;
                Ok(Self {
                    source,
                    steps,
                    sessions,
                })
            }
        }
    }

    pub fn days(&self) -> BTreeMap<NaiveDate, Day> {
        let mut days = BTreeMap::<NaiveDate, Day>::new();
        for step in &self.steps {
            let day = days.entry(step.day).or_default();
            day.tokens += step.tokens;
            day.steps += 1;
        }
        days
    }

    pub fn summary(&self, today: NaiveDate) -> Summary {
        let days = self.days();
        let active: Vec<NaiveDate> = days
            .iter()
            .filter(|(_, day)| day.tokens > 0)
            .map(|(date, _)| *date)
            .collect();
        let (current_streak, longest_streak) = streaks(&active, today);
        Summary {
            tokens: days.values().map(|day| day.tokens).sum(),
            steps: days.values().map(|day| u64::from(day.steps)).sum(),
            active_days: active.len() as u32,
            current_streak,
            longest_streak,
            busiest: days
                .iter()
                .filter(|(_, day)| day.tokens > 0)
                .max_by_key(|(date, day)| (day.tokens, std::cmp::Reverse(**date)))
                .map(|(date, day)| (*date, day.tokens)),
            first: active.first().copied(),
            last_active: active.last().copied(),
            cost: self.steps.iter().map(|step| step.cost).sum(),
        }
    }

    /// The calendar years with any activity, newest first.
    pub fn years(&self) -> Vec<i32> {
        let mut years: Vec<i32> = self.steps.iter().map(|step| step.day.year()).collect();
        years.sort_unstable_by(|a, b| b.cmp(a));
        years.dedup();
        years
    }

    /// The models that used the most over the whole record, the most first. Charts name these
    /// and keep each one's colour whatever period they show.
    pub fn leaders(&self) -> Vec<String> {
        let mut by_model = HashMap::<&str, u64>::new();
        for step in &self.steps {
            *by_model.entry(step.model.as_str()).or_default() += step.tokens;
        }
        let mut ranked: Vec<(&str, u64)> = by_model.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        ranked
            .into_iter()
            .take(NAMED_SERIES)
            .map(|(model, _)| model.to_owned())
            .collect()
    }

    /// The `count` days ending on `last`, split by model: the leaders that used anything in
    /// those days, then every other model together.
    pub fn daily(&self, last: NaiveDate, count: u64) -> Daily {
        let first = last - Days::new(count.saturating_sub(1));
        let window: Vec<&Step> = self
            .steps
            .iter()
            .filter(|step| step.day >= first && step.day <= last && step.tokens > 0)
            .collect();
        let leaders = self.leaders();
        let mut series: Vec<Option<String>> = leaders
            .iter()
            .filter(|leader| window.iter().any(|step| &step.model == *leader))
            .map(|leader| Some(leader.clone()))
            .collect();
        if window.iter().any(|step| !leaders.contains(&step.model)) {
            series.push(None);
        }
        let days: Vec<NaiveDate> = first.iter_days().take(count as usize).collect();
        let mut values = vec![vec![0; series.len()]; days.len()];
        for step in window {
            let row = (step.day - first).num_days() as usize;
            let column = series
                .iter()
                .position(|name| name.as_deref() == Some(step.model.as_str()))
                .or_else(|| series.iter().position(Option::is_none))
                .unwrap_or_default();
            if let Some(cell) = values.get_mut(row).and_then(|row| row.get_mut(column)) {
                *cell += step.tokens;
            }
        }
        Daily {
            days,
            series,
            values,
        }
    }

    /// The sessions that used the most from `since` on, the most first.
    pub fn top_sessions(&self, since: NaiveDate, limit: usize) -> Vec<SessionUse> {
        let mut found = HashMap::<&str, SessionUse>::new();
        let mut models = HashMap::<&str, HashMap<&str, u64>>::new();
        for step in self.steps.iter().filter(|step| step.day >= since) {
            let Some(id) = step.session.as_deref() else {
                continue;
            };
            let entry = found.entry(id).or_insert_with(|| SessionUse {
                id: id.to_owned(),
                tokens: 0,
                steps: 0,
                last: 0,
                model: String::new(),
            });
            entry.tokens += step.tokens;
            entry.steps += 1;
            entry.last = entry.last.max(step.at);
            *models
                .entry(id)
                .or_default()
                .entry(&step.model)
                .or_default() += step.tokens;
        }
        let mut sessions: Vec<SessionUse> = found
            .into_values()
            .map(|mut session| {
                if let Some(model) = models
                    .get(session.id.as_str())
                    .and_then(|used| used.iter().max_by_key(|(_, tokens)| **tokens))
                {
                    session.model = (*model.0).to_owned();
                }
                session
            })
            .filter(|session| session.tokens > 0)
            .collect();
        sessions.sort_by(|a, b| b.tokens.cmp(&a.tokens).then(b.last.cmp(&a.last)));
        sessions.truncate(limit);
        sessions
    }
}

/// The current and longest runs of consecutive days in `active`, which is sorted.
pub fn streaks(active: &[NaiveDate], today: NaiveDate) -> (u32, u32) {
    let mut longest = 0;
    let mut run = 0;
    let mut previous: Option<NaiveDate> = None;
    for &day in active {
        run = match previous {
            Some(before) if before.succ_opt() == Some(day) => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        previous = Some(day);
    }
    let yesterday = today.pred_opt().unwrap_or(today);
    let current = match previous {
        Some(last) if last == today || last == yesterday => run,
        _ => 0,
    };
    (current, longest)
}

/// The weeks covering `first` to `last`, Sunday first as on GitHub. Days outside the range
/// are `None`.
pub fn weeks(first: NaiveDate, last: NaiveDate) -> Vec<[Option<NaiveDate>; 7]> {
    let start = first - Days::new(u64::from(first.weekday().num_days_from_sunday()));
    let mut weeks = Vec::new();
    let mut day = start;
    while day <= last {
        let mut week = [None; 7];
        for cell in &mut week {
            if day >= first && day <= last {
                *cell = Some(day);
            }
            day = day.succ_opt().unwrap_or(day);
        }
        weeks.push(week);
    }
    weeks
}

/// The values that split active days into four shades, from the quarters of their totals.
pub fn shade_limits(totals: impl Iterator<Item = u64>) -> [u64; 3] {
    let mut active: Vec<u64> = totals.filter(|total| *total > 0).collect();
    if active.is_empty() {
        return [0; 3];
    }
    active.sort_unstable();
    let at = |share: f64| active[((active.len() - 1) as f64 * share).round() as usize];
    [at(0.25), at(0.5), at(0.75)]
}

/// The shade for a day: 0 for none, then 1 to 4.
pub fn shade(tokens: u64, limits: [u64; 3]) -> usize {
    match tokens {
        0 => 0,
        _ if tokens <= limits[0] => 1,
        _ if tokens <= limits[1] => 2,
        _ if tokens <= limits[2] => 3,
        _ => 4,
    }
}

fn from_app(path: &Path) -> rusqlite::Result<Vec<Step>> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = db.prepare(
        "SELECT day, at, model, session, input + output + reasoning FROM steps ORDER BY day, at",
    )?;
    let rows = statement.query_map([], |row| {
        let day: String = row.get(0)?;
        let at: Option<i64> = row.get(1)?;
        Ok((day, at, row.get(2)?, row.get(3)?, row.get::<_, i64>(4)?))
    })?;
    let mut steps = Vec::new();
    for row in rows {
        let (day, at, model, session, tokens) = row?;
        // Steps recorded before November 2026 have only their UTC date.
        let (at, day) = match at.and_then(|at| Some((at, local_day(at)?))) {
            Some(found) => found,
            None => {
                let Ok(date) = NaiveDate::parse_from_str(&day, "%Y-%m-%d") else {
                    continue;
                };
                let noon = date
                    .and_hms_opt(12, 0, 0)
                    .map_or(0, |time| time.and_utc().timestamp_millis());
                (noon, date)
            }
        };
        steps.push(Step {
            at,
            day,
            model,
            session,
            tokens: tokens.max(0) as u64,
            cost: 0.0,
        });
    }
    steps.sort_by_key(|step| step.at);
    Ok(steps)
}

/// Reads OpenCode's replies and session titles, without changing its database.
fn from_opencode(path: &Path) -> rusqlite::Result<(Vec<Step>, HashMap<String, Recorded>)> {
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.busy_timeout(std::time::Duration::from_secs(2))?;
    let has = |table: &str| -> rusqlite::Result<bool> {
        db.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count > 0)
    };
    // OpenCode 2 keeps replies in `session_message`; OpenCode 1 kept them in `message`.
    let query = if has("session_message")? {
        "SELECT time_created, session_id,
            json_extract(data, '$.model.providerID') || '/' || json_extract(data, '$.model.id'),
            COALESCE(json_extract(data, '$.tokens.input'), 0)
                + COALESCE(json_extract(data, '$.tokens.output'), 0)
                + COALESCE(json_extract(data, '$.tokens.reasoning'), 0),
            COALESCE(json_extract(data, '$.cost'), 0)
        FROM session_message WHERE type = 'assistant'"
    } else if has("message")? {
        "SELECT time_created, session_id,
            json_extract(data, '$.providerID') || '/' || json_extract(data, '$.modelID'),
            COALESCE(json_extract(data, '$.tokens.input'), 0)
                + COALESCE(json_extract(data, '$.tokens.output'), 0)
                + COALESCE(json_extract(data, '$.tokens.reasoning'), 0),
            COALESCE(json_extract(data, '$.cost'), 0)
        FROM message WHERE json_extract(data, '$.role') = 'assistant'"
    } else {
        return Ok((Vec::new(), HashMap::new()));
    };
    let mut statement = db.prepare(query)?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<f64>>(3)?,
            row.get::<_, Option<f64>>(4)?,
        ))
    })?;
    let mut steps = Vec::new();
    for row in rows {
        let (at, session, model, tokens, cost) = row?;
        let Some(day) = local_day(at) else { continue };
        steps.push(Step {
            at,
            day,
            model: model.unwrap_or_else(|| "unknown".into()),
            session,
            tokens: tokens.unwrap_or(0.0).max(0.0) as u64,
            cost: cost.unwrap_or(0.0).max(0.0),
        });
    }
    steps.sort_by_key(|step| step.at);
    let mut sessions = HashMap::new();
    for table in ["session_v2", "session"] {
        if !has(table)? {
            continue;
        }
        let mut statement = db.prepare(&format!(
            "SELECT id, COALESCE(title, ''), directory FROM {table}"
        ))?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Recorded {
                    title: row.get(1)?,
                    directory: row.get(2)?,
                },
            ))
        })?;
        for row in rows {
            let (id, recorded) = row?;
            sessions.entry(id).or_insert(recorded);
        }
    }
    Ok((steps, sessions))
}

/// 161106972 as 161,106,972.
fn grouped(count: u64) -> String {
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

/// Usage for `--usage`: totals, streaks, and the last 20 weeks as a grid of shades.
pub fn report(history: &History, today: NaiveDate) -> String {
    let summary = history.summary(today);
    let mut out = String::new();
    let source = match history.source {
        Source::App => "Super OpenCode",
        Source::OpenCode => "OpenCode on this computer",
    };
    out.push_str(&format!("Usage recorded by {source}\n\n"));
    if summary.steps == 0 {
        out.push_str("Nothing recorded yet.\n");
        return out;
    }
    let plural = |count: u32, one: &str| {
        if count == 1 {
            format!("{count} {one}")
        } else {
            format!("{count} {one}s")
        }
    };
    out.push_str(&format!("Tokens            {}\n", grouped(summary.tokens)));
    out.push_str(&format!("Steps             {}\n", grouped(summary.steps)));
    out.push_str(&format!("Active days       {}\n", summary.active_days));
    out.push_str(&format!(
        "Current streak    {}\n",
        plural(summary.current_streak, "day")
    ));
    out.push_str(&format!(
        "Longest streak    {}\n",
        plural(summary.longest_streak, "day")
    ));
    if let Some((day, tokens)) = summary.busiest {
        out.push_str(&format!(
            "Busiest day       {} ({} tokens)\n",
            day.format("%-d %b %Y"),
            grouped(tokens)
        ));
    }
    if history.source == Source::OpenCode {
        out.push_str(&format!("Model charges     ${:.2}\n", summary.cost));
    }
    let first = today - Days::new(7 * 20 - 1);
    let days = history.days();
    let limits = shade_limits(days.range(first..=today).map(|(_, day)| day.tokens));
    out.push_str("\nLast 20 weeks\n");
    let grid = weeks(first, today);
    for (row, name) in ["", "Mon", "", "Wed", "", "Fri", ""].iter().enumerate() {
        out.push_str(&format!("{name:<4}"));
        for week in &grid {
            let mark = match week[row] {
                None => ' ',
                Some(day) => ['·', '░', '▒', '▓', '█']
                    [shade(days.get(&day).map_or(0, |day| day.tokens), limits)],
            };
            out.push(mark);
        }
        out.push('\n');
    }
    out.push_str("    less · ░ ▒ ▓ █ more\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn step(day: &str, model: &str, session: &str, tokens: u64) -> Step {
        let day = date(day);
        Step {
            at: day
                .and_hms_opt(12, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp_millis(),
            day,
            model: model.into(),
            session: Some(session.into()),
            tokens,
            cost: 0.0,
        }
    }

    #[test]
    fn streaks_count_days_in_a_row() {
        let active = [
            date("2026-11-01"),
            date("2026-11-02"),
            date("2026-11-03"),
            date("2026-11-07"),
            date("2026-11-08"),
        ];
        assert_eq!(streaks(&active, date("2026-11-08")), (2, 3));
        // Today has nothing yet, so the run ending yesterday still counts.
        assert_eq!(streaks(&active, date("2026-11-09")), (2, 3));
        assert_eq!(streaks(&active, date("2026-11-10")), (0, 3));
        assert_eq!(streaks(&[], date("2026-11-10")), (0, 0));
    }

    #[test]
    fn weeks_start_on_sunday_and_cover_the_range() {
        // 15 November 2026 is a Sunday.
        let grid = weeks(date("2026-11-04"), date("2026-11-15"));
        assert_eq!(grid.len(), 3);
        assert_eq!(grid[0][0], None); // Sunday 1 November is before the range.
        assert_eq!(grid[0][3], Some(date("2026-11-04")));
        assert_eq!(grid[2][0], Some(date("2026-11-15")));
        assert_eq!(grid[2][1], None);
    }

    #[test]
    fn shades_split_active_days_into_quarters() {
        let limits = shade_limits([0, 10, 20, 30, 40, 50].into_iter());
        assert_eq!(shade(0, limits), 0);
        assert_eq!(shade(10, limits), 1);
        assert_eq!(shade(50, limits), 4);
        assert_eq!(shade_limits([0, 0].into_iter()), [0; 3]);
    }

    #[test]
    fn summaries_daily_series_and_top_sessions() {
        let mut steps = vec![
            step("2026-11-13", "opencode/glm", "a", 100),
            step("2026-11-14", "opencode/glm", "a", 300),
            step("2026-11-14", "opencode/deepseek", "b", 50),
            step("2026-11-15", "opencode/mimo", "c", 0),
        ];
        for index in 0..6 {
            steps.push(step(
                "2026-11-15",
                &format!("other/m{index}"),
                "d",
                1 + index,
            ));
        }
        let history = History {
            source: Source::App,
            steps,
            sessions: HashMap::new(),
        };
        let summary = history.summary(date("2026-11-15"));
        assert_eq!(summary.active_days, 3);
        assert_eq!(summary.current_streak, 3);
        assert_eq!(summary.busiest, Some((date("2026-11-14"), 350)));
        let daily = history.daily(date("2026-11-15"), 7);
        assert_eq!(daily.days.len(), 7);
        assert_eq!(daily.series.len(), NAMED_SERIES + 1);
        assert_eq!(daily.series[0].as_deref(), Some("opencode/glm"));
        assert_eq!(daily.series.last(), Some(&None));
        assert_eq!(daily.total(), summary.tokens);
        let top = history.top_sessions(date("2026-11-14"), 2);
        assert_eq!(top[0].id, "a");
        assert_eq!(top[0].tokens, 300);
        assert_eq!(top.len(), 2);
        assert!(report(&history, date("2026-11-15")).contains("Current streak    3 days"));
    }

    /// Reads OpenCode's real database on this computer: `cargo test opencode_history -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn opencode_history() {
        let path = opencode_database(crate::runner::find_opencode().as_deref()).unwrap();
        let history = History::load(Source::OpenCode, Path::new("unused"), Some(&path)).unwrap();
        println!("{}", report(&history, today()));
        println!("{} sessions named", history.sessions.len());
        assert!(!history.steps.is_empty());
    }
}
