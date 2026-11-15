//! Everything that goes into a prompt besides typing, and reading the answer aloud:
//! attachments, the speech model, dictation, and the system voice.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, SystemTime},
};

use eframe::egui;
use uuid::Uuid;

use crate::{
    app::App,
    attach::{self, Kind, Problem, Sent},
    markdown, models,
    speak::Speaker,
    speech::{self, Recorder},
};

/// Prepared text from attachments is kept this long, so a session can be run again.
const KEEP_PREPARED: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// An attachment's result from its worker thread.
type Prepared = (Uuid, Result<Sent, Problem>);

pub enum Prep {
    Preparing,
    Ready(Sent),
    NeedsModel,
    Failed(String),
}

pub struct Pending {
    pub id: Uuid,
    pub source: PathBuf,
    pub name: String,
    pub kind: Kind,
    pub prep: Prep,
}

pub enum ModelState {
    Missing,
    Downloading {
        received: Arc<AtomicU64>,
        cancel: Arc<AtomicBool>,
        done: Receiver<Result<PathBuf, String>>,
    },
    Ready(PathBuf),
    Failed(String),
}

pub enum Dictation {
    Idle,
    Recording(Recorder),
    Transcribing(Receiver<Result<String, String>>),
}

pub struct Inputs {
    pub pending: Vec<Pending>,
    prepared: (Sender<Prepared>, Receiver<Prepared>),
    pub model: ModelState,
    /// Show the panel that offers to download the speech model.
    pub offer_model: bool,
    pub dictation: Dictation,
    pub speaker: Speaker,
    /// The turn being read aloud.
    pub reading: Option<Uuid>,
    /// A message about dictation, reading aloud, or a file that couldn't be added.
    pub notice: Option<String>,
    /// Files are being dragged over the window.
    pub dropping: bool,
    /// Lets screenshots show dictation without a microphone: level and elapsed time.
    pub staged_listening: Option<(f32, Duration)>,
    /// Image previews for attachment cards, by file.
    thumbs: HashMap<PathBuf, Thumb>,
    thumb_results: (Sender<Preview>, Receiver<Preview>),
}

/// A finished preview, for the file it shows.
type Preview = (PathBuf, Thumb);

#[derive(Clone)]
enum Thumb {
    Loading,
    Ready(egui::TextureHandle),
    Unavailable,
}

/// Images larger than this are shown as a file card instead of being decoded.
const PREVIEW_LIMIT: u64 = 40 * 1024 * 1024;

/// Decodes `path` into a small texture, or says it can't.
fn load_thumb(ctx: &egui::Context, path: &Path) -> Thumb {
    let small_enough = fs::metadata(path).is_ok_and(|meta| meta.len() <= PREVIEW_LIMIT);
    let decoded = small_enough
        .then(|| {
            image::ImageReader::open(path)
                .ok()?
                .with_guessed_format()
                .ok()?
                .decode()
                .ok()
        })
        .flatten();
    let Some(picture) = decoded else {
        return Thumb::Unavailable;
    };
    let picture = picture.thumbnail(256, 256).to_rgba8();
    let size = [picture.width() as usize, picture.height() as usize];
    let pixels = egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw());
    Thumb::Ready(ctx.load_texture(
        format!("preview {}", path.display()),
        pixels,
        egui::TextureOptions::LINEAR,
    ))
}

impl Inputs {
    /// The preview of `path`, starting to load it the first time it is asked for.
    pub fn thumbnail(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        if !attach::can_preview(path) {
            return None;
        }
        match self.thumbs.get(path) {
            Some(Thumb::Ready(texture)) => Some(texture.clone()),
            Some(_) => None,
            None => {
                self.thumbs.insert(path.to_path_buf(), Thumb::Loading);
                let (sender, ctx, path) = (
                    self.thumb_results.0.clone(),
                    ctx.clone(),
                    path.to_path_buf(),
                );
                thread::spawn(move || {
                    let thumb = load_thumb(&ctx, &path);
                    let _ = sender.send((path, thumb));
                    ctx.request_repaint();
                });
                None
            }
        }
    }

