//! Command-line use: one task in the terminal, the usage table, and the free-model catalog.

use std::{
    cell::RefCell,
    env,
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    sync::atomic::AtomicBool,
};

use crate::{
    attach::{self, Problem},
    models,
    runner::{self, BACKUP, BUILTIN, Event, Task},
    speech,
    storage::Store,
};

pub const USAGE: &str = "\
super-opencode [--project <folder>]      open the app
super-opencode --run <task> [options]    run one task in the terminal
super-opencode --usage                   show usage: totals, streaks, and today
super-opencode --models                  list the free models the app offers
super-opencode --catalog                 list free OpenRouter models with tool support
super-opencode --download-speech-model   download the model dictation and audio files use

Options for --run:
  --project <folder>   project folder (default: the current folder)
  --model <id>         a model ID listed by --models or --catalog
  --backup             start on the backup model from Settings
  --fallback-on-429    continue on the backup model after a rate limit; a backup on
                       another provider shares the session with that provider
  --attach <path>      attach a file or folder (repeatable)
  --effort <level>     the model's effort level, such as low, high, or max
  --plan               run OpenCode's plan agent, which can't edit files
  --session <id>       continue an OpenCode session
  --private            allow only the built-in model and no fallback

--run also follows the app's Settings for the backup model, time limit, and approvals.";

#[derive(Default)]
pub struct Options {
    pub project: Option<PathBuf>,
    /// Debug builds only: folder to write interface screenshots into.
    pub preview: Option<PathBuf>,
    run: Option<String>,
    model: Option<String>,
    attach: Vec<std::path::PathBuf>,
    effort: Option<String>,
    plan: bool,
    models: bool,
    download: bool,
    session: Option<String>,
    backup: bool,
    fallback: bool,
    private: bool,
    usage: bool,
    catalog: bool,
    help: bool,
}

impl Options {
    pub fn parse(mut args: impl Iterator<Item = OsString>) -> Result<Self, String> {
        let mut options = Self::default();
        while let Some(flag) = args.next() {
            let name = flag.to_string_lossy().into_owned();
            let mut value = |what: &str| args.next().ok_or_else(|| format!("{name} needs {what}."));
            let text = |value: OsString| {
                value
                    .into_string()
                    .map_err(|_| format!("The value for {name} must be UTF-8."))
            };
            match name.as_str() {
                "--project" => options.project = Some(value("a folder")?.into()),
                "--run" => options.run = Some(text(value("a task")?)?),
                "--model" => options.model = Some(text(value("a model ID")?)?),
                "--attach" => options.attach.push(value("a file or folder")?.into()),
                "--effort" => options.effort = Some(text(value("an effort level")?)?),
                "--plan" => options.plan = true,
                "--models" => options.models = true,
                "--download-speech-model" => options.download = true,
                "--session" => options.session = Some(text(value("a session ID")?)?),
                "--backup" => options.backup = true,
                "--fallback-on-429" => options.fallback = true,
                "--private" => options.private = true,
                "--usage" => options.usage = true,
                "--catalog" => options.catalog = true,
                "--help" | "-h" => options.help = true,
                #[cfg(debug_assertions)]
                "--preview" => options.preview = Some(value("a folder")?.into()),
                _ => return Err(format!("Unknown argument: {name}")),
            }
        }
        if options.backup && options.model.is_some() {
            return Err("Use either --backup or --model, not both.".into());
        }
        let needs_task = options.backup
            || options.fallback
            || options.private
            || options.model.is_some()
            || options.session.is_some()
            || options.effort.is_some()
            || options.plan
            || !options.attach.is_empty();
        if needs_task && options.run.is_none() {
            return Err("These options apply to a task. Add --run <task>.".into());
        }
        Ok(options)
    }

    /// Whether this invocation prints to the terminal instead of opening the window.
    pub fn is_terminal_command(&self) -> bool {
        self.help
            || self.usage
            || self.catalog
            || self.models
            || self.download
            || self.run.is_some()
    }
}

/// Release builds on Windows use the GUI subsystem and start without a console.
/// Reusing the launching terminal makes command-line output visible there.
pub fn attach_console() {
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn AttachConsole(process: u32) -> i32;
        }
        const PARENT_PROCESS: u32 = u32::MAX;
        unsafe {
            AttachConsole(PARENT_PROCESS);
        }
    }
}

