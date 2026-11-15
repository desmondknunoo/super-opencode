use std::{
    collections::VecDeque,
    env,
    ffi::OsString,
    io::{self, BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::activity;

pub use crate::models::{BACKUP, DEFAULT as BUILTIN};
const CONFIG: &str = include_str!("../opencode.json");
const CATALOG_URL: &str = "https://openrouter.ai/api/v1/models";

/// How long OpenCode gets to save its session after Stop before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(3);
const DIAGNOSTIC_LINES: usize = 8;

/// OpenRouter publishers listed by `--catalog`.
const PUBLISHERS: &[&str] = &[
    "baidu",
    "baichuan",
    "bytedance",
    "deepseek",
    "dots-studio",
    "01-ai",
    "inclusionai",
    "internlm",
    "meituan",
    "minimax",
    "moonshotai",
    "qwen",
    "stepfun",
    "tencent",
    "thudm",
    "xiaomi",
    "z-ai",
    "zhipuai",
];

pub enum Event {
    Output(String),
    Session(String),
    /// The run ended on a rate or usage limit and did not continue on the backup model.
    Limited,
    Usage {
        model: String,
        timestamp: i64,
        part: Value,
    },
    /// What OpenCode is doing now, such as “Running cargo test”, and since when, in
    /// milliseconds since the Unix epoch.
    Activity {
        doing: String,
        since: i64,
    },
    /// The task's private OpenCode server, so a message can be added while it works. The
    /// password stays in memory and is never shown.
    Server(Reach),
    Finished(Result<(), String>),
}

/// How to reach a running task's private OpenCode server.
#[derive(Clone)]
pub struct Reach {
    pub url: String,
    password: String,
}

impl std::fmt::Debug for Reach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reach")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

/// Adds `text` to what the task running in `session` is doing: OpenCode delivers it to the
/// model at its next step, as when you type to an agent that is working.
pub fn steer(reach: &Reach, session: &str, text: &str) -> Result<(), String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        .build();
    let agent: ureq::Agent = config.into();
    let response = agent
        .post(format!("{}/api/session/{session}/prompt", reach.url))
        .header(
            "Authorization",
            format!("Basic {}", base64(&format!("opencode:{}", reach.password))),
        )
        .send_json(serde_json::json!({ "text": text, "delivery": "steer" }))
        .map_err(|error| format!("Couldn’t reach the task: {error}"))?;
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(format!("OpenCode didn’t take the message (HTTP {status})."))
    }
}

pub struct Task {
    pub directory: PathBuf,
    pub binary: Option<PathBuf>,
    pub model: String,
    pub prompt: String,
    pub session: Option<String>,
    pub fallback: bool,
    /// Files OpenCode attaches to the message with `--file`.
    pub files: Vec<PathBuf>,
    /// The model a rate-limited task continues on when `fallback` is set.
    pub backup: String,
    /// Let OpenCode approve its own permission requests (`--auto`).
    pub auto_approve: bool,
    /// Stop the task after this long.
    pub time_limit: Option<Duration>,
    /// The model's effort level, such as `high`. `None` uses the model's default.
    pub variant: Option<String>,
    /// The OpenCode agent to run, such as `plan`. `None` uses OpenCode's default, `build`.
    pub agent: Option<String>,
    /// Report what OpenCode is doing as it happens, through a private OpenCode server.
    /// Without it, steps are reported only once they finish.
    pub live: bool,
}

impl Task {
    /// What `--model` receives: the model, and its effort level after a `#`.
    fn model_argument(&self) -> String {
        match &self.variant {
            Some(variant) => format!("{}#{variant}", self.model),
            None => self.model.clone(),
        }
    }
}

pub fn run(task: Task, stop: &AtomicBool, send: impl Fn(Event)) {
    let result = run_task(task, stop, &send, check_free_model);
    send(Event::Finished(result));
}

fn run_task(
    task: Task,
    stop: &AtomicBool,
    send: &impl Fn(Event),
    verify_free: impl Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    let binary = task.binary.clone().or_else(find_opencode).ok_or_else(|| {
        "OpenCode 2.x was not found. Install it or select the executable in Settings.".to_owned()
    })?;
    let deadline = task.time_limit.map(|limit| Instant::now() + limit);
    for model in [&task.model, &task.backup] {
        if !crate::models::is_free(model) {
            return Err(format!(
                "{model} is not on Super OpenCode’s list of free models, so it was not run."
            ));
        }
    }
    if task.model.starts_with("openrouter/") {
        verify_free(&task.model)?;
    }
    let attempt = invoke(&binary, &task, stop, deadline, send)?;
    if !task.fallback
        || task.model == task.backup
        || !attempt.limited
        || stop.load(Ordering::Relaxed)
    {
        if attempt.limited {
            send(Event::Limited);
        }
        return attempt.result;
    }
    let Some(session) = attempt.session else {
        return Err("OpenCode hit a rate limit before returning a session ID.".into());
    };
    if task.backup.starts_with("openrouter/") {
        verify_free(&task.backup)?;
    }
    let provider = crate::models::find(&task.backup)
        .map_or_else(|| "its provider".to_owned(), |model| model.provider_name());
    send(Event::Output(format!(
        "Rate limit reached. Continuing with {} on {provider}.\n",
        crate::models::name_of(&task.backup)
    )));
    let followup = Task {
        session: Some(session),
        // Effort levels differ between models; the backup starts on its own default.
        variant: None,
        model: task.backup.clone(),
        prompt: format!(
            "Continue in the existing workspace. Inspect earlier tool results and current files. Preserve completed work.\n\nOriginal task:\n{}",
            task.prompt
        ),
        ..task
    };
    invoke(&binary, &followup, stop, deadline, send)?.result
}