    /// Whether any preview is still loading.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn previews_loading(&self) -> bool {
        self.thumbs
            .values()
            .any(|thumb| matches!(thumb, Thumb::Loading))
    }
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            prepared: mpsc::channel(),
            model: ModelState::Missing,
            offer_model: false,
            dictation: Dictation::Idle,
            speaker: Speaker::default(),
            reading: None,
            notice: None,
            dropping: false,
            staged_listening: None,
            thumbs: HashMap::new(),
            thumb_results: mpsc::channel(),
        }
    }
}

/// Removes prepared attachment text older than [`KEEP_PREPARED`].
pub fn prune(data: &Path) {
    let folder = data.join("attachments");
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > KEEP_PREPARED);
        if old {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// The kind of an attachment known only by its saved name, for its icon.
pub(crate) fn sent_kind(sent: &Sent) -> Kind {
    if sent.reference.is_some() || sent.note.ends_with("text files") || sent.note == "1 text file" {
        return Kind::Folder;
    }
    let note = sent.note.as_str();
    if note.starts_with("image") {
        Kind::Image
    } else if note.starts_with("video") {
        Kind::Video
    } else if note.starts_with("transcript") {
        Kind::Audio
    } else if note.starts_with("text,") {
        Kind::Document
    } else {
        Kind::Text
    }
}

impl App {
    /// Where settings, sessions, the speech model, and prepared attachments live.
    pub(crate) fn data_dir(&self) -> PathBuf {
        match &self.store {
            Some(store) => store.location().to_path_buf(),
            None => std::env::temp_dir().join("Super OpenCode"),
        }
    }

    fn speech_model(&self) -> Option<PathBuf> {
        match &self.inputs.model {
            ModelState::Ready(path) => Some(path.clone()),
            _ => None,
        }
    }

    pub(crate) fn pick_files(&mut self, ctx: &egui::Context) {
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("Attach files")
            .pick_files()
        {
            self.attach(paths, ctx);
        }
    }

    pub(crate) fn pick_folder(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Attach a folder")
            .pick_folder()
        {
            self.attach(vec![path], ctx);
        }
    }

    pub(crate) fn attach(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        for source in paths {
            if self.inputs.pending.iter().any(|item| item.source == source) {
                continue;
            }
            let name = attach::display_name(&source);
            let id = Uuid::new_v4();
            match attach::classify(&source) {
                Ok(kind) => {
                    self.inputs.pending.push(Pending {
                        id,
                        source,
                        name,
                        kind,
                        prep: Prep::Preparing,
                    });
                    self.prepare(id, ctx);
                }
                Err(message) => self.inputs.pending.push(Pending {
                    id,
                    source,
                    name,
                    kind: Kind::Text,
                    prep: Prep::Failed(message),
                }),
            }
        }
        self.focus_composer = true;
    }

    fn prepare(&mut self, id: Uuid, ctx: &egui::Context) {
        let model = self.speech_model();
        let project = self.settings.project.clone();
        let language = self.settings.dictation_language.clone();
        let workdir = self.data_dir().join("attachments").join(id.to_string());
        let Some(item) = self.inputs.pending.iter_mut().find(|item| item.id == id) else {
            return;
        };
        if item.kind == Kind::Audio && model.is_none() {
            item.prep = Prep::NeedsModel;
            self.inputs.offer_model = true;
            return;
        }
        item.prep = Prep::Preparing;
        let (kind, source) = (item.kind, item.source.clone());
        let sender = self.inputs.prepared.0.clone();
        let repaint = ctx.clone();
        thread::spawn(move || {
            let result = attach::prepare(
                kind,
                &source,
                project.as_deref(),
                &workdir,
                model.as_deref(),
                language.as_deref(),
            );
            let _ = sender.send((id, result));
            repaint.request_repaint();
        });
    }

    pub(crate) fn remove_attachment(&mut self, id: Uuid) {
        self.inputs.pending.retain(|item| item.id != id);
    }

    /// Attachments the selected model cannot read: the attachment, why, and a model that can.
    pub(crate) fn refusals(&self) -> Vec<(Uuid, String, Option<usize>)> {
        let model = models::at(self.run_route()).id;
        self.inputs
            .pending
            .iter()
            .filter(|item| !matches!(item.prep, Prep::Failed(_)))
            .filter_map(|item| {
                attach::refusal(item.kind, &item.source, &model)
                    .map(|(message, alternative)| (item.id, message, alternative))
            })
            .collect()
    }

    /// Whether every attachment is prepared and readable by the selected model.
    pub(crate) fn attachments_ready(&self) -> bool {
        self.inputs
            .pending
            .iter()
            .all(|item| matches!(item.prep, Prep::Ready(_)))
            && self.refusals().is_empty()
    }

    pub(crate) fn take_attachments(&mut self) -> Vec<Sent> {
        std::mem::take(&mut self.inputs.pending)
            .into_iter()
            .filter_map(|item| match item.prep {
                Prep::Ready(sent) => Some(sent),
                _ => None,
            })
            .collect()
    }

    /// Puts a turn's attachments back in the composer, for running it again.
    pub(crate) fn restore_attachments(&mut self, sent: &[Sent]) {
        self.inputs.pending = sent
            .iter()
            .filter(|item| item.file.as_ref().is_none_or(|file| file.exists()))
            .map(|item| Pending {
                id: Uuid::new_v4(),
                source: item
                    .original()
                    .map_or_else(|| PathBuf::from(&item.name), Path::to_path_buf),
                name: item.name.clone(),
                kind: sent_kind(item),
                prep: Prep::Ready(item.clone()),
            })
            .collect();
    }

    pub(crate) fn start_download(&mut self, ctx: &egui::Context) {
        if matches!(self.inputs.model, ModelState::Downloading { .. }) {
            return;
        }
        let received = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, done) = mpsc::channel();
        let data = self.data_dir();
        let (counter, stop, repaint) = (received.clone(), cancel.clone(), ctx.clone());
        thread::spawn(move || {
            let _ = sender.send(speech::download(&data, &counter, &stop));
            repaint.request_repaint();
        });
        self.inputs.model = ModelState::Downloading {
            received,
            cancel,
            done,
        };
    }

    pub(crate) fn cancel_download(&mut self) {
        if let ModelState::Downloading { cancel, .. } = &self.inputs.model {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(crate) fn remove_model(&mut self) {
        if let ModelState::Ready(path) = &self.inputs.model {
            match fs::remove_file(path) {
                Ok(()) => self.inputs.model = ModelState::Missing,
                Err(error) => {
                    self.inputs.notice = Some(format!("Couldn’t remove the speech model: {error}"))
                }
            }
        }
    }

    /// Starts dictation, or stops it and transcribes what was said.
    pub(crate) fn toggle_dictation(&mut self, ctx: &egui::Context) {
        match std::mem::replace(&mut self.inputs.dictation, Dictation::Idle) {
            Dictation::Idle => {
                if self.speech_model().is_none() {
                    self.inputs.offer_model = true;
                    return;
                }
                self.inputs.speaker.stop();
                self.inputs.reading = None;
                match Recorder::start() {
                    Ok(recorder) => {
                        self.inputs.notice = None;
                        self.inputs.dictation = Dictation::Recording(recorder);
                    }
                    Err(message) => self.inputs.notice = Some(microphone_help(&message)),
                }
            }
            Dictation::Recording(recorder) => {
                let samples = recorder.finish();
                let Some(model) = self.speech_model() else {
                    return;
                };
                let (sender, receiver) = mpsc::channel();
                let repaint = ctx.clone();
                let language = self.settings.dictation_language.clone();
                thread::spawn(move || {
                    let _ = sender.send(speech::transcribe(&model, &samples, language.as_deref()));
                    repaint.request_repaint();
                });
                self.inputs.dictation = Dictation::Transcribing(receiver);
            }
            transcribing @ Dictation::Transcribing(_) => self.inputs.dictation = transcribing,
        }
    }

    pub(crate) fn cancel_dictation(&mut self) {
        self.inputs.dictation = Dictation::Idle;
    }

    pub(crate) fn read_aloud(&mut self, turn: Uuid, output: &str) {
        self.cancel_dictation();
        match self
            .inputs
            .speaker
            .speak(&markdown::speakable(output), self.settings.reading_pace)
        {
            Ok(()) => self.inputs.reading = Some(turn),
            Err(message) => self.inputs.notice = Some(message),
        }
    }

    pub(crate) fn stop_reading(&mut self) {
        self.inputs.speaker.stop();
        self.inputs.reading = None;
    }

    /// Collects finished background work and files dropped on the window.
    pub(crate) fn poll_inputs(&mut self, ctx: &egui::Context) {
        while let Ok((path, thumb)) = self.inputs.thumb_results.1.try_recv() {
            self.inputs.thumbs.insert(path, thumb);
        }
        while let Ok((id, result)) = self.inputs.prepared.1.try_recv() {
            if let Some(item) = self.inputs.pending.iter_mut().find(|item| item.id == id) {
                item.prep = match result {
                    Ok(sent) => Prep::Ready(sent),
                    Err(Problem::NeedsSpeechModel) => Prep::NeedsModel,
                    Err(Problem::Failed(message)) => Prep::Failed(message),
                };
            }
        }

        let finished = match &self.inputs.model {
            ModelState::Downloading { done, .. } => done.try_recv().ok(),
            _ => None,
        };
        match finished {
            Some(Ok(path)) => {
                self.inputs.model = ModelState::Ready(path);
                self.inputs.offer_model = false;
                let waiting: Vec<Uuid> = self
                    .inputs
                    .pending
                    .iter()
                    .filter(|item| matches!(item.prep, Prep::NeedsModel))
                    .map(|item| item.id)
                    .collect();
                for id in waiting {
                    self.prepare(id, ctx);
                }
            }
            Some(Err(message)) if message == "Cancelled" => self.inputs.model = ModelState::Missing,
            Some(Err(message)) => self.inputs.model = ModelState::Failed(message),
            None => {}
        }
        if matches!(self.inputs.model, ModelState::Downloading { .. }) {
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        if let Dictation::Transcribing(receiver) = &self.inputs.dictation
            && let Ok(result) = receiver.try_recv()
        {
            self.inputs.dictation = Dictation::Idle;
            match result {
                Ok(text) if text.trim().is_empty() => {
                    self.inputs.notice =
                        Some("No speech was heard. Check the microphone and try again.".into())
                }
                Ok(text) => {
                    if !self.prompt.is_empty() && !self.prompt.ends_with(char::is_whitespace) {
                        self.prompt.push(' ');
                    }
                    self.prompt.push_str(text.trim());
                    self.focus_composer = true;
                }
                Err(message) => self.inputs.notice = Some(message),
            }
        }
        if let Dictation::Recording(recorder) = &self.inputs.dictation {
            if recorder.elapsed() >= speech::LONGEST {
                self.toggle_dictation(ctx);
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }

        if self.inputs.reading.is_some() {
            if self.inputs.speaker.is_speaking() {
                ctx.request_repaint_after(Duration::from_millis(250));
            } else {
                self.inputs.reading = None;
            }
        }

        let (dropped, hovering): (Vec<PathBuf>, bool) = ctx.input(|input| {
            (
                input
                    .raw
                    .dropped_files
                    .iter()
                    .map(|file| file.path().to_path_buf())
                    .collect(),
                !input.raw.hovered_files.is_empty(),
            )
        });
        self.inputs.dropping = hovering;
        if !dropped.is_empty() {
            if self.settings.project.is_some() && !self.is_running() {
                self.attach(dropped, ctx);
            } else {
                self.inputs.notice = Some("Choose a project folder before attaching files.".into());
            }
        }
    }
}

/// Adds where to grant microphone access to an error about the microphone.
fn microphone_help(message: &str) -> String {
    if cfg!(target_os = "macos") {
        format!(
            "{message} If you denied access, allow Super OpenCode in System Settings › Privacy & Security › Microphone."
        )
    } else if cfg!(windows) {
        format!("{message} Check Settings › Privacy & security › Microphone.")
    } else {
        message.to_owned()
    }
}
