//! Application state and what happens to it. Drawing lives in `views`.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use eframe::egui::{self, Key, Modifiers};
use uuid::Uuid;

use crate::{
    attach,
    inputs::{self, Dictation, Inputs, ModelState},
    models, providers,
    runner::{self, Event, Task},
    speech,
    storage::{Appearance, Session, Settings, Store, Turn, Usage},
    theme,
};

pub const NAME: &str = "Super OpenCode";

/// What the main area shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Chat,
    Usage,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ready,
    Running,
    Stopping,
    Complete,
    Stopped,
    Failed,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Running => "Running",
            Self::Stopping => "Stopping",
            Self::Complete => "Complete",
            Self::Stopped => "Stopped",
            Self::Failed => "Run failed",
        }
    }
}

/// The composer's menus, for screenshots that need one drawn open.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub enum Menu {
    Add,
    Access,
    Model,
    Context,
    More,
}

/// What a look for models found: the free models, and which providers have keys.
pub struct Discovered {
    models: Result<Vec<models::Model>, String>,
    connections: Result<Vec<providers::Connection>, String>,
}

/// A finished connect or disconnect, and what OpenCode offers afterwards.
pub struct ProviderOutcome {
    provider: &'static str,
    result: Result<Discovered, String>,
}

fn look(binary: &std::path::Path) -> Discovered {
    Discovered {
        models: models::discover(binary),
        connections: providers::connections(binary),
    }
}

pub struct Running {
    receiver: mpsc::Receiver<Event>,
    stop: Arc<AtomicBool>,
    worker: thread::JoinHandle<()>,
    /// The task's private OpenCode server, once it has started.
    reach: Option<runner::Reach>,
}

/// A message on its way into a running task: its session, its text, and whether it arrived.
type Steering = mpsc::Receiver<(Uuid, String, Result<(), String>)>;

/// A message waiting for its session's task to finish.
#[derive(Clone)]
pub(crate) struct Queued {
    pub(crate) prompt: String,
    pub(crate) attachments: Vec<attach::Sent>,
}

/// A session's task while that session isn't the one on screen: still running, or finished
/// with a result not seen yet. Opening the session brings it back on screen.
/// Where an unsent prompt is kept while its session is out of view.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Draft {
    Session(Uuid),
    /// A new session not sent yet, in this project.
    New(Option<PathBuf>),
}

pub(crate) struct Parked {
    running: Option<Running>,
    pub(crate) status: Status,
    task_error: Option<String>,
    limited: bool,
    pub(crate) doing: Option<(String, i64)>,
    task_started: i64,
    awake: Option<crate::awake::Awake>,
    /// Screenshots show a running task without a process behind it.
    #[cfg(debug_assertions)]
    staged: bool,
}

impl Parked {
    pub(crate) fn is_running(&self) -> bool {
        #[cfg(debug_assertions)]
        if self.staged {
            return true;
        }
        self.running.is_some()
    }

    /// A parked task for screenshots: running with `doing`, or finished with `status`.
    #[cfg(debug_assertions)]
    pub(crate) fn staged(status: Status, doing: Option<&str>) -> Self {
        Self {
            running: None,
            status,
            task_error: None,
            limited: false,
            doing: doing.map(|doing| (doing.to_owned(), crate::runner::now_millis() - 20_000)),
            task_started: crate::runner::now_millis() - 60_000,
            awake: None,
            staged: doing.is_some(),
        }
    }
}

pub struct App {
    pub(crate) logo: egui::TextureHandle,
    pub(crate) store: Option<Store>,
    pub(crate) settings: Settings,
    pub(crate) sessions: Vec<Session>,
    pub(crate) active: Option<Session>,
    pub(crate) route: usize,
    pub(crate) prompt: String,
    pub(crate) status: Status,
    pub(crate) task_error: Option<String>,
    pub(crate) storage_error: Option<String>,
    /// The last run stopped on a rate or usage limit, so continuing on the backup is offered.
    pub(crate) limited: bool,
    pub(crate) running: Option<Running>,
    pub(crate) view: View,
    /// Today's usage, read when it changes instead of on every frame.
    pub(crate) usage: Result<Vec<Usage>, String>,
    /// The OpenCode executable found on this computer, when Settings names none.
    pub(crate) detected: Option<PathBuf>,
    /// Puts the keyboard in the composer on the next frame.
    pub(crate) focus_composer: bool,
    /// Lets screenshots show the running state without a process behind it.
    pub(crate) staged_running: bool,
    /// What the running task is doing, and since when (milliseconds since the Unix epoch).
    pub(crate) doing: Option<(String, i64)>,
    /// When the running task started, in milliseconds since the Unix epoch.
    pub(crate) task_started: i64,
    /// Usage over time for the Usage page and the start screen, once read.
    pub(crate) history: Option<Result<Arc<crate::history::History>, String>>,
    history_task: Option<mpsc::Receiver<Result<crate::history::History, String>>>,
    /// Another read is wanted once the current one finishes.
    history_stale: bool,
    /// Days the Usage page's chart and top sessions cover: 7, 30, or 90.
    pub(crate) usage_days: u64,
    /// The year the activity grid shows. `None` shows the last twelve months.
    pub(crate) usage_year: Option<i32>,
    /// Attachments, dictation, the speech model, and reading aloud.
    pub(crate) inputs: Inputs,
    /// Scrolls the settings page to this offset once.
    pub(crate) scroll_settings_to: Option<f32>,
    /// The Settings category shown, and what its search box holds.
    pub(crate) settings_section: crate::views::Section,
    pub(crate) settings_query: String,
    /// Runs OpenCode's plan agent, which can't edit files.
    pub(crate) plan_mode: bool,
    /// The Full access confirmation is open.
    pub(crate) confirm_full_access: bool,
    /// Keeps the computer awake while a task runs, when Settings asks for it.
    pub(crate) awake: Option<crate::awake::Awake>,
    /// Project groups in the sidebar opened or closed by hand, and those showing every session.
    pub(crate) sidebar_toggled: std::collections::HashSet<PathBuf>,
    pub(crate) sidebar_all: std::collections::HashSet<PathBuf>,
    /// The tutorial is open at this page.
    pub(crate) tutorial: Option<usize>,
    /// OpenCode being installed in the background, and how the last try went.
    installing: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    pub(crate) install_failed: Option<String>,
    /// Messages waiting for their session's task to finish, by session.
    pub(crate) queued: std::collections::HashMap<Uuid, Vec<Queued>>,
    /// Sessions whose queue waits because their last task failed or was stopped.
    pub(crate) held: std::collections::HashSet<Uuid>,
    /// Messages on their way into running tasks.
    steering: Vec<Steering>,
    /// Unsent prompts and attachments of sessions out of view.
    pub(crate) drafts: std::collections::HashMap<Draft, (String, Vec<inputs::Pending>)>,
    /// Tasks of sessions that aren't on screen, by session.
    pub(crate) parked: std::collections::HashMap<Uuid, Parked>,
    /// The in-app browser's panel, while open.
    pub(crate) browser: Option<crate::browser::Browser>,
    /// The composer is too narrow for its controls' labels.
    pub(crate) narrow: bool,
    /// The side pane that is open, and whether it fills the window.
    pub(crate) pane: Option<crate::panes::Pane>,
    pub(crate) pane_wide: bool,
    /// Shells in the terminal dock, the one shown, and whether the dock is open.
    pub(crate) terminals: Vec<crate::terminal::Terminal>,
    pub(crate) terminal_tab: usize,
    pub(crate) terminal_open: bool,
    pub(crate) terminal_error: Option<String>,
    /// The terminal had the keyboard last frame, so Esc and shortcuts go to it.
    pub(crate) terminal_focused: bool,
    pub(crate) changes: crate::panes::Changes,
    pub(crate) files: crate::panes::Files,
    pub(crate) todos: crate::panes::Todos,
    /// The requests Full access would approve, read from OpenCode when the confirmation opens.
    pub(crate) ask_rules: Option<Result<Vec<String>, String>>,
    ask_rules_task: Option<mpsc::Receiver<Result<Vec<String>, String>>>,
    /// Draws one of the composer's menus open, for screenshots.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) preview_menu: Option<Menu>,
    /// OpenCode's catalog is being read for free models.
    pub(crate) discovery: Option<mpsc::Receiver<Discovered>>,
    pub(crate) discovery_started: bool,
    /// What the last look for models found, or why it failed.
    pub(crate) discovery_status: String,
    /// Providers OpenCode holds keys for.
    pub(crate) connections: Vec<providers::Connection>,
    /// The provider whose key box is open in Settings.
    pub(crate) connecting: Option<&'static str>,
    /// The key being typed. Cleared as soon as it is handed to OpenCode.
    pub(crate) key_input: String,
    /// A key being saved or removed in the background.
    pub(crate) provider_task: Option<mpsc::Receiver<ProviderOutcome>>,
    /// The result of the last connect or disconnect, and whether it failed.
    pub(crate) provider_message: Option<(String, bool)>,
    #[cfg(debug_assertions)]
    pub(crate) preview: Option<crate::preview::Preview>,
    /// Debug builds: shows every working sign at several moments, for screenshots.
    #[cfg(debug_assertions)]
    pub(crate) preview_signs: bool,
}