pub fn find_opencode() -> Option<PathBuf> {
    let filename = if cfg!(windows) {
        "opencode.exe"
    } else {
        "opencode"
    };
    let home = dirs::home_dir();
    let mut candidates: Vec<PathBuf> = env::var_os("OPENCODE_BIN")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    if let Some(home) = home {
        candidates.push(home.join(".opencode").join("bin").join(filename));
        candidates.push(home.join(".local").join("bin").join(filename));
    }
    if let Some(path) = env::var_os("PATH") {
        candidates.extend(env::split_paths(&path).map(|p| p.join(filename)));
    }
    #[cfg(target_os = "macos")]
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/opencode"),
        PathBuf::from("/usr/local/bin/opencode"),
    ]);
    candidates
        .into_iter()
        .find(|p| p.is_absolute() && p.is_file())
}

/// The PATH handed to OpenCode. An app opened from Finder, the Dock, or a desktop launcher
/// starts with a minimal PATH, so the usual install folders are added after the inherited
/// entries. Inherited entries keep their order and priority.
pub(crate) fn child_path(inherited: Option<OsString>, home: Option<PathBuf>) -> Option<OsString> {
    let mut entries: Vec<PathBuf> = inherited
        .map(|path| env::split_paths(&path).collect())
        .unwrap_or_default();
    let mut extra = Vec::new();
    if let Some(home) = home {
        extra.push(home.join(".opencode").join("bin"));
        extra.push(home.join(".local").join("bin"));
    }
    if cfg!(unix) {
        extra
            .extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    }
    for folder in extra {
        if folder.is_dir() && !entries.contains(&folder) {
            entries.push(folder);
        }
    }
    env::join_paths(entries).ok()
}

fn catalog() -> Result<Vec<Value>, String> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .build();
    let agent: ureq::Agent = config.into();
    let mut listing: Value = agent.get(CATALOG_URL).call()
        .and_then(|mut response| response.body_mut().read_json())
        .map_err(|_| "Couldn’t check free model availability with OpenRouter. Try again or choose GLM 5.3 Flash.")?;
    match listing["data"].take() {
        Value::Array(entries) => Ok(entries),
        _ => Err("OpenRouter’s model catalog came back in an unexpected format. Try again or choose GLM 5.3 Flash.".into()),
    }
}

fn check_free_model(model: &str) -> Result<(), String> {
    let slug = model
        .strip_prefix("openrouter/")
        .ok_or("Unrecognized model provider.")?;
    if catalog()?.iter().any(|entry| available_free(entry, slug)) {
        Ok(())
    } else {
        Err("OpenRouter no longer lists this model as free with tool support. Choose another model.".into())
    }
}

/// OpenCode model IDs for the free OpenRouter models that support tools, from [`PUBLISHERS`].
pub fn free_catalog() -> Result<Vec<String>, String> {
    let mut models: Vec<String> = catalog()?
        .iter()
        .filter(|entry| is_free_tool_model(entry))
        .filter_map(|entry| entry["id"].as_str())
        .filter(|id| {
            id.split('/')
                .next()
                .is_some_and(|publisher| PUBLISHERS.contains(&publisher))
        })
        .map(|id| format!("openrouter/{id}"))
        .collect();
    models.sort();
    Ok(models)
}

fn is_free_tool_model(entry: &Value) -> bool {
    let Some(pricing) = entry["pricing"].as_object() else {
        return false;
    };
    entry["id"].as_str().is_some_and(|id| id.ends_with(":free"))
        && entry["supported_parameters"]
            .as_array()
            .is_some_and(|p| p.iter().any(|v| v == "tools"))
        && pricing.contains_key("prompt")
        && pricing.contains_key("completion")
        && pricing.values().all(|price| {
            price
                .as_f64()
                .or_else(|| price.as_str().and_then(|p| p.parse().ok()))
                == Some(0.0)
        })
}

fn available_free(entry: &Value, slug: &str) -> bool {
    entry["id"].as_str() == Some(slug) && is_free_tool_model(entry)
}

struct Attempt {
    result: Result<(), String>,
    limited: bool,
    session: Option<String>,
}

/// Why a run ended early, as the app reports it.
fn time_limit_message(task: &Task) -> String {
    let minutes = task.time_limit.map_or(0, |limit| limit.as_secs() / 60);
    format!("Stopped after {minutes} minutes, the time limit set in Settings.")
}