pub fn execute(options: Options) -> ExitCode {
    let result = if options.help {
        println!("Super OpenCode {}\n\n{USAGE}", env!("CARGO_PKG_VERSION"));
        Ok(ExitCode::SUCCESS)
    } else if options.usage {
        usage()
    } else if options.catalog {
        catalog()
    } else if options.models {
        if let Err(message) = discover(None) {
            eprintln!("{message} Showing the built-in list.");
        }
        for model in models::all() {
            println!(
                "{}\t{}\t{}\t{}",
                model.id,
                model.name,
                model.reads(),
                models::provider_access(&model.provider)
            );
        }
        Ok(ExitCode::SUCCESS)
    } else if options.download {
        download()
    } else {
        task(options)
    };
    result.unwrap_or_else(|message| {
        eprintln!("{message}");
        ExitCode::FAILURE
    })
}

fn usage() -> Result<ExitCode, String> {
    let store = Store::open().map_err(|error| format!("Local storage is unavailable: {error}"))?;
    let settings = store.settings().unwrap_or_default();
    let file = store
        .usage_file()
        .map_err(|error| format!("Couldn’t read usage: {error}"))?;
    let source = settings.usage_source;
    let opencode = match source {
        crate::history::Source::OpenCode => crate::history::opencode_database(
            settings
                .executable
                .clone()
                .or_else(crate::runner::find_opencode)
                .as_deref(),
        ),
        crate::history::Source::App => None,
    };
    let history = crate::history::History::load(source, &file, opencode.as_deref())?;
    println!(
        "{}",
        crate::history::report(&history, crate::history::today())
    );
    let rows = store
        .usage_today()
        .map_err(|error| format!("Couldn’t read usage: {error}"))?;
    println!(
        "Super OpenCode today, {}",
        chrono::Local::now().date_naive()
    );
    println!("model\tcompleted_steps\tinput\toutput\treasoning\tcache_read\tcache_write");
    for row in rows {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            row.model,
            row.steps,
            row.input,
            row.output,
            row.reasoning,
            row.cache_read,
            row.cache_write
        );
    }
    Ok(ExitCode::SUCCESS)
}

/// Adds OpenCode's free models to the list, using the executable from Settings if one is set.
fn discover(executable: Option<std::path::PathBuf>) -> Result<(), String> {
    let binary = executable
        .or_else(runner::find_opencode)
        .ok_or("OpenCode was not found.")?;
    models::set_discovered(models::discover(&binary)?);
    Ok(())
}

fn download() -> Result<ExitCode, String> {
    let store = Store::open().map_err(|error| format!("Local storage is unavailable: {error}"))?;
    if let Some(path) = speech::installed(store.location()) {
        println!("Already installed: {}", path.display());
        return Ok(ExitCode::SUCCESS);
    }
    eprintln!(
        "Downloading {} ({}) from {}",
        speech::MODEL.name,
        speech::megabytes(speech::MODEL.bytes),
        speech::MODEL.url
    );
    let received = std::sync::atomic::AtomicU64::new(0);
    let path = speech::download(store.location(), &received, &INTERRUPTED)?;
    println!("Installed: {}", path.display());
    Ok(ExitCode::SUCCESS)
}

fn catalog() -> Result<ExitCode, String> {
    println!("{BUILTIN}");
    for model in runner::free_catalog()? {
        println!("{model}");
    }
    Ok(ExitCode::SUCCESS)
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Ctrl+C reaches only this process, because OpenCode runs in its own process group.
/// Turning the signal into a stop request lets the runner shut OpenCode down too.
/// Output as a terminal shows it: each finished step as a sentence, such as
/// `  Read src/main.rs (0.4s)`.
fn readable(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            let Some(notice) = crate::activity::parse(line.trim_end_matches('\n')) else {
                return line.to_owned();
            };
            let sentence = if notice.tool == "reasoning" {
                "Thought".to_owned()
            } else {
                notice.sentence()
            };
            let mut shown = format!("  {sentence}");
            if !notice.took.is_empty() {
                shown.push_str(&format!(" ({})", notice.took));
            }
            if notice.failed() {
                shown.push_str(" — failed");
            }
            shown.push('\n');
            shown
        })
        .collect()
}