pub fn launch(
    project: Option<PathBuf>,
    preview: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let logo = image::load_from_memory(include_bytes!("../assets/logo.png"))?.to_rgba8();
    let (width, height) = logo.dimensions();
    // The interface shows the mark at 22 to 44 points. A texture near that size stays crisp
    // where the full-size artwork would shimmer.
    let small = image::imageops::resize(&logo, 96, 96, image::imageops::FilterType::Lanczos3);
    let source = egui::ColorImage::from_rgba_unmultiplied([96, 96], small.as_raw());
    let icon = egui::IconData {
        rgba: logo.into_raw(),
        width,
        height,
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1100.0, 720.0])
        .with_min_inner_size([780.0, 520.0])
        .with_icon(icon);
    if cfg!(target_os = "macos") {
        // The sidebar runs to the top of the window, under the window buttons.
        viewport = viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        NAME,
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            let logo = cc.egui_ctx.load_texture(
                "Super OpenCode logo",
                source,
                egui::TextureOptions::LINEAR,
            );
            #[cfg(debug_assertions)]
            if let Some(folder) = preview {
                return Ok(Box::new(App::for_preview(logo, folder)));
            }
            #[cfg(not(debug_assertions))]
            let _ = preview;
            let app = App::new(logo, project);
            app.apply_appearance(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}

impl App {
    pub(crate) fn empty(logo: egui::TextureHandle) -> Self {
        Self {
            logo,
            store: None,
            settings: Settings::default(),
            sessions: Vec::new(),
            active: None,
            route: 0,
            prompt: String::new(),
            status: Status::Ready,
            task_error: None,
            storage_error: None,
            limited: false,
            running: None,
            view: View::Chat,
            usage: Ok(Vec::new()),
            detected: None,
            focus_composer: true,
            staged_running: false,
            doing: None,
            task_started: 0,
            history: None,
            history_task: None,
            history_stale: false,
            usage_days: 30,
            usage_year: None,
            inputs: Inputs::default(),
            scroll_settings_to: None,
            settings_section: crate::views::Section::Models,
            settings_query: String::new(),
            plan_mode: false,
            confirm_full_access: false,
            tutorial: None,
            parked: Default::default(),
            drafts: Default::default(),
            queued: Default::default(),
            held: Default::default(),
            steering: Vec::new(),
            installing: None,
            install_failed: None,
            sidebar_toggled: Default::default(),
            sidebar_all: Default::default(),
            awake: None,
            browser: None,
            narrow: false,
            pane: None,
            pane_wide: false,
            terminals: Vec::new(),
            terminal_tab: 0,
            terminal_open: false,
            terminal_error: None,
            terminal_focused: false,
            changes: Default::default(),
            files: Default::default(),
            todos: Default::default(),
            ask_rules: None,
            ask_rules_task: None,
            preview_menu: None,
            discovery: None,
            discovery_started: false,
            discovery_status: String::new(),
            connections: Vec::new(),
            connecting: None,
            key_input: String::new(),
            provider_task: None,
            provider_message: None,
            #[cfg(debug_assertions)]
            preview: None,
            #[cfg(debug_assertions)]
            preview_signs: false,
        }
    }

    fn new(logo: egui::TextureHandle, project: Option<PathBuf>) -> Self {
        let mut app = Self::empty(logo);
        match Store::open() {
            Ok(store) => {
                match store.settings() {
                    Ok(settings) => app.settings = settings,
                    Err(error) => {
                        app.storage_error = Some(format!("Couldn’t load settings: {error}"))
                    }
                }
                match store.sessions() {
                    Ok(sessions) => app.sessions = sessions,
                    Err(error) => {
                        app.storage_error = Some(format!("Couldn’t load saved sessions: {error}"))
                    }
                }
                app.store = Some(store);
            }
            Err(error) => {
                app.storage_error = Some(format!("Local storage is unavailable: {error}"))
            }
        }
        if let Some(directory) = project.or_else(|| app.settings.project.clone()) {
            app.choose_project(directory);
        }
        app.route = app.default_route();
        app.detected = runner::find_opencode();
        // Without OpenCode nothing can run, so it is installed straight away.
        if app.detected.is_none() && app.settings.executable.is_none() {
            app.install_opencode();
        }
        if !app.settings.tutorial_seen {
            app.tutorial = Some(0);
        }
        // Debug builds can open a page in the in-app browser at launch, for checking it.
        #[cfg(debug_assertions)]
        if let Ok(url) = std::env::var("SUPER_OPENCODE_BROWSE") {
            app.browser = Some(crate::browser::Browser::new(Some(url)));
            app.pane = Some(crate::panes::Pane::Browser);
        }
        app.refresh_usage();
        let data = app.data_dir();
        if let Some(model) = speech::installed(&data) {
            app.inputs.model = ModelState::Ready(model);
        }
        thread::spawn(move || inputs::prune(&data));
        app
    }

    pub(crate) fn apply_appearance(&self, ctx: &egui::Context) {
        ctx.set_theme(match self.settings.appearance {
            Appearance::System => egui::ThemePreference::System,
            Appearance::Light => egui::ThemePreference::Light,
            Appearance::Dark => egui::ThemePreference::Dark,
        });
        ctx.set_zoom_factor(self.settings.scale.factor());
    }

    /// Where new sessions start: the default model from Settings.
    pub(crate) fn default_route(&self) -> usize {
        models::index_of(&self.settings.default_model).unwrap_or_default()
    }

    /// The backup model's position in the list, when it is one this app offers.
    pub(crate) fn backup_route(&self) -> Option<usize> {
        models::index_of(&self.settings.backup_model)
    }

    /// Whether a rate limit on the selected model can continue on the backup.
    pub(crate) fn can_fall_back(&self) -> bool {
        self.backup_route()
            .is_some_and(|backup| backup != self.route)
    }

    /// Whether the session on screen has a task running.
    pub(crate) fn is_running(&self) -> bool {
        self.running.is_some() || self.staged_running
    }

    /// Tasks running now, on screen or not.
    pub(crate) fn tasks_running(&self) -> usize {
        usize::from(self.is_running())
            + self
                .parked
                .values()
                .filter(|parked| parked.is_running())
                .count()
    }

    /// Whether another task may start now, under the limit set in Settings.
    pub(crate) fn room_for_task(&self) -> bool {
        self.tasks_running() < usize::from(self.settings.max_tasks.max(1))
    }

    /// Moves the session on screen, and its task if one is running or has news, out of view
    /// so another can be opened. The session stays in the sidebar.
    fn park_active(&mut self) {
        let Some(session) = self.active.take() else {
            return;
        };
        let id = session.id;
        self.sessions.retain(|saved| saved.id != id);
        let at = self
            .sessions
            .iter()
            .position(|saved| saved.updated < session.updated)
            .unwrap_or(self.sessions.len());
        self.sessions.insert(at, session);
        if self.running.is_some() {
            self.parked.insert(
                id,
                Parked {
                    running: self.running.take(),
                    status: self.status,
                    task_error: self.task_error.take(),
                    limited: self.limited,
                    doing: self.doing.take(),
                    task_started: self.task_started,
                    awake: self.awake.take(),
                    #[cfg(debug_assertions)]
                    staged: false,
                },
            );
        }
    }

    /// Brings a session's parked task back on screen.
    fn unpark(&mut self, id: Uuid) {
        if let Some(parked) = self.parked.remove(&id) {
            self.running = parked.running;
            self.status = parked.status;
            self.task_error = parked.task_error;
            self.limited = parked.limited;
            self.doing = parked.doing;
            self.task_started = parked.task_started;
            self.awake = parked.awake;
        }
    }

    /// The executable a task would use: the one chosen in Settings, else the one found.
    pub(crate) fn opencode(&self) -> Option<&PathBuf> {
        self.settings.executable.as_ref().or(self.detected.as_ref())
    }

    pub(crate) fn refresh_usage(&mut self) {
        self.usage = match &self.store {
            Some(store) => store
                .usage_today()
                .map_err(|error| format!("Couldn’t read usage: {error}")),
            None => Err("Local storage is unavailable.".into()),
        };
    }

    pub(crate) fn open(&mut self, view: View) {
        if view == View::Usage {
            self.refresh_usage();
            self.load_history();
        }
        if view == View::Settings {
            self.detected = runner::find_opencode();
        }
        self.view = view;
    }

    pub(crate) fn pick_project(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Choose a project folder")
            .pick_folder()
        {
            self.choose_project(path);
        }
    }

    pub(crate) fn choose_project(&mut self, path: PathBuf) {
        match runner::canonical_project(&path) {
            Ok(directory) => {
                if self.settings.project.as_ref() != Some(&directory) {
                    // The draft belongs to the project it was written in.
                    self.stash_draft();
                    self.park_active();
                    self.settings.project = Some(directory);
                    self.save_settings();
                    self.clear_view();
                    self.restore_draft();
                }
            }
            Err(error) => {
                self.settings.project = None;
                self.task_error = Some(error.to_string());
            }
        }
    }

    pub(crate) fn save_settings(&mut self) {
        if let Some(store) = &self.store
            && let Err(error) = store.save_settings(&self.settings)
        {
            self.storage_error = Some(format!("Couldn’t save settings: {error}"));
        }
    }

    /// Starts a new session in the current project.
    /// Opens a new session, keeping what was typed in the one before for when it is opened
    /// again.
    pub(crate) fn reset(&mut self) {
        self.stash_draft();
        self.park_active();
        self.clear_view();
        self.restore_draft();
    }

    /// Where a draft is kept: with its session, or with its project for a new session.
    fn draft_key(&self) -> Draft {
        match &self.active {
            Some(session) => Draft::Session(session.id),
            None => Draft::New(self.settings.project.clone()),
        }
    }

    /// Puts the unsent prompt and attachments aside for the session on screen.
    fn stash_draft(&mut self) {
        let key = self.draft_key();
        let prompt = std::mem::take(&mut self.prompt);
        let pending = std::mem::take(&mut self.inputs.pending);
        if prompt.trim().is_empty() && pending.is_empty() {
            self.drafts.remove(&key);
        } else {
            self.drafts.insert(key, (prompt, pending));
        }
    }

    /// Brings back what was being written in the session now on screen.
    fn restore_draft(&mut self) {
        if let Some((prompt, pending)) = self.drafts.remove(&self.draft_key()) {
            self.prompt = prompt;
            self.inputs.pending = pending;
        }
    }

    /// Empties the screen for another session. Drafts and tasks are put aside first.
    fn clear_view(&mut self) {
        self.running = None;
        self.doing = None;
        self.awake = None;
        self.active = None;
        self.prompt.clear();
        self.inputs.pending.clear();
        self.task_error = None;
        self.limited = false;
        self.route = self.default_route();
        self.status = Status::Ready;
        self.view = View::Chat;
        self.focus_composer = true;
    }

    pub(crate) fn select_session(&mut self, session: Session) {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.id == session.id)
        {
            self.view = View::Chat;
            return;
        }
        self.stash_draft();
        self.park_active();
        self.clear_view();
        // The sidebar's copy is the newest: a parked task writes its output there.
        let session = self
            .sessions
            .iter()
            .find(|saved| saved.id == session.id)
            .cloned()
            .unwrap_or(session);
        self.route = models::index_of(&session.route_id).unwrap_or_else(|| self.default_route());
        let id = session.id;
        self.active = Some(session);
        self.unpark(id);
        self.restore_draft();
    }

    /// Asks OpenCode, in the background, which free models it can use right now.
    pub(crate) fn start_discovery(&mut self, ctx: &egui::Context) {
        self.discovery_started = true;
        let Some(binary) = self.opencode().cloned() else {
            self.discovery_status =
                "OpenCode was not found, so only the built-in models are listed.".into();
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(look(&binary));
            repaint.request_repaint();
        });
        self.discovery = Some(receiver);
        self.discovery_status = "Looking for free models…".into();
    }

    /// Asks before turning on Full access. Standard access turns on straight away.
    pub(crate) fn choose_access(&mut self, ctx: &egui::Context, full: bool) {
        if !full {
            self.settings.auto_approve = false;
            self.save_settings();
            return;
        }
        if self.settings.auto_approve {
            return;
        }
        self.confirm_full_access = true;
        if self.ask_rules.is_some() || self.ask_rules_task.is_some() {
            return;
        }
        let Some(binary) = self.opencode().cloned() else {
            self.ask_rules = Some(Err("OpenCode was not found.".into()));
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let _ = sender.send(runner::ask_rules(&binary));
            repaint.request_repaint();
        });
        self.ask_rules_task = Some(receiver);
    }

    /// Closes the confirmation, turning Full access on when `accepted`.
    pub(crate) fn answer_full_access(&mut self, accepted: bool) {
        self.confirm_full_access = false;
        if accepted {
            self.settings.auto_approve = true;
            self.save_settings();
        }
    }

    /// Sends web links opened this frame to the in-app browser, when Settings asks for it.
    /// Other links, and every link when it is off, open in the default browser.
    fn route_links(&mut self, ctx: &egui::Context) {
        if !crate::browser::SUPPORTED || !self.settings.links_in_app {
            return;
        }
        let urls = ctx.output_mut(|output| {
            let mut urls = Vec::new();
            output.commands.retain(|command| match command {
                egui::OutputCommand::OpenUrl(open) if crate::browser::is_web(&open.url) => {
                    urls.push(open.url.clone());
                    false
                }
                _ => true,
            });
            urls
        });
        if let Some(url) = urls.into_iter().last() {
            self.open_in_app(url);
            ctx.request_repaint();
        }
    }

    /// With the setting on, shows HTML files OpenCode just wrote or edited in the browser.
    fn open_written_pages(&mut self, text: &str) {
        if !self.settings.auto_open_html || !crate::browser::SUPPORTED {
            return;
        }
        let Some(project) = self.settings.project.clone() else {
            return;
        };
        for line in text.lines() {
            let Some(notice) = crate::activity::parse(line) else {
                continue;
            };
            let wrote = matches!(notice.tool, "write" | "edit" | "apply_patch" | "multiedit");
            let html = notice.detail.ends_with(".html") || notice.detail.ends_with(".htm");
            if wrote && html && !notice.failed() {
                let path = project.join(notice.detail);
                if path.is_file() {
                    self.open_file_in_browser(&path);
                }
            }
        }
    }

    /// Shows `url` in the in-app browser, opening its pane if needed.
    pub(crate) fn open_in_app(&mut self, url: String) {
        self.browser
            .get_or_insert_with(|| crate::browser::Browser::new(None))
            .open(url);
        self.pane = Some(crate::panes::Pane::Browser);
    }

    /// Installs OpenCode in the background with its own installer.
    pub(crate) fn install_opencode(&mut self) {
        if self.installing.is_some() {
            return;
        }
        self.install_failed = None;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(crate::install::install_opencode());
        });
        self.installing = Some(receiver);
    }

    pub(crate) fn is_installing(&self) -> bool {
        self.installing.is_some()
    }

    fn poll_install(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.installing else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(path)) => {
                self.installing = None;
                self.detected = Some(path);
                self.start_discovery(ctx);
            }
            Ok(Err(message)) => {
                self.installing = None;
                self.install_failed = Some(message);
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            Err(mpsc::TryRecvError::Disconnected) => self.installing = None,
        }
    }

    /// Reads usage over time on another thread. OpenCode's database can take a moment.
    pub(crate) fn load_history(&mut self) {
        if self.history_task.is_some() {
            self.history_stale = true;
            return;
        }
        let source = self.settings.usage_source;
        let file = match &self.store {
            Some(store) => store
                .usage_file()
                .map_err(|error| format!("Couldn’t read usage: {error}")),
            None => Err("Local storage is unavailable.".to_owned()),
        };
        let binary = self.opencode().cloned();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let result = file.and_then(|file| {
                let opencode = match source {
                    crate::history::Source::OpenCode => {
                        crate::history::opencode_database(binary.as_deref())
                    }
                    crate::history::Source::App => None,
                };
                crate::history::History::load(source, &file, opencode.as_deref())
            });
            let _ = sender.send(result);
        });
        self.history_task = Some(receiver);
    }

    fn poll_history(&mut self, ctx: &egui::Context) {
        let Some(receiver) = &self.history_task else {
            return;
        };
        match receiver.try_recv() {
            Ok(result) => {
                self.history_task = None;
                // A read for another source is out of date as soon as it lands.
                let current = result
                    .as_ref()
                    .map_or(true, |history| history.source == self.settings.usage_source);
                if current {
                    self.history = Some(result.map(Arc::new));
                }
                if self.history_stale || !current {
                    self.history_stale = false;
                    self.load_history();
                }
                ctx.request_repaint();
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Err(mpsc::TryRecvError::Disconnected) => self.history_task = None,
        }
    }

    fn poll_ask_rules(&mut self) {
        if let Some(result) = self
            .ask_rules_task
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.ask_rules_task = None;
            self.ask_rules = Some(result);
        }
    }

    /// Opens the key box for `provider` in Settings.
    pub(crate) fn begin_connect(&mut self, provider: &'static str) {
        self.key_input.clear();
        self.provider_message = None;
        self.connecting = Some(provider);
        self.open(View::Settings);
        self.settings_section = crate::views::Section::Providers;
        self.settings_query.clear();
        self.scroll_settings_to = Some(0.0);
    }

    pub(crate) fn cancel_connect(&mut self) {
        self.key_input.clear();
        self.connecting = None;
    }

    /// Hands the typed key to OpenCode, then looks for models again.
    pub(crate) fn save_key(&mut self, ctx: &egui::Context) {
        let Some(provider) = self.connecting else {
            return;
        };
        let Some(binary) = self.opencode().cloned() else {
            self.provider_message = Some(("OpenCode was not found.".into(), true));
            return;
        };
        let key = std::mem::take(&mut self.key_input);
        self.connecting = None;
        self.run_provider_task(ctx, provider, move || {
            providers::connect(&binary, provider, &key)?;
            Ok(look(&binary))
        });
    }

    /// Removes the key this app stored for `provider`.
    pub(crate) fn disconnect(&mut self, ctx: &egui::Context, provider: &'static str) {
        let Some(binary) = self.opencode().cloned() else {
            return;
        };
        self.run_provider_task(ctx, provider, move || {
            providers::disconnect(&binary, provider)?;
            Ok(look(&binary))
        });
    }

    fn run_provider_task(
        &mut self,
        ctx: &egui::Context,
        provider: &'static str,
        work: impl FnOnce() -> Result<Discovered, String> + Send + 'static,
    ) {
        let (sender, receiver) = mpsc::channel();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let outcome = ProviderOutcome {
                provider,
                result: work(),
            };
            let _ = sender.send(outcome);
            repaint.request_repaint();
        });
        self.provider_task = Some(receiver);
        self.provider_message = None;
    }

    pub(crate) fn is_connected(&self, provider: &str) -> Option<&providers::Connection> {
        self.connections
            .iter()
            .find(|connection| connection.provider == provider)
    }

    fn poll_providers(&mut self) {
        let Some(outcome) = self
            .provider_task
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        else {
            return;
        };
        self.provider_task = None;
        let name = providers::find(outcome.provider).map_or(outcome.provider, |p| p.name);
        match outcome.result {
            Err(message) => {
                self.provider_message = Some((message, true));
                // Reopen the key box so the key can be corrected.
                self.connecting = Some(outcome.provider);
            }
            Ok(found) => {
                self.apply_discovered(found);
                let count = models::all()
                    .iter()
                    .filter(|model| model.provider == outcome.provider)
                    .count();
                let message = if self.is_connected(outcome.provider).is_none() {
                    format!("{name} is disconnected and its models were removed.")
                } else if count > 0 {
                    format!("{name} is connected. {count} free models are in the picker.")
                } else {
                    format!(
                        "{name} is connected, but OpenCode lists no free models from it. Check the key, then choose Look for Models Again."
                    )
                };
                self.provider_message = Some((message, false));
            }
        }
    }

    fn poll_discovery(&mut self) {
        let Some(found) = self
            .discovery
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        else {
            return;
        };
        self.discovery = None;
        self.apply_discovered(found);
    }

    fn apply_discovered(&mut self, found: Discovered) {
        if let Ok(connections) = found.connections {
            self.connections = connections;
        }
        match found.models {
            Ok(models_found) => {
                let selected = models::at(self.route).id;
                models::set_discovered(models_found);
                self.route = models::index_of(&selected).unwrap_or_else(|| self.default_route());
                if self.active.is_none() && self.status == Status::Ready {
                    self.route = self.default_route();
                }
                let list = models::all();
                let providers = models::providers(&list).len();
                self.discovery_status = format!(
                    "{} free models from {providers} providers. Checked at {}.",
                    list.len(),
                    chrono::Local::now().format("%H:%M")
                );
            }
            Err(message) => {
                self.discovery_status = format!("{message} The built-in models are listed.")
            }
        }
    }

    /// The effort level chosen for `model`, when the model still offers it.
    pub(crate) fn effort_of(&self, model: &models::Model) -> Option<String> {
        self.settings
            .efforts
            .get(&model.id)
            .filter(|variant| model.variants.contains(variant))
            .cloned()
    }

    /// Chooses an effort level for `model`. `None` goes back to the model's default.
    pub(crate) fn set_effort(&mut self, model: &str, variant: Option<String>) {
        match variant {
            Some(variant) => self.settings.efforts.insert(model.to_owned(), variant),
            None => self.settings.efforts.remove(model),
        };
        self.save_settings();
    }

    /// What Auto would pick for the prompt being written, when Auto is on.
    pub(crate) fn auto_pick(&self) -> Option<crate::auto::Pick> {
        let kinds: Vec<_> = self.inputs.pending.iter().map(|item| item.kind).collect();
        self.pick_for(self.prompt.trim(), &kinds)
    }

    /// What Auto picks for `prompt` with these attachments, when Auto is on.
    fn pick_for(&self, prompt: &str, kinds: &[attach::Kind]) -> Option<crate::auto::Pick> {
        (self.settings.model_mode == crate::auto::Mode::Auto)
            .then(|| crate::auto::pick(prompt, kinds, self.plan_mode, &self.settings.auto_models))
    }

    /// Whether a message can join the task on screen now: it runs, its server answers, and
    /// OpenCode has named the session.
    pub(crate) fn can_steer(&self) -> bool {
        self.running.as_ref().is_some_and(|run| run.reach.is_some())
            && self
                .active
                .as_ref()
                .is_some_and(|session| session.server_session.is_some())
    }

    /// Sends what the composer holds into the running task. OpenCode gives it to the model
    /// at its next step. Attachments can't go this way, so with any they wait in the queue.
    pub(crate) fn send_now(&mut self, ctx: &egui::Context) {
        let text = self.prompt.trim().to_owned();
        if text.is_empty() {
            return;
        }
        if !self.can_steer() || !self.inputs.pending.is_empty() {
            self.queue_prompt();
            return;
        }
        let (Some(reach), Some(session)) = (
            self.running.as_ref().and_then(|run| run.reach.clone()),
            self.active
                .as_ref()
                .and_then(|session| session.server_session.clone()),
        ) else {
            return;
        };
        let Some(id) = self.active.as_ref().map(|session| session.id) else {
            return;
        };
        self.prompt.clear();
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = runner::steer(&reach, &session, &text);
            let _ = sender.send((id, text, result));
            ctx.request_repaint();
        });
        self.steering.push(receiver);
    }

    /// Puts what the composer holds in this session's queue, to run when its task finishes.
    pub(crate) fn queue_prompt(&mut self) {
        let prompt = self.prompt.trim().to_owned();
        let Some(id) = self.active.as_ref().map(|session| session.id) else {
            return;
        };
        if prompt.is_empty() || !self.attachments_ready() {
            return;
        }
        let attachments = self.take_attachments();
        self.queued.entry(id).or_default().push(Queued {
            prompt,
            attachments,
        });
        self.prompt.clear();
    }

    /// Shows messages sent into running tasks in their sessions, or puts back one that
    /// didn't arrive.
    fn poll_steering(&mut self) {
        let mut done = Vec::new();
        self.steering.retain(|receiver| match receiver.try_recv() {
            Ok(result) => {
                done.push(result);
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => false,
        });
        for (id, text, result) in done {
            match result {
                Ok(()) => {
                    let line = format!("{}\n", crate::activity::added_notice(&text));
                    let session = match &mut self.active {
                        Some(active) if active.id == id => Some(active),
                        _ => self.sessions.iter_mut().find(|saved| saved.id == id),
                    };
                    if let Some(turn) = session.and_then(|session| session.turns.last_mut()) {
                        turn.output.push_str(&line);
                    }
                }
                Err(message) => {
                    // It goes back in the queue so it isn't lost.
                    self.queued.entry(id).or_default().insert(
                        0,
                        Queued {
                            prompt: text,
                            attachments: Vec::new(),
                        },
                    );
                    self.inputs.notice = Some(format!("{message} It will run after the task."));
                }
            }
        }
    }

    /// Starts queued messages whose sessions are free, while there is room. A session whose
    /// last task failed or was stopped keeps its queue until you send it.
    fn start_queues(&mut self, ctx: &egui::Context) {
        let waiting: Vec<Uuid> = self
            .queued
            .iter()
            .filter(|(_, items)| !items.is_empty())
            .map(|(id, _)| *id)
            .filter(|id| !self.held.contains(id))
            .collect();
        for id in waiting {
            if !self.room_for_task() {
                break;
            }
            let busy = if self.active.as_ref().is_some_and(|active| active.id == id) {
                self.is_running()
            } else {
                self.parked.get(&id).is_some_and(Parked::is_running)
            };
            if busy {
                continue;
            }
            self.send_queued(ctx, id, 0);
        }
    }

    /// Runs one queued message now, in its own session, wherever the screen is.
    pub(crate) fn send_queued(&mut self, ctx: &egui::Context, id: Uuid, index: usize) {
        let Some(item) = self
            .queued
            .get_mut(&id)
            .filter(|items| index < items.len())
            .map(|items| items.remove(index))
        else {
            return;
        };
        self.held.remove(&id);
        let back = self.active.as_ref().map(|session| session.id);
        let view = self.view;
        if back != Some(id) {
            let Some(session) = self.sessions.iter().find(|saved| saved.id == id).cloned() else {
                return;
            };
            self.select_session(session);
        }
        if self.is_running() || !self.room_for_task() {
            // Not now after all: it keeps its place.
            self.queued.entry(id).or_default().insert(0, item);
        } else {
            self.submit_from(ctx, Some(item));
        }
        if back != Some(id) {
            match back.and_then(|back| self.sessions.iter().find(|saved| saved.id == back).cloned())
            {
                Some(session) => self.select_session(session),
                None => self.reset(),
            }
            self.view = view;
        }
    }

    /// The model the next task runs on: Auto's pick, or the one chosen in the composer.
    pub(crate) fn run_route(&self) -> usize {
        self.auto_pick()
            .and_then(|pick| models::index_of(&pick.choice.model))
            .unwrap_or(self.route)
    }

    pub(crate) fn can_submit(&self) -> bool {
        !self.is_running()
            && self.room_for_task()
            && self.settings.project.is_some()
            && !self.prompt.trim().is_empty()
            && self.attachments_ready()
    }

    pub(crate) fn submit(&mut self, context: &egui::Context) {
        if !self.can_submit() {
            return;
        }
        self.submit_from(context, None);
    }

    /// Starts a task in the session on screen: what the composer holds, or a queued message.
    fn submit_from(&mut self, context: &egui::Context, queued: Option<Queued>) {
        if self.is_running() || !self.room_for_task() {
            return;
        }
        let Some(directory) = self
            .active
            .as_ref()
            .map(|session| session.project_path.clone())
            .or_else(|| self.settings.project.clone())
        else {
            return;
        };
        let from_composer = queued.is_none();
        let (prompt, attachments) = match queued {
            Some(queued) => (queued.prompt, queued.attachments),
            None => (self.prompt.trim().to_owned(), self.take_attachments()),
        };
        let kinds: Vec<_> = attachments.iter().map(crate::inputs::sent_kind).collect();
        let picked = self.pick_for(&prompt, &kinds);
        let route = picked
            .as_ref()
            .and_then(|pick| models::index_of(&pick.choice.model))
            .unwrap_or(self.route);
        let selected = models::at(route);
        let (id, name) = (selected.id.as_str(), selected.name.as_str());
        let can_fall_back = self.backup_route().is_some_and(|backup| backup != route);
        let variant = match &picked {
            Some(pick) => pick.choice.effort.clone(),
            None => self.effort_of(&selected),
        };
        let mut label = name.to_owned();
        if let Some(variant) = &variant {
            label.push_str(&format!(" · {}", models::effort_label(variant)));
        }
        if self.plan_mode {
            label.push_str(" · Plan mode");
        }
        if let Some(pick) = &picked {
            label.push_str(&format!(" · Auto: {}", pick.reason));
        }
        let session = self.active.get_or_insert_with(|| Session {
            id: Uuid::new_v4(),
            project_path: directory.clone(),
            route_id: id.into(),
            server_session: None,
            turns: Vec::new(),
            updated: Session::revision_time(),
            context_tokens: 0,
            spent: Default::default(),
        });
        session.route_id = id.into();
        let server_session = session.server_session.clone();
        let task = Task {
            directory,
            binary: self.settings.executable.clone(),
            model: id.into(),
            prompt: format!("{prompt}{}", attach::describe(&attachments)),
            session: server_session,
            fallback: self.settings.fallback && can_fall_back,
            files: attachments
                .iter()
                .filter_map(|item| item.file.clone())
                .collect(),
            backup: self.settings.backup_model.clone(),
            auto_approve: self.settings.auto_approve,
            time_limit: (self.settings.time_limit_minutes > 0).then(|| {
                std::time::Duration::from_secs(u64::from(self.settings.time_limit_minutes) * 60)
            }),
            variant,
            agent: self.plan_mode.then(|| "plan".to_owned()),
            live: true,
        };
        if let Some(session) = &mut self.active {
            session.turns.push(Turn {
                id: Uuid::new_v4(),
                prompt,
                model: label,
                output: String::new(),
                attachments,
            });
            session.updated = Session::revision_time();
            // It shows in the sidebar from the start, so it can be left while it runs.
            let copy = session.clone();
            self.sessions.retain(|saved| saved.id != copy.id);
            self.sessions.insert(0, copy);
        }
        self.stop_reading();
        if from_composer {
            self.prompt.clear();
        }
        self.task_error = None;
        self.limited = false;
        self.status = Status::Running;
        self.view = View::Chat;
        self.task_started = runner::now_millis();
        if self.settings.keep_awake {
            self.awake = crate::awake::Awake::start();
        }
        self.doing = Some(("Starting OpenCode".to_owned(), self.task_started));
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let repaint = context.clone();
        let worker = thread::spawn(move || {
            runner::run(task, &cancel, |event| {
                let _ = sender.send(event);
                repaint.request_repaint();
            });
        });
        self.running = Some(Running {
            receiver,
            stop,
            worker,
            reach: None,
        });
    }

    /// Runs the last prompt again, on the backup model when `on_backup` is set.
    pub(crate) fn retry(&mut self, context: &egui::Context, on_backup: bool) {
        let Some((prompt, attachments)) = self
            .active
            .as_ref()
            .and_then(|session| session.turns.last())
            .map(|turn| (turn.prompt.clone(), turn.attachments.clone()))
        else {
            return;
        };
        self.restore_attachments(&attachments);
        if on_backup && let Some(backup) = self.backup_route() {
            self.route = backup;
        }
        self.prompt = prompt;
        self.submit(context);
    }

    pub(crate) fn stop(&mut self) {
        if let Some(run) = &self.running {
            self.status = Status::Stopping;
            run.stop.store(true, Ordering::Relaxed);
        }
    }

    fn apply(&mut self, event: Event, ctx: &egui::Context) {
        match event {
            Event::Output(text) => {
                self.open_written_pages(&text);
                if let Some(turn) = self
                    .active
                    .as_mut()
                    .and_then(|session| session.turns.last_mut())
                {
                    turn.output.push_str(&text);
                }
            }
            Event::Session(id) => {
                if let Some(session) = &mut self.active {
                    session.server_session = Some(id);
                }
            }
            Event::Limited => self.limited = true,
            Event::Activity { doing, since } => self.doing = Some((doing, since)),
            Event::Server(reach) => {
                if let Some(run) = &mut self.running {
                    run.reach = Some(reach);
                }
            }
            Event::Usage {
                model,
                timestamp,
                part,
            } => {
                if let Some(store) = &self.store
                    && let Err(error) = store.record(&model, timestamp, &part)
                {
                    self.storage_error = Some(format!("Couldn’t record usage: {error}"));
                }
                if let Some(session) = &mut self.active {
                    session.add_step(crate::storage::StepTokens::of(&part));
                }
                self.refresh_usage();
            }
            Event::Finished(result) => {
                self.doing = None;
                self.awake = None;
                self.load_history();
                self.todos.read_at = None;
                if self.pane == Some(crate::panes::Pane::Changes) {
                    self.refresh_changes();
                }
                if let Some(run) = self.running.take() {
                    let _ = run.worker.join();
                }
                if self.settings.notify && !ctx.input(|input| input.focused) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Informational,
                    ));
                }
                if result.is_ok()
                    && self.settings.read_aloud
                    && let Some(turn) = self.active.as_ref().and_then(|s| s.turns.last())
                {
                    let (id, output) = (turn.id, turn.output.clone());
                    self.read_aloud(id, &output);
                }
                self.status = match result {
                    Ok(()) => Status::Complete,
                    Err(error) if error == "Stopped" => Status::Stopped,
                    Err(error) => {
                        self.task_error = Some(error);
                        Status::Failed
                    }
                };
                if let Some(session) = &mut self.active {
                    session.updated = Session::revision_time();
                    if let Some(store) = &self.store
                        && let Err(error) = store.save(session)
                    {
                        self.storage_error = Some(format!("Couldn’t save the session: {error}"));
                    }
                    self.sessions.retain(|saved| saved.id != session.id);
                    self.sessions.insert(0, session.clone());
                }
                self.focus_composer = true;
                self.after_task(ctx);
            }
        }
    }

    /// When a task ends: a queue behind a task that failed or was stopped waits to be sent by
    /// hand; any other queue starts its next message.
    fn after_task(&mut self, ctx: &egui::Context) {
        let ended: Vec<(Uuid, Status)> = self
            .active
            .iter()
            .filter(|_| !self.is_running())
            .map(|session| (session.id, self.status))
            .chain(
                self.parked
                    .iter()
                    .filter(|(_, parked)| !parked.is_running())
                    .map(|(id, parked)| (*id, parked.status)),
            )
            .collect();
        for (id, status) in ended {
            if matches!(status, Status::Failed | Status::Stopped)
                && self.queued.get(&id).is_some_and(|items| !items.is_empty())
            {
                self.held.insert(id);
            }
        }
        self.start_queues(ctx);
    }

    /// Delivers what parked tasks report to their sessions in the sidebar.
    fn poll_parked(&mut self, ctx: &egui::Context) {
        let ids: Vec<Uuid> = self
            .parked
            .iter()
            .filter(|(_, parked)| parked.running.is_some())
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            while let Some(event) = self
                .parked
                .get(&id)
                .and_then(|parked| parked.running.as_ref())
                .and_then(|run| run.receiver.try_recv().ok())
            {
                self.apply_parked(id, event, ctx);
            }
        }
    }

    fn apply_parked(&mut self, id: Uuid, event: Event, ctx: &egui::Context) {
        let Some(parked) = self.parked.get_mut(&id) else {
            return;
        };
        let session = self.sessions.iter_mut().find(|saved| saved.id == id);
        match event {
            Event::Output(text) => {
                if let Some(turn) = session.and_then(|session| session.turns.last_mut()) {
                    turn.output.push_str(&text);
                }
            }
            Event::Session(server) => {
                if let Some(session) = session {
                    session.server_session = Some(server);
                }
            }
            Event::Limited => parked.limited = true,
            Event::Activity { doing, since } => parked.doing = Some((doing, since)),
            Event::Server(reach) => {
                if let Some(run) = &mut parked.running {
                    run.reach = Some(reach);
                }
            }
            Event::Usage {
                model,
                timestamp,
                part,
            } => {
                if let Some(session) = session {
                    session.add_step(crate::storage::StepTokens::of(&part));
                }
                if let Some(store) = &self.store
                    && let Err(error) = store.record(&model, timestamp, &part)
                {
                    self.storage_error = Some(format!("Couldn’t record usage: {error}"));
                }
                self.refresh_usage();
            }
            Event::Finished(result) => {
                parked.doing = None;
                parked.awake = None;
                if let Some(run) = parked.running.take() {
                    let _ = run.worker.join();
                }
                parked.status = match result {
                    Ok(()) => Status::Complete,
                    Err(error) if error == "Stopped" => Status::Stopped,
                    Err(error) => {
                        parked.task_error = Some(error);
                        Status::Failed
                    }
                };
                if let Some(session) = session {
                    session.updated = Session::revision_time();
                    if let Some(store) = &self.store
                        && let Err(error) = store.save(session)
                    {
                        self.storage_error = Some(format!("Couldn’t save the session: {error}"));
                    }
                }
                self.sessions
                    .sort_by(|a, b| b.updated.total_cmp(&a.updated));
                self.load_history();
                if self.settings.notify {
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Informational,
                    ));
                }
                self.after_task(ctx);
            }
        }
        ctx.request_repaint();
    }

    /// Stops a session's task, on screen or parked.
    pub(crate) fn stop_session(&mut self, id: Uuid) {
        if self.active.as_ref().is_some_and(|active| active.id == id) {
            self.stop();
        } else if let Some(parked) = self.parked.get_mut(&id)
            && let Some(run) = &parked.running
        {
            parked.status = Status::Stopping;
            run.stop.store(true, Ordering::Relaxed);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        // A dialog or the terminal takes the keyboard: Esc there is theirs.
        if self.confirm_full_access || self.tutorial.is_some() || self.terminal_focused {
            return;
        }
        let pressed = |modifiers, key| ctx.input_mut(|input| input.consume_key(modifiers, key));
        if pressed(Modifiers::COMMAND, Key::N) {
            self.reset();
        }
        if pressed(Modifiers::COMMAND, Key::Comma) {
            self.open(View::Settings);
        }
        if pressed(Modifiers::COMMAND, Key::D) {
            self.toggle_dictation(ctx);
        }
        if ctx.input(|input| input.key_pressed(Key::Escape)) {
            if !matches!(self.inputs.dictation, Dictation::Idle) {
                self.cancel_dictation();
            } else if self.inputs.reading.is_some() {
                self.stop_reading();
            } else if self.running.is_some() {
                self.stop();
            } else if self.view != View::Chat {
                self.view = View::Chat;
                self.focus_composer = true;
            }
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(debug_assertions)]
        if let Some(mut preview) = self.preview.take() {
            preview.drive(self, ctx);
            self.preview = Some(preview);
        }
        while let Some(event) = self
            .running
            .as_ref()
            .and_then(|run| run.receiver.try_recv().ok())
        {
            self.apply(event, ctx);
        }
        self.poll_parked(ctx);
        self.poll_steering();
        if !self.discovery_started {
            self.start_discovery(ctx);
            self.load_history();
        }
        self.poll_discovery();
        self.poll_providers();
        self.poll_ask_rules();
        self.poll_history(ctx);
        self.poll_install(ctx);
        self.poll_todos(ctx);
        self.poll_changes(ctx);
        self.poll_inputs(ctx);
        self.shortcuts(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.draw(ui);
        let ctx = ui.ctx().clone();
        self.route_links(&ctx);
        let covered = self.confirm_full_access || self.tutorial.is_some();
        #[cfg(debug_assertions)]
        let previewing = self.preview.is_some();
        #[cfg(not(debug_assertions))]
        let previewing = false;
        let browsing = self.pane == Some(crate::panes::Pane::Browser);
        if let Some(browser) = &mut self.browser {
            browser.shown = !covered && browsing;
            if !browsing {
                browser.hide_all();
            }
            // A click in the app takes the keyboard back from the page.
            if ctx.input(|input| input.pointer.any_pressed()) {
                browser.release_focus();
            }
            if !previewing {
                browser.sync(&ctx, frame);
            }
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // Every task is asked to stop first, so they wind down together.
        let runs: Vec<Running> = self
            .running
            .take()
            .into_iter()
            .chain(
                self.parked
                    .values_mut()
                    .filter_map(|parked| parked.running.take()),
            )
            .collect();
        for run in &runs {
            run.stop.store(true, Ordering::Relaxed);
        }
        for run in runs {
            let _ = run.worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (App, egui::Context) {
        let ctx = egui::Context::default();
        let logo = ctx.load_texture(
            "logo",
            egui::ColorImage::new([1, 1], vec![egui::Color32::WHITE]),
            egui::TextureOptions::LINEAR,
        );
        (App::empty(logo), ctx)
    }

    fn saved(title: &str) -> Session {
        Session {
            id: Uuid::new_v4(),
            project_path: PathBuf::from("/work/app"),
            route_id: models::DEFAULT.into(),
            server_session: None,
            turns: vec![Turn {
                id: Uuid::new_v4(),
                prompt: title.into(),
                model: "GLM 5.3 Flash".into(),
                output: "Done.".into(),
                attachments: Vec::new(),
            }],
            updated: Session::revision_time(),
            context_tokens: 0,
            spent: Default::default(),
        }
    }

    #[test]
    fn a_task_keeps_running_while_its_chat_is_out_of_view() {
        let (mut app, ctx) = app();
        app.settings.project = Some(PathBuf::from("/work/app"));
        let first = saved("First");
        let second = saved("Second");
        app.sessions = vec![first.clone(), second.clone()];
        app.select_session(first.clone());

        // A task for the first chat, fed by hand.
        let (sender, receiver) = mpsc::channel();
        app.running = Some(Running {
            receiver,
            stop: Arc::new(AtomicBool::new(false)),
            worker: thread::spawn(|| {}),
            reach: None,
        });
        app.status = Status::Running;
        assert_eq!(app.tasks_running(), 1);

        app.select_session(second);
        assert!(!app.is_running(), "the chat on screen has no task");
        assert_eq!(app.tasks_running(), 1, "the first chat's task still counts");

        sender.send(Event::Output(" More output.".into())).unwrap();
        sender.send(Event::Finished(Ok(()))).unwrap();
        app.poll_parked(&ctx);
        let kept = app
            .sessions
            .iter()
            .find(|session| session.id == first.id)
            .unwrap();
        assert!(kept.turns[0].output.ends_with("More output."));
        assert_eq!(app.tasks_running(), 0);

        app.select_session(first);
        assert_eq!(
            app.status,
            Status::Complete,
            "its result is there on return"
        );
        assert!(app.parked.is_empty());
    }

    #[test]
    fn drafts_stay_with_their_chat() {
        let (mut app, _) = app();
        app.settings.project = Some(PathBuf::from("/work/app"));
        let first = saved("First");
        let second = saved("Second");
        app.sessions = vec![first.clone(), second.clone()];

        app.select_session(first.clone());
        app.prompt = "Half-written question".into();
        app.select_session(second.clone());
        assert!(app.prompt.is_empty(), "the other chat starts empty");
        app.prompt = "Another draft".into();

        app.reset();
        assert!(app.prompt.is_empty(), "a new chat starts empty");
        app.prompt = "A new idea".into();

        app.select_session(first.clone());
        assert_eq!(app.prompt, "Half-written question");
        app.select_session(second);
        assert_eq!(app.prompt, "Another draft");
        app.reset();
        assert_eq!(
            app.prompt, "A new idea",
            "the new chat's draft comes back too"
        );

        // A cleared draft doesn't come back.
        app.select_session(first.clone());
        app.prompt.clear();
        app.reset();
        app.select_session(first);
        assert!(app.prompt.is_empty());
    }

    #[test]
    fn full_access_waits_for_confirmation() {
        let (mut app, ctx) = app();
        app.choose_access(&ctx, true);
        assert!(app.confirm_full_access, "the confirmation opens");
        assert!(
            !app.settings.auto_approve,
            "nothing changes before confirming"
        );
        app.answer_full_access(false);
        assert!(!app.confirm_full_access);
        assert!(
            !app.settings.auto_approve,
            "cancelling keeps Standard access"
        );

        app.choose_access(&ctx, true);
        app.answer_full_access(true);
        assert!(app.settings.auto_approve);

        // Already on: choosing it again asks nothing.
        app.choose_access(&ctx, true);
        assert!(!app.confirm_full_access);

        // Going back to Standard access needs no confirmation.
        app.choose_access(&ctx, false);
        assert!(!app.settings.auto_approve);
        assert!(!app.confirm_full_access);
    }
}