fn invoke(
    binary: &Path,
    task: &Task,
    stop: &AtomicBool,
    deadline: Option<Instant>,
    send: &impl Fn(Event),
) -> Result<Attempt, String> {
    if stop.load(Ordering::Relaxed) {
        return Err("Stopped".into());
    }
    // A private server reports each step as it starts; without one, OpenCode runs its own.
    let server = if task.live {
        Server::start(binary, task, stop)
    } else {
        None
    };
    if server.is_none() && task.live {
        send(Event::Activity {
            doing: "Working".into(),
            since: now_millis(),
        });
    }
    if let Some(server) = &server {
        send(Event::Server(Reach {
            url: server.url.clone(),
            password: server.password.clone(),
        }));
    }
    let mut command = Command::new(binary);
    match &server {
        Some(server) => {
            command
                .args(["--server", &server.url])
                .env("OPENCODE_PASSWORD", &server.password);
        }
        None => {
            command.arg("--standalone");
        }
    }
    command.args([
        "run",
        "--format",
        "json",
        "--thinking",
        "--title",
        "Super OpenCode",
        "--model",
        &task.model_argument(),
    ]);
    if let Some(agent) = &task.agent {
        command.args(["--agent", agent]);
    }
    if let Some(session) = &task.session {
        command.args(["--session", session]);
    }
    if task.auto_approve {
        command.arg("--auto");
    }
    for file in &task.files {
        command.arg("--file").arg(file);
    }
    command
        .args(["--", &task.prompt])
        .current_dir(&task.directory)
        .env("PWD", &task.directory)
        .env("OPENCODE_CONFIG_CONTENT", CONFIG)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = child_path(env::var_os("PATH"), dirs::home_dir()) {
        command.env("PATH", path);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Couldn’t start OpenCode: {e}"))?;
    let (tx, rx) = mpsc::channel();
    read_lines(
        child.stdout.take().expect("piped stdout"),
        tx.clone(),
        false,
    );
    read_lines(child.stderr.take().expect("piped stderr"), tx, true);
    let mut events = Attempt {
        result: Ok(()),
        limited: false,
        session: task.session.clone(),
    };
    let mut diagnostics = VecDeque::new();

    let expired = || deadline.is_some_and(|deadline| Instant::now() >= deadline);
    loop {
        if stop.load(Ordering::Relaxed) {
            stop_child(&mut child, &rx);
            return Err("Stopped".into());
        }
        if expired() {
            stop_child(&mut child, &rx);
            return Err(time_limit_message(task));
        }
        if let Some(server) = &server {
            server.report(send);
        }
        match rx.recv_timeout(Duration::from_millis(75)) {
            Ok((line, false)) => consume(&line, task, &mut events, send),
            Ok((line, true)) => remember(&mut diagnostics, &line),
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let exit = loop {
        if stop.load(Ordering::Relaxed) {
            stop_child(&mut child, &rx);
            return Err("Stopped".into());
        }
        if expired() {
            stop_child(&mut child, &rx);
            return Err(time_limit_message(task));
        }
        if let Some(exit) = child.try_wait().map_err(|e| e.to_string())? {
            break exit;
        }
        thread::sleep(Duration::from_millis(50));
    };
    if !exit.success() && events.result.is_ok() {
        events.result = Err(exit_message(exit, &diagnostics));
    }
    Ok(events)
}

fn read_lines(reader: impl Read + Send + 'static, send: Sender<(String, bool)>, diagnostic: bool) {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut bytes = Vec::new();
        while let Ok(count) = reader.read_until(b'\n', &mut bytes) {
            if count == 0
                || send
                    .send((String::from_utf8_lossy(&bytes).into_owned(), diagnostic))
                    .is_err()
            {
                break;
            }
            bytes.clear();
        }
    });
}

/// Keeps the last few lines OpenCode wrote to stderr, without terminal colour codes.
fn remember(diagnostics: &mut VecDeque<String>, line: &str) {
    let mut plain = String::new();
    let mut characters = line.chars();
    while let Some(character) = characters.next() {
        if character == '\u{1b}' {
            // Skip a colour or cursor sequence: ESC [ ... final letter.
            if characters.next() == Some('[') {
                for next in characters.by_ref() {
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if !character.is_control() {
            plain.push(character);
        }
    }
    let plain = plain.trim();
    if plain.is_empty() {
        return;
    }
    if diagnostics.len() == DIAGNOSTIC_LINES {
        diagnostics.pop_front();
    }
    diagnostics.push_back(plain.chars().take(300).collect());
}

fn exit_message(exit: ExitStatus, diagnostics: &VecDeque<String>) -> String {
    if diagnostics.is_empty() {
        return format!(
            "OpenCode exited with {exit}. Check its installation and provider connection."
        );
    }
    let mut message = format!("OpenCode exited with {exit}. Its last output:");
    for line in diagnostics {
        message.push('\n');
        message.push_str(line);
    }
    message
}

fn stop_child(child: &mut Child, output: &Receiver<(String, bool)>) {
    let pid = child.id();
    #[cfg(unix)]
    {
        // The child leads its own process group, so each signal also reaches the tools it
        // started. Ask first so OpenCode can save its session, then kill whatever is left.
        // The child is reaped only after the final signal, which keeps its group ID reserved.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        let deadline = Instant::now() + STOP_GRACE;
        while Instant::now() < deadline {
            // Both pipes closing means OpenCode has exited.
            if output.recv_timeout(Duration::from_millis(50)) == Err(RecvTimeoutError::Disconnected)
            {
                break;
            }
        }
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = (output, STOP_GRACE);
        let _ = Command::new("taskkill.exe")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .output();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Milliseconds since the Unix epoch, the clock OpenCode's events use.
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}

/// How long a private server gets to start listening and to open its event stream.
const SERVER_START: Duration = Duration::from_secs(10);
const STREAM_START: Duration = Duration::from_secs(3);

/// What the event stream reports to the task.
enum Live {
    Connected,
    Doing(String, i64),
}

/// A private OpenCode server for one task, listening on this computer only, behind a
/// password made for it. Its event stream says what OpenCode is doing as it happens.
/// Dropping it stops the server and every command it started.
struct Server {
    child: Child,
    url: String,
    password: String,
    live: Receiver<Live>,
}

impl Server {
    /// Starts the server, or returns `None` so the task runs without live activity.
    fn start(binary: &Path, task: &Task, stop: &AtomicBool) -> Option<Self> {
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .and_then(|listener| listener.local_addr())
            .ok()?
            .port();
        let password = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let mut command = Command::new(binary);
        command
            .args([
                "serve",
                "--hostname",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ])
            .current_dir(&task.directory)
            .env("PWD", &task.directory)
            .env("OPENCODE_CONFIG_CONTENT", CONFIG)
            .env("OPENCODE_PASSWORD", &password)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(path) = child_path(env::var_os("PATH"), dirs::home_dir()) {
            command.env("PATH", path);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let child = command.spawn().ok()?;
        let (sender, live) = mpsc::channel();
        let mut server = Self {
            child,
            url: format!("http://127.0.0.1:{port}"),
            password,
            live,
        };
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let deadline = Instant::now() + SERVER_START;
        loop {
            if stop.load(Ordering::Relaxed)
                || Instant::now() >= deadline
                || !matches!(server.child.try_wait(), Ok(None))
            {
                return None;
            }
            if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(40));
        }
        watch(
            format!("{}/api/event", server.url),
            format!("Basic {}", base64(&format!("opencode:{}", server.password))),
            task.directory.clone(),
            sender,
        );
        // Events before the stream opens would be missed, so the task waits for it.
        match server
            .live
            .recv_timeout(STREAM_START + Duration::from_secs(1))
        {
            Ok(Live::Connected) => Some(server),
            _ => None,
        }
    }

    /// Passes on what OpenCode has started doing since the last call.
    fn report(&self, send: &impl Fn(Event)) {
        let mut latest = None;
        while let Ok(update) = self.live.try_recv() {
            if let Live::Doing(doing, since) = update {
                latest = Some((doing, since));
            }
        }
        if let Some((doing, since)) = latest {
            send(Event::Activity { doing, since });
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let pid = self.child.id();
        #[cfg(unix)]
        {
            // The server leads its own process group, which holds the commands it ran.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGTERM);
            }
            let deadline = Instant::now() + STOP_GRACE;
            while Instant::now() < deadline && matches!(self.child.try_wait(), Ok(None)) {
                thread::sleep(Duration::from_millis(25));
            }
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill.exe")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .creation_flags(0x0800_0000)
                .stdin(Stdio::null())
                .output();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reads the server's event stream on its own thread until the server stops.
fn watch(url: String, authorization: String, project: PathBuf, live: Sender<Live>) {
    thread::spawn(move || {
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(3)))
            .build();
        let agent: ureq::Agent = config.into();
        // The port can accept connections a moment before the server answers on it.
        let deadline = Instant::now() + STREAM_START;
        let response = loop {
            match agent
                .get(&url)
                .header("Authorization", &authorization)
                .header("Accept", "text/event-stream")
                .call()
            {
                Ok(response) => break response,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
                Err(_) => return,
            }
        };
        if live.send(Live::Connected).is_err() {
            return;
        }
        let mut steps = Steps::default();
        let reader = BufReader::new(response.into_body().into_reader());
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let Ok(event) = serde_json::from_str::<Value>(data.trim_start()) else {
                continue;
            };
            if let Some((doing, since)) = steps.update(&event, &project)
                && live.send(Live::Doing(doing, since)).is_err()
            {
                break;
            }
        }
    });
}

/// Follows OpenCode's events to say what it is doing now.
#[derive(Default)]
struct Steps {
    /// Tools started and not yet finished: ID, name, what the line says, and since when.
    tools: Vec<(String, String, String, i64)>,
}

impl Steps {
    fn update(&mut self, event: &Value, project: &Path) -> Option<(String, i64)> {
        let at = event["created"].as_i64().unwrap_or_else(now_millis);
        let data = &event["data"];
        let id = data["id"].as_str().unwrap_or_default();
        let kind = event["type"].as_str()?;
        let doing = match kind {
            "session.step.started" => "Waiting for the model".to_owned(),
            "session.reasoning.started" => "Thinking".to_owned(),
            "session.text.started" => "Writing the reply".to_owned(),
            "session.tool.input.started" => {
                let name = data["name"].as_str().unwrap_or("tool").to_owned();
                let doing = activity::preparing(&name);
                self.tools.retain(|tool| tool.0 != id);
                self.tools.push((id.to_owned(), name, doing.clone(), at));
                doing
            }
            "session.tool.called" => {
                let tool = self.tools.iter_mut().find(|tool| tool.0 == id)?;
                let detail = activity::detail(&tool.1, &data["input"], Some(project));
                tool.2 = activity::doing(&tool.1, &detail);
                tool.3 = at;
                return Some((tool.2.clone(), at));
            }
            "session.tool.input.delta" | "session.tool.input.ended" | "session.tool.progress" => {
                return None;
            }
            finished if finished.starts_with("session.tool.") => {
                self.tools.retain(|tool| tool.0 != id);
                return Some(match self.tools.last() {
                    Some(running) => (running.2.clone(), running.3),
                    None => ("Waiting for the model".to_owned(), at),
                });
            }
            _ => return None,
        };
        Some((doing, at))
    }
}

/// Standard Base64 with padding, for the server's Basic authorization header.
fn base64(text: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let group = u32::from(bytes[0]) << 16 | u32::from(bytes[1]) << 8 | u32::from(bytes[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(ALPHABET[(group >> (18 - 6 * index) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn consume(line: &str, task: &Task, attempt: &mut Attempt, send: &impl Fn(Event)) {
    let Ok(event) = serde_json::from_str::<Value>(line) else {
        return;
    };
    if let Some(id) = event["sessionID"].as_str() {
        if attempt
            .session
            .as_deref()
            .is_some_and(|expected| expected != id)
        {
            attempt.result = Err("OpenCode returned a different session ID.".into());
            attempt.limited = false;
            return;
        }
        if attempt.session.is_none() {
            attempt.session = Some(id.to_owned());
            send(Event::Session(id.to_owned()));
        }
    }
    let part = &event["part"];
    match event["type"].as_str() {
        Some("text") => {
            if let Some(text) = part["text"].as_str() {
                send(Event::Output(format!("{text}\n")));
            }
        }
        Some("tool_use") => send(Event::Output(format!(
            "{}\n",
            activity::tool_notice(part, Some(&task.directory))
        ))),
        Some("reasoning") => {
            // A blank reasoning part is the model's empty thinking block, not a step.
            if part["text"]
                .as_str()
                .is_some_and(|text| !text.trim().is_empty())
            {
                send(Event::Output(format!(
                    "{}\n",
                    activity::reasoning_notice(part)
                )));
            }
        }
        Some("step_finish") => {
            if let Some(timestamp) = event["timestamp"].as_i64() {
                send(Event::Usage {
                    model: task.model.clone(),
                    timestamp,
                    part: part.clone(),
                });
            }
        }
        Some("error") => {
            // OpenCode 2.x reports `{type, message}` and adds `status` only when it knows the
            // HTTP code, so a rate limit is recognised by either field.
            let error = &event["error"];
            let status = ["status", "statusCode"]
                .iter()
                .filter_map(|key| error.get(key))
                .find(|value| !value.is_null())
                .map(|value| {
                    value
                        .as_str()
                        .map_or_else(|| value.to_string(), str::to_owned)
                });
            let kind = error["type"].as_str();
            attempt.limited = status.as_deref() == Some("429")
                || matches!(kind, Some("provider.rate-limit" | "provider.free-limit"));
            attempt.result = Err(describe(
                &task.model,
                status.as_deref(),
                kind,
                error["message"].as_str(),
            ));
        }
        _ => {}
    }
}

/// Says what went wrong and what to do next, then quotes OpenCode's own explanation.
fn describe(
    model: &str,
    status: Option<&str>,
    kind: Option<&str>,
    message: Option<&str>,
) -> String {
    let code = status.or(kind).unwrap_or("unknown error");
    let summary = match (status, kind) {
        (Some("401" | "403"), _) | (_, Some("provider.auth")) => format!(
            "{model} rejected the request ({code}). Check the provider connection in OpenCode."
        ),
        (Some("429"), _) | (_, Some("provider.rate-limit")) => format!(
            "{model} reached its rate limit ({code}). Try another free model or wait for the limit to reset."
        ),
        (_, Some("provider.free-limit")) => {
            format!("{model} reached its free usage limit ({code}). Try another free model.")
        }
        (_, Some("provider.no-route")) => {
            format!("{model} is unavailable. Check that its OpenCode provider is configured.")
        }
        _ => format!("{model}: {code}"),
    };
    match message.map(str::trim).filter(|text| !text.is_empty()) {
        Some(message) => format!("{summary}\nOpenCode reported: {message}"),
        None => summary,
    }
}

/// One permission rule, said for people: "Read *.env".
fn rule_label(action: &str, resource: &str) -> String {
    let target = if resource == "*" {
        "anything"
    } else {
        resource
    };
    match action {
        "read" => format!("Read {target}"),
        "edit" => format!("Edit {target}"),
        "shell" | "bash" => {
            if resource == "*" {
                "Run any command".to_owned()
            } else {
                format!("Run {target}")
            }
        }
        "external_directory" => format!("Use folders outside the project ({target})"),
        "webfetch" => format!("Fetch web pages ({target})"),
        "websearch" => "Search the web".to_owned(),
        "question" => "Ask you questions".to_owned(),
        "*" => "Anything".to_owned(),
        other => format!("{other} {target}"),
    }
}

/// The rules `agent` sets to ask, from `opencode debug agents` output.
fn parse_ask_rules(json: &str, agent: &str) -> Result<Vec<String>, String> {
    let agents: Value = serde_json::from_str(json)
        .map_err(|_| "OpenCode’s agent list came back in an unexpected format.".to_owned())?;
    let rules = agents
        .as_array()
        .and_then(|list| list.iter().find(|entry| entry["id"] == agent))
        .and_then(|entry| entry["permissions"].as_array())
        .ok_or_else(|| format!("OpenCode has no {agent} agent."))?;
    let mut asked = Vec::new();
    for rule in rules {
        if rule["effect"] != "ask" {
            continue;
        }
        let label = rule_label(
            rule["action"].as_str().unwrap_or("*"),
            rule["resource"].as_str().unwrap_or("*"),
        );
        if !asked.contains(&label) {
            asked.push(label);
        }
    }
    Ok(asked)
}

/// The requests OpenCode's `build` agent stops to ask about, with the user's configuration
/// applied. Full access approves these.
pub fn ask_rules(binary: &Path) -> Result<Vec<String>, String> {
    let mut command = Command::new(binary);
    command
        .args(["--standalone", "debug", "agents"])
        .current_dir(env::temp_dir())
        .env("OPENCODE_CONFIG_CONTENT", CONFIG)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    if let Some(path) = child_path(env::var_os("PATH"), dirs::home_dir()) {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command
        .output()
        .map_err(|error| format!("Couldn’t ask OpenCode for its rules: {error}"))?;
    parse_ask_rules(&String::from_utf8_lossy(&output.stdout), "build")
}

pub fn canonical_project(path: &Path) -> io::Result<PathBuf> {
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Choose an existing project folder.",
        ));
    }
    #[cfg(windows)]
    // Canonical Windows paths use the extended namespace, which some OpenCode tools display literally.
    {
        if path.is_absolute() {
            Ok(path.to_owned())
        } else {
            Ok(env::current_dir()?.join(path))
        }
    }
    #[cfg(not(windows))]
    path.canonicalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_zero_cost_tool_models_pass() {
        let slug = "test/model:free";
        let mut entry = json!({
            "id": slug, "supported_parameters": ["tools"],
            "pricing": {"prompt": "0", "completion": "0", "request": 0}
        });
        assert!(available_free(&entry, slug));
        entry["pricing"]["request"] = json!("0.01");
        assert!(!available_free(&entry, slug));
        entry["pricing"]["request"] = json!("0");
        entry["supported_parameters"] = json!([]);
        assert!(!available_free(&entry, slug));
    }

    #[test]
    fn ask_rules_are_read_from_the_agent_list() {
        let listed = r#"[
          {"id": "plan", "permissions": [{"action": "edit", "resource": "*", "effect": "deny"}]},
          {"id": "build", "permissions": [
            {"action": "*", "resource": "*", "effect": "allow"},
            {"action": "read", "resource": "*.env", "effect": "ask"},
            {"action": "read", "resource": "*.env.*", "effect": "ask"},
            {"action": "read", "resource": "*.env.example", "effect": "allow"},
            {"action": "shell", "resource": "git push*", "effect": "ask"},
            {"action": "read", "resource": "*.env", "effect": "ask"}
          ]}
        ]"#;
        assert_eq!(
            parse_ask_rules(listed, "build").unwrap(),
            ["Read *.env", "Read *.env.*", "Run git push*"]
        );
        assert!(parse_ask_rules(listed, "review").is_err());
        assert!(parse_ask_rules("not json", "build").is_err());
        assert_eq!(rule_label("shell", "*"), "Run any command");
    }

    /// Reads this computer's real rules: `cargo test this_computers_ask_rules -- --ignored --nocapture`
    /// Runs a real task on the default free model with live activity:
    /// `cargo test live_activity -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_activity() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("notes.txt"), "The code word is HERON.\n").unwrap();
        let mut task = task(
            folder.path(),
            &find_opencode().unwrap(),
            "Read notes.txt, then run `sleep 3 && echo finished` in the shell, then reply with the code word.",
            false,
        );
        task.live = true;
        task.auto_approve = true;
        let seen = std::sync::Mutex::new(Vec::new());
        let started = Instant::now();
        run(task, &AtomicBool::new(false), |event| {
            let line = match event {
                Event::Activity { doing, since } => format!("now: {doing} (since {since})"),
                Event::Output(text) => format!("out: {}", text.trim_end()),
                Event::Finished(result) => format!("finished: {result:?}"),
                _ => return,
            };
            println!("{:>6.2}s {line}", started.elapsed().as_secs_f32());
            seen.lock().unwrap().push(line);
        });
        let seen = seen.into_inner().unwrap();
        assert!(
            seen.iter()
                .any(|line| line.starts_with("now: Running sleep 3")),
            "{seen:?}"
        );
        assert!(
            seen.iter()
                .any(|line| line.starts_with("out: read · completed · notes.txt")),
            "{seen:?}"
        );
        assert!(seen.iter().any(|line| line.contains("HERON")), "{seen:?}");
        assert_eq!(seen.last().map(String::as_str), Some("finished: Ok(())"));
    }

    /// Sends a message into a running task: `cargo test live_steer -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_steer() {
        let folder = tempfile::tempdir().unwrap();
        let mut task = task(
            folder.path(),
            &find_opencode().unwrap(),
            "Run `sleep 6 && echo first` in the shell, then reply with one short sentence.",
            false,
        );
        task.live = true;
        task.auto_approve = true;
        let reach = std::sync::Mutex::new(None::<Reach>);
        let session = std::sync::Mutex::new(None::<String>);
        let steered = std::sync::Mutex::new(None::<Result<(), String>>);
        let output = std::sync::Mutex::new(String::new());
        run(task, &AtomicBool::new(false), |event| match event {
            Event::Server(found) => *reach.lock().unwrap() = Some(found),
            Event::Session(id) => *session.lock().unwrap() = Some(id),
            Event::Activity { doing, .. } if doing.starts_with("Running sleep 6") => {
                let mut sent = steered.lock().unwrap();
                if sent.is_none() {
                    let reach = reach.lock().unwrap().clone().unwrap();
                    let id = session.lock().unwrap().clone().unwrap();
                    *sent = Some(steer(
                        &reach,
                        &id,
                        "Also: end your reply with the word PELICAN in capitals.",
                    ));
                    println!("steered: {sent:?}");
                }
            }
            Event::Output(text) => output.lock().unwrap().push_str(&text),
            Event::Finished(result) => println!("finished: {result:?}"),
            _ => {}
        });
        let output = output.into_inner().unwrap();
        println!("{output}");
        assert_eq!(steered.into_inner().unwrap(), Some(Ok(())));
        assert!(
            output.contains("PELICAN"),
            "the message didn't reach the task"
        );
    }

    /// Stops a live task while its command runs: `cargo test live_stop -- --ignored`
    #[test]
    #[ignore]
    fn live_stop() {
        let folder = tempfile::tempdir().unwrap();
        let mut task = task(
            folder.path(),
            &find_opencode().unwrap(),
            "Run `sleep 47 && echo finished` in the shell, then reply DONE.",
            false,
        );
        task.live = true;
        task.auto_approve = true;
        let stop = AtomicBool::new(false);
        let outcome = std::sync::Mutex::new(None);
        run(task, &stop, |event| match event {
            Event::Activity { doing, .. } if doing.starts_with("Running sleep 47") => {
                thread::sleep(Duration::from_secs(1));
                stop.store(true, Ordering::Relaxed);
            }
            Event::Finished(result) => *outcome.lock().unwrap() = Some(result),
            _ => {}
        });
        assert_eq!(outcome.into_inner().unwrap(), Some(Err("Stopped".into())));
        let left = Command::new("pgrep")
            .args(["-f", "sleep 47"])
            .output()
            .unwrap();
        assert!(left.stdout.is_empty(), "the command still runs");
    }

    #[test]
    #[ignore]
    fn this_computers_ask_rules() {
        let rules = ask_rules(&find_opencode().unwrap()).unwrap();
        println!("asks before: {rules:?}");
    }

    #[test]
    fn install_folders_follow_the_inherited_path() {
        let home = tempfile::tempdir().unwrap();
        let installed = home.path().join(".opencode").join("bin");
        std::fs::create_dir_all(&installed).unwrap();
        let inherited = env::join_paths([Path::new("/first"), Path::new("/second")]).unwrap();
        let path = child_path(Some(inherited), Some(home.path().into())).unwrap();
        let entries: Vec<PathBuf> = env::split_paths(&path).collect();
        assert_eq!(
            entries[..2],
            [PathBuf::from("/first"), PathBuf::from("/second")]
        );
        assert!(entries.contains(&installed));
        // A folder that does not exist is left out.
        assert!(!entries.contains(&home.path().join(".local").join("bin")));
    }

    #[test]
    fn errors_quote_opencode_and_keep_the_recovery_step() {
        let text = describe(
            "opencode/glm",
            None,
            Some("provider.free-limit"),
            Some("Free usage limit reached."),
        );
        assert!(text.starts_with("opencode/glm reached its free usage limit"));
        assert!(text.ends_with("OpenCode reported: Free usage limit reached."));
        assert!(describe("m", Some("401"), None, None).contains("Check the provider connection"));
        assert_eq!(describe("m", None, None, Some("  ")), "m: unknown error");
    }

    #[test]
    fn stderr_keeps_its_last_lines_without_colour_codes() {
        let mut lines = VecDeque::new();
        for index in 0..DIAGNOSTIC_LINES + 3 {
            remember(&mut lines, &format!("\u{1b}[31mline {index}\u{1b}[0m\n"));
        }
        remember(&mut lines, "   \n");
        assert_eq!(lines.len(), DIAGNOSTIC_LINES);
        assert_eq!(lines.front().unwrap(), "line 3");
        assert_eq!(
            lines.back().unwrap(),
            &format!("line {}", DIAGNOSTIC_LINES + 2)
        );
    }

    #[cfg(unix)]
    fn fixture(script: &str) -> (tempfile::TempDir, PathBuf) {
        use std::{fs, os::unix::fs::PermissionsExt};

        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("opencode");
        fs::write(&binary, script).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        (directory, binary)
    }

    #[cfg(unix)]
    fn task(directory: &Path, binary: &Path, prompt: &str, fallback: bool) -> Task {
        Task {
            directory: directory.canonicalize().unwrap(),
            binary: Some(binary.into()),
            model: BUILTIN.into(),
            prompt: prompt.into(),
            session: None,
            fallback,
            files: Vec::new(),
            backup: BACKUP.into(),
            auto_approve: false,
            time_limit: None,
            variant: None,
            agent: None,
            live: false,
        }
    }

    #[cfg(unix)]
    #[test]
    fn retries_a_429_with_the_same_session_only() {
        use std::cell::RefCell;

        let (directory, binary) = fixture(
            r#"#!/bin/sh
[ "$PWD" = "$(/bin/pwd -P)" ] || exit 2
[ -n "$OPENCODE_CONFIG_CONTENT" ] || exit 3
case "$*" in
  *" --model openrouter/cohere/north-mini-code:free --session fixture-session "*)
    printf '%s\n' '{"type":"text","sessionID":"fixture-session","part":{"text":"Continued ✓"}}'
    ;;
  *"Authentication failure"*)
    printf '%s\n' '{"type":"error","sessionID":"fixture-session","error":{"status":401}}'
    exit 1
    ;;
  *"Limit without a status"*)
    printf '%s\n' '{"type":"error","sessionID":"fixture-session","error":{"type":"provider.rate-limit","message":"Slow down"}}'
    exit 1
    ;;
  *"Crash"*)
    echo "config file is not valid JSON" >&2
    exit 7
    ;;
  *)
    printf '%s\n' '{"type":"error","sessionID":"fixture-session","error":{"status":429}}'
    exit 1
    ;;
esac
"#,
        );
        let task = |prompt: &str, fallback: bool| task(directory.path(), &binary, prompt, fallback);
        let stop = AtomicBool::new(false);
        let output = RefCell::new(String::new());
        let send = |event| {
            if let Event::Output(line) = event {
                output.borrow_mut().push_str(&line);
            }
        };
        let check = |model: &str| {
            assert_eq!(model, BACKUP);
            Ok(())
        };

        run_task(task("First request", true), &stop, &send, check).unwrap();
        assert!(output.borrow().contains("Continued ✓"));
        output.borrow_mut().clear();
        run_task(task("Limit without a status", true), &stop, &send, check).unwrap();
        assert!(output.borrow().contains("Continued ✓"));

        let no_check = |_: &str| panic!("Fallback must not run");
        assert!(
            run_task(task("Manual switch", false), &stop, &send, no_check)
                .unwrap_err()
                .contains("429")
        );
        assert!(
            run_task(task("Authentication failure", true), &stop, &send, no_check)
                .unwrap_err()
                .contains("401")
        );
        let limited = run_task(
            task("Limit without a status", false),
            &stop,
            &send,
            no_check,
        )
        .unwrap_err();
        assert!(limited.contains("OpenCode reported: Slow down"));
        let crash = run_task(task("Crash", true), &stop, &send, no_check).unwrap_err();
        assert!(crash.contains("config file is not valid JSON"), "{crash}");
    }

    #[cfg(unix)]
    #[test]
    fn attachments_are_passed_as_file_flags() {
        let (directory, binary) = fixture(
            r#"#!/bin/sh
expected="--file $(dirname "$0")/notes.txt --file $(dirname "$0")/shot.png -- Read them"
case "$*" in
  *"$expected") printf '%s\n' '{"type":"text","sessionID":"s","part":{"text":"Both files"}}' ;;
  *) echo "unexpected: $*" >&2; exit 4 ;;
esac
"#,
        );
        let mut task = task(directory.path(), &binary, "Read them", false);
        task.files = vec![
            directory.path().join("notes.txt"),
            directory.path().join("shot.png"),
        ];
        let output = std::cell::RefCell::new(String::new());
        let result = run_task(
            task,
            &AtomicBool::new(false),
            &|event| {
                if let Event::Output(text) = event {
                    output.borrow_mut().push_str(&text);
                }
            },
            |_: &str| Ok(()),
        );
        assert_eq!(result, Ok(()));
        assert!(output.borrow().contains("Both files"));
    }

    #[cfg(unix)]
    #[test]
    fn effort_and_agent_reach_opencode_and_the_backup_drops_effort() {
        let (directory, binary) = fixture(
            r#"#!/bin/sh
case "$*" in
  *" --model opencode/glm-5.3-flash-free#high --agent plan "*)
    printf '%s\n' '{"type":"error","sessionID":"s","error":{"status":429}}'
    exit 1 ;;
  *" --model openrouter/cohere/north-mini-code:free --agent plan --session s "*)
    printf '%s\n' '{"type":"text","sessionID":"s","part":{"text":"backup on its default"}}' ;;
  *) echo "unexpected: $*" >&2; exit 4 ;;