fn watch_interrupts() {
    #[cfg(unix)]
    {
        extern "C" fn interrupted(_signal: libc::c_int) {
            INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let handler = interrupted as extern "C" fn(libc::c_int) as libc::sighandler_t;
        unsafe {
            libc::signal(libc::SIGINT, handler);
            libc::signal(libc::SIGTERM, handler);
        }
    }
}

fn task(options: Options) -> Result<ExitCode, String> {
    let prompt = options.run.unwrap_or_default();
    if prompt.trim().is_empty() {
        return Err("The task is empty.".into());
    }
    let store = Store::open()
        .inspect_err(|error| eprintln!("Usage will not be recorded: {error}"))
        .ok();
    let settings = store
        .as_ref()
        .and_then(|store| store.settings().ok())
        .unwrap_or_default();
    let wanted = [
        options.model.as_deref(),
        Some(settings.backup_model.as_str()),
    ];
    if wanted.iter().flatten().any(|id| models::find(id).is_none())
        && let Err(message) = discover(settings.executable.clone())
    {
        eprintln!("{message}");
    }
    let backup = if models::find(&settings.backup_model).is_some() {
        settings.backup_model.clone()
    } else {
        BACKUP.to_owned()
    };
    // With Auto on in Settings and no model named, Auto picks from the prompt and the files.
    let auto = (options.model.is_none()
        && !options.backup
        && !options.private
        && settings.model_mode == crate::auto::Mode::Auto)
        .then(|| {
            let kinds: Vec<_> = options
                .attach
                .iter()
                .filter_map(|path| attach::classify(path).ok())
                .collect();
            crate::auto::pick(&prompt, &kinds, options.plan, &settings.auto_models)
        });
    if let Some(pick) = &auto {
        eprintln!("Auto picked {} for {}.", pick.choice.label(), pick.reason);
    }
    let model = if options.backup {
        backup.clone()
    } else if let Some(pick) = &auto {
        pick.choice.model.clone()
    } else {
        options.model.unwrap_or_else(|| BUILTIN.to_owned())
    };
    if options.private && (model != BUILTIN || options.fallback) {
        return Err(
            "--private allows only the built-in model and turns off cross-provider fallback."
                .into(),
        );
    }
    let project = match options.project {
        Some(folder) => folder,
        None => env::current_dir().map_err(|error| error.to_string())?,
    };
    let directory = runner::canonical_project(&project).map_err(|error| error.to_string())?;

    let data = store.as_ref().map_or_else(
        || env::temp_dir().join("Super OpenCode"),
        |store| store.location().to_path_buf(),
    );
    let workdir = data
        .join("attachments")
        .join(uuid::Uuid::new_v4().to_string());
    let speech_model = speech::installed(&data);
    let mut sent = Vec::new();
    for path in &options.attach {
        let kind =
            attach::classify(path).map_err(|message| format!("{}: {message}", path.display()))?;
        if let Some((message, _)) = attach::refusal(kind, path, &model) {
            return Err(format!("{}: {message}", path.display()));
        }
        match attach::prepare(
            kind,
            path,
            Some(&directory),
            &workdir,
            speech_model.as_deref(),
            settings.dictation_language.as_deref(),
        ) {
            Ok(item) => {
                eprintln!("Attached {} ({})", item.name, item.note);
                sent.push(item);
            }
            Err(Problem::NeedsSpeechModel) => {
                return Err(format!(
                    "{}: audio needs the speech model. Run super-opencode --download-speech-model first.",
                    path.display()
                ));
            }
            Err(Problem::Failed(message)) => return Err(format!("{}: {message}", path.display())),
        }
    }

    let variant = options
        .effort
        .clone()
        .or_else(|| auto.as_ref().and_then(|pick| pick.choice.effort.clone()))
        .or_else(|| settings.efforts.get(&model).cloned());
    let task = Task {
        directory,
        binary: settings.executable.clone(),
        fallback: options.fallback && model != backup,
        prompt: format!("{prompt}{}", attach::describe(&sent)),
        model,
        session: options.session,
        files: sent.iter().filter_map(|item| item.file.clone()).collect(),
        backup,
        auto_approve: settings.auto_approve && !options.private,
        time_limit: (settings.time_limit_minutes > 0)
            .then(|| std::time::Duration::from_secs(u64::from(settings.time_limit_minutes) * 60)),
        variant,
        agent: options.plan.then(|| "plan".to_owned()),
        live: false,
    };

    watch_interrupts();
    let session = RefCell::new(None);
    let outcome = RefCell::new(None);
    runner::run(task, &INTERRUPTED, |event| match event {
        Event::Output(text) => {
            // A closed pipe is not worth a panic; the run still finishes and is recorded.
            let mut stdout = io::stdout().lock();
            let _ = stdout.write_all(readable(&text).as_bytes());
            let _ = stdout.flush();
        }
        Event::Activity { .. } | Event::Server(_) => {}
        Event::Session(id) => *session.borrow_mut() = Some(id),
        Event::Limited => {}
        Event::Usage {
            model,
            timestamp,
            part,
        } => {
            if let Some(store) = &store
                && let Err(error) = store.record(&model, timestamp, &part)
            {
                eprintln!("Couldn’t record usage: {error}");
            }
        }
        Event::Finished(result) => *outcome.borrow_mut() = Some(result),
    });

    if let Some(id) = session.into_inner() {
        eprintln!("Session {id}");
    }
    match outcome.into_inner() {
        Some(Ok(())) => Ok(ExitCode::SUCCESS),
        Some(Err(message)) if message == "Stopped" => Ok(ExitCode::from(130)),
        Some(Err(message)) => Err(message),
        None => Err("The run ended without a result.".into()),
    }
}