esac
"#,
        );
        let mut planned = task(directory.path(), &binary, "Plan it", true);
        planned.variant = Some("high".into());
        planned.agent = Some("plan".into());
        let output = std::cell::RefCell::new(String::new());
        let result = run_task(
            planned,
            &AtomicBool::new(false),
            &|event| {
                if let Event::Output(text) = event {
                    output.borrow_mut().push_str(&text);
                }
            },
            |_: &str| Ok(()),
        );
        assert_eq!(result, Ok(()), "{}", output.borrow());
        assert!(output.borrow().contains("backup on its default"));
    }

    #[cfg(unix)]
    #[test]
    fn time_limit_and_auto_approval_reach_opencode() {
        let (directory, binary) = fixture(
            r#"#!/bin/sh
case "$*" in
  *" --auto "*) printf '%s\n' '{"type":"text","sessionID":"s","part":{"text":"auto on"}}' ;;
esac
while :; do sleep 0.05; done
"#,
        );
        let mut limited = task(directory.path(), &binary, "Long task", false);
        limited.auto_approve = true;
        // Long enough for the script to start while the other tests run in parallel.
        limited.time_limit = Some(Duration::from_millis(1500));
        let output = std::cell::RefCell::new(String::new());
        let result = run_task(
            limited,
            &AtomicBool::new(false),
            &|event| {
                if let Event::Output(text) = event {
                    output.borrow_mut().push_str(&text);
                }
            },
            |_: &str| Ok(()),
        );
        let output = output.into_inner();
        assert!(
            output.contains("auto on"),
            "output: {output:?}, result: {result:?}"
        );
        assert!(result.unwrap_err().starts_with("Stopped after 0 minutes"));
    }

    #[cfg(unix)]
    #[test]
    fn stop_lets_opencode_finish_before_killing_it() {
        let (directory, binary) = fixture(
            r#"#!/bin/sh
trap 'echo saved > "$(dirname "$0")/saved"; exit 0' TERM
printf '%s\n' '{"type":"text","sessionID":"fixture-session","part":{"text":"Working"}}'
while :; do sleep 0.05; done
"#,
        );
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        let result = run_task(
            task(directory.path(), &binary, "Long task", false),
            &stop,
            &|event| {
                if matches!(event, Event::Output(_)) {
                    stop.store(true, Ordering::Relaxed);
                }
            },
            |_: &str| Ok(()),
        );
        assert_eq!(result.unwrap_err(), "Stopped");
        assert!(directory.path().join("saved").is_file());
        assert!(started.elapsed() < STOP_GRACE);
    }
}
