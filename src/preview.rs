//! Debug builds only. `--preview <folder>` draws the interface in a set of fixed states with
//! sample data and saves each as a PNG, so layout and colour can be checked without driving
//! the app by hand. Nothing here reads or writes the user's sessions.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Theme, ViewportCommand};
use uuid::Uuid;

use crate::{
    app::{App, Status, View},
    storage::{Session, Turn, Usage},
};

const SCENES: &[&str] = &[
    "welcome-light",
    "chat-light",
    "attachments-light",
    "sent-light",
    "listening-light",
    "running-light",
    "running-early-light",
    "failed-light",
    "usage-light",
    "usage-lower-light",
    "settings-light",
    "settings-lower-light",
    "providers-light",
    "menu-add-light",
    "menu-access-light",
    "menu-model-light",
    "menu-context-light",
    "confirm-light",
    "no-project-light",
    "chat-dark",
    "attachments-dark",
    "menu-context-dark",
    "confirm-dark",
    "welcome-dark",
    "settings-dark",
    "running-dark",
    "usage-dark",
    "auto-light",
    "menu-model-auto-light",
    "settings-auto-light",
    "tutorial-light",
    "tutorial-models-light",
    "tutorial-providers-light",
    "tutorial-dark",
    "help-light",
    "browser-light",
    "pane-browser-new-light",
    "pane-tasks-light",
    "pane-changes-light",
    "pane-diff-light",
    "pane-files-light",
    "pane-terminal-light",
    "terminal-side-light",
    "pane-wide-light",
    "pane-diff-dark",
    "menu-more-light",
    "link-light",
    "images-light",
    "queue-light",
    "parallel-light",
    "parallel-dark",
    "table-light",
    "table-dark",
    "signs-light",
    "signs-dark",
];

/// Frames to draw before capturing, so layout and fonts have settled.
const SETTLE: u32 = 8;

pub struct Preview {
    folder: PathBuf,
    /// The scenes to capture: all, or those whose names contain `SUPER_OPENCODE_SCENES`.
    scenes: Vec<&'static str>,
    scene: usize,
    frames: u32,
    waited: u32,
}

impl Preview {
    pub fn drive(&mut self, app: &mut App, ctx: &egui::Context) {
        let shot = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(shot) = shot
            && let Some(name) = self.scenes.get(self.scene)
        {
            let [width, height] = shot.size;
            let path = self.folder.join(format!("{name}.png"));
            match image::RgbaImage::from_raw(width as u32, height as u32, shot.as_raw().to_vec()) {
                Some(picture) => {
                    if let Err(error) = picture.save(&path) {
                        eprintln!("Couldn’t save {}: {error}", path.display());
                    }
                }
                None => eprintln!("The capture for {} had an unexpected size.", path.display()),
            }
            self.scene += 1;
            self.frames = 0;
        }
        let Some(scene) = self.scenes.get(self.scene).copied() else {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        };
        if self.frames == 0 {
            app.stage(scene, ctx, &self.folder);
            // macOS stops painting a window it thinks is hidden; keep this one in front.
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(egui::WindowLevel::AlwaysOnTop));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }
        self.frames += 1;
        // Wait for picture previews, which load in the background, for up to two seconds.
        // Picture previews load in the background, and a shell takes a moment to print.
        let waiting =
            app.inputs.previews_loading() || scene.contains("terminal") && self.waited < 90;
        if self.frames == SETTLE && waiting && self.waited < 120 {
            self.frames -= 1;
            self.waited += 1;
            std::thread::sleep(std::time::Duration::from_millis(16));
        } else if self.frames == SETTLE {
            self.waited = 0;
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint();
    }
}

const REPLY: &str = "\
reasoning · completed · 2.1s
read · completed · src/routes/invoices.rs · 0.1s
grep · completed · “total” in src/billing · 0.2s
The handler in `src/routes/invoices.rs` builds the total with `f64`, so sums such as 0.1 + 0.2 pick up binary rounding error before they are stored.

## What I changed
- **Amounts are integers now.** `Invoice::total` returns cents as `i64`, and formatting happens only at the edge.
- `parse_amount` rejects more than two decimal places instead of rounding them away.
- Added a regression test for the three-line invoice from the bug report.

apply_patch · completed · src/billing/invoice.rs and 2 more · 0.1s
shell · completed · cargo test --locked · 14s
```rust
pub fn total(&self) -> i64 {
    self.lines.iter().map(|line| line.quantity * line.unit_cents).sum()
}
```

All 42 tests pass, including the new `total_is_exact_for_fractional_prices`.";

const TABLE_REPLY: &str = "\
reasoning · completed · 13s
shell · completed · npm run test:totp 2>&1 | tail -1 · 4.1s
shell · completed · cd rust && cargo build 2>&1 | tail -1 · 38s
All automated tests pass:

| Test | Result |
|---|---|
| Rust debug build | ✅ |
| Vault read from real data (bundled app) | ✅ 2 accounts |
| Write round-trip (Rust encrypt → Rust read) | ✅ |
| PIN setup/verify (scrypt, temp copy) | ✅ |
| Independent Node read of the real vault (format check) | ✅ 2 accounts |
| Code signature on installed app | ❌ unsigned build |
| RFC 6238 TOTP vectors | ✅ |

Keyring is launching now. Quick checklist:

1. Touch ID unlocks and your 2 accounts (*Fluid Platform Super Admin*, *Fluid*) show with live codes
2. Menu bar icon: left-click toggles the window, right-click has **Lock Now** / **Quit**
3. Red close button hides to the tray (doesn’t lock); window reopens with your session
4. Hide codes (eye) → re-reveal asks for Touch ID
5. Copy a code → clipboard clears after your configured delay
6. Delete an account in the Rust app, then confirm it’s gone in a vault read (or re-add it)

When you’re satisfied, reply **OK** and I’ll commit.";

const SAMPLE_DIFF: &str = "\
diff --git a/src/billing/invoice.rs b/src/billing/invoice.rs
index 3f2a1c9..8b7e0d4 100644
--- a/src/billing/invoice.rs
+++ b/src/billing/invoice.rs
@@ -12,14 +12,19 @@ pub struct Invoice {
     pub id: InvoiceId,
     pub customer: CustomerId,
     pub lines: Vec<Line>,
+    /// ISO 4217 code, such as USD. Backfilled from the customer for older invoices.
+    pub currency: Currency,
 }
 
 impl Invoice {
-    pub fn total(&self) -> f64 {
-        self.lines.iter().map(|line| line.quantity as f64 * line.unit_price).sum()
+    /// The total in the currency's smallest unit, so sums stay exact.
+    pub fn total(&self) -> i64 {
+        self.lines
+            .iter()
+            .map(|line| line.quantity * line.unit_cents)
+            .sum()
     }
 }
@@ -40,7 +45,7 @@ impl Display for Invoice {
     fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
-        write!(f, \"{:.2}\", self.total())
+        write!(f, \"{}\", self.currency.format(self.total()))
     }
 }";

const PARTIAL: &str = "\
reasoning · completed · 3.4s
read · completed · migrations/0007_add_currency.sql · 0.1s
read · completed · src/models/customer.rs · 0.1s
shell · completed · cargo build · 41s
The migration adds a `currency` column with a default of `USD`, then backfills it from the customer record. I am running the migration against the test database now";

/// Eleven months of made-up activity: busier weekdays, quiet weekends, a few breaks, and a
/// current streak. The same every run.
fn sample_history(sessions: &[Session]) -> crate::history::History {
    use chrono::Days;
    let models = [
        "opencode/glm-5.3-flash-free",
        "opencode/deepseek-v4.1-flash-free",
        "openrouter/cohere/north-mini-code:free",
        "opencode/mimo-v2.6-flash-free",
        "nvidia/openai/gpt-oss-20b",
        "ollama/qwen3:8b",
        "opencode/minimax-m3.1-flash-free",
    ];
    let mut seed: u64 = 0x5EED_CAFE;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) as u32
    };
    let today = crate::history::today();
    let mut steps = Vec::new();
    for back in (0..330u64).rev() {
        let day = today - Days::new(back);
        let weekend = day.format("%u").to_string().parse::<u32>().unwrap_or(1) >= 6;
        let roll = next() % 100;
        // A break in early summer, and fewer weekends.
        let idle = (back > 140 && back < 158) || (weekend && roll < 65) || roll < 18;
        if idle && back > 9 {
            continue;
        }
        let count = 2 + next() % if weekend { 6 } else { 22 };
        for step in 0..count {
            let pick = (next() % 100) as usize;
            let model = match pick {
                0..=51 => models[0],
                52..=69 => models[1],
                70..=81 => models[2],
                82..=89 => models[3],
                90..=94 => models[4],
                95..=97 => models[5],
                _ => models[6],
            };
            let session = if back < 30 {
                Some(format!(
                    "ses_sample{}",
                    next() as usize % sessions.len().max(1)
                ))
            } else {
                Some(format!("ses_old{}", back / 3))
            };
            let at = day
                .and_hms_opt(9 + step % 9, next() % 60, 0)
                .and_then(|time| time.and_local_timezone(chrono::Local).single())
                .map_or(0, |time| time.timestamp_millis());
            steps.push(crate::history::Step {
                at,
                day,
                model: model.into(),
                session,
                tokens: u64::from(800 + next() % 14_000),
                cost: 0.0,
            });
        }
    }
    crate::history::History {
        source: crate::history::Source::OpenCode,
        steps,
        sessions: std::collections::HashMap::new(),
    }
}

fn session(project: &Path, minutes_ago: f64, prompts: &[(&str, &str)]) -> Session {
    Session {
        id: Uuid::new_v4(),
        project_path: project.to_path_buf(),
        route_id: "opencode/glm-5.3-flash-free".into(),
        server_session: None,
        turns: prompts
            .iter()
            .map(|(prompt, output)| Turn {
                id: Uuid::new_v4(),
                prompt: (*prompt).into(),
                model: "GLM 5.3 Flash".into(),
                output: (*output).into(),
                attachments: Vec::new(),
            })
            .collect(),
        updated: Session::revision_time() - minutes_ago * 60.0,
        context_tokens: 0,
        spent: Default::default(),
    }
}

fn usage(model: &str, numbers: [i64; 6]) -> Usage {
    let [steps, input, output, reasoning, cache_read, cache_write] = numbers;
    Usage {
        model: model.into(),
        steps,
        input,
        output,
        reasoning,
        cache_read,
        cache_write,
    }
}

impl App {
    pub(crate) fn for_preview(logo: egui::TextureHandle, folder: PathBuf) -> Self {
        let mut app = Self::empty(logo);
        // Screenshots use the built-in list, so they don't depend on this computer's accounts.
        app.discovery_started = true;
        let only = std::env::var("SUPER_OPENCODE_SCENES").unwrap_or_default();
        app.preview = Some(Preview {
            folder,
            scenes: SCENES
                .iter()
                .copied()
                .filter(|scene| only.split(',').any(|part| scene.contains(part.trim())))
                .collect(),
            scene: 0,
            frames: 0,
            waited: 0,
        });
        app
    }

    /// Puts the app into the named state. The data is sample data and is labelled as such
    /// by living only in this module.
    fn stage(&mut self, scene: &str, ctx: &egui::Context, folder: &Path) {
        let (name, theme) = scene.rsplit_once('-').unwrap_or((scene, "light"));
        // Sample catalog: GLM's effort levels, one connected provider, and one local model.
        let sample = |id: &str, name: &str, variants: &[&str]| crate::models::Model {
            id: id.into(),
            name: name.into(),
            provider: id.split('/').next().unwrap_or_default().into(),
            images: &[],
            video: &[],
            context: 131_072,
            variants: variants
                .iter()
                .map(|variant| (*variant).to_owned())
                .collect(),
        };
        crate::models::set_discovered(vec![
            sample(
                crate::models::DEFAULT,
                "GLM 5.3 Flash",
                &["low", "high", "max"],
            ),
            sample("nvidia/openai/gpt-oss-20b", "GPT OSS 20B", &[]),
            sample("ollama/qwen3:8b", "qwen3:8b", &[]),
        ]);
        ctx.set_theme(if theme == "dark" {
            Theme::Dark
        } else {
            Theme::Light
        });
        let home = dirs::home_dir().unwrap_or_default();
        let project = home.join("code").join("ledger-api");
        let fix = "The invoice total is off by a cent for some invoices. Find the cause in the billing module and fix it, with a test.";
        let mut first = session(&project, 4.0, &[(fix, REPLY)]);
        first.context_tokens = 48_213;
        first.spent = crate::storage::Spent {
            steps: 12,
            read: 310_406,
            written: 8_420,
        };
        self.sessions = vec![
            first,
            session(
                &project,
                95.0,
                &[("Add a currency column to invoices and backfill it", "")],
            ),
            session(
                &project,
                60.0 * 5.0,
                &[("Explain how the retry queue decides when to give up", "")],
            ),
            session(
                &project,
                60.0 * 27.0,
                &[("Why does cargo test hang on the webhook suite?", "")],
            ),
            session(
                &project,
                60.0 * 24.0 * 3.0,
                &[("Rename LedgerEntry to JournalLine everywhere", "")],
            ),
        ];
        // Two other projects, so the sidebar shows its groups.
        let other = home.join("code").join("mobile-app");
        let site = home.join("code").join("marketing-site");
        let mut elsewhere = vec![
            session(
                &other,
                60.0 * 30.0,
                &[("Fix the crash when the camera permission is denied", "")],
            ),
            session(
                &other,
                60.0 * 50.0,
                &[("Add pull to refresh on the orders screen", "")],
            ),
            session(
                &site,
                60.0 * 24.0 * 2.0,
                &[("Make the pricing table fit on phones", "")],
            ),
        ];
        self.sessions.append(&mut elsewhere);
        self.settings = crate::storage::Settings::default();
        self.settings.project = Some(project.clone());
        self.settings.fallback = true;
        self.settings.executable = None;
        self.detected = Some(home.join(".opencode").join("bin").join("opencode"));
        self.usage = Ok(vec![
            usage(
                "opencode/glm-5.3-flash-free",
                [31, 412_880, 18_342, 6_120, 301_556, 22_410],
            ),
            usage(
                "openrouter/cohere/north-mini-code:free",
                [6, 58_204, 3_911, 0, 12_800, 4_096],
            ),
        ]);
        self.active = None;
        self.inputs = crate::inputs::Inputs::default();
        self.view = View::Chat;
        self.status = Status::Ready;
        self.task_error = None;
        self.storage_error = None;
        self.limited = false;
        self.staged_running = false;
        self.connections.clear();
        self.preview_menu = None;
        self.plan_mode = false;
        self.confirm_full_access = false;
        self.ask_rules = None;
        self.settings
            .efforts
            .insert(crate::models::DEFAULT.into(), "high".into());
        self.connecting = None;
        self.provider_message = None;
        self.route = 0;
        self.prompt.clear();
        self.focus_composer = false;
        self.history = None;
        self.parked.clear();
        self.queued.clear();
        self.doing = None;
        self.preview_signs = name == "signs";
        self.tutorial = None;
        self.settings_section = crate::views::Section::Models;
        self.settings_query.clear();
        self.sidebar_toggled.clear();
        self.browser = None;
        self.pane = None;
        self.pane_wide = false;
        self.terminals.clear();
        self.terminal_open = false;
        self.changes = Default::default();
        self.files = Default::default();
        self.todos = Default::default();
        self.usage_days = 30;
        self.usage_year = None;
        // Sessions the sample usage history can point back to.
        for (index, session) in self.sessions.iter_mut().enumerate() {
            session.server_session = Some(format!("ses_sample{index}"));
        }
        match name {
            "chat" => {
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.prompt = "Now do the same for credit notes".into();
            }
            "running" => {
                let mut current = self.sessions[1].clone();
                current.turns[0].output = PARTIAL.into();
                self.active = Some(current);
                self.status = Status::Running;
                self.staged_running = true;
                let now = crate::runner::now_millis();
                self.task_started = now - 128_000;
                self.doing = Some((
                    "Running sqlx migrate run --database-url $TEST_DATABASE_URL".into(),
                    now - 12_000,
                ));
            }
            "running-early" => {
                let mut current = self.sessions[1].clone();
                current.turns[0].output = String::new();
                self.active = Some(current);
                self.status = Status::Running;
                self.staged_running = true;
                let now = crate::runner::now_millis();
                self.task_started = now - 4_000;
                self.doing = Some(("Thinking".into(), now - 4_000));
            }
            "failed" => {
                let mut current = self.sessions[2].clone();
                current.turns[0].output = "read · completed\n".into();
                self.active = Some(current);
                self.status = Status::Failed;
                self.limited = true;
                self.settings.fallback = false;
                self.task_error = Some(
                    "opencode/glm-5.3-flash-free reached its free usage limit (provider.free-limit). Try another free model.\nOpenCode reported: Free usage limit reached. Connect a provider or switch models."
                        .into(),
                );
            }
            "usage" => {
                self.view = View::Usage;
                self.history = Some(Ok(std::sync::Arc::new(sample_history(&self.sessions))));
            }
            "usage-lower" => {
                self.view = View::Usage;
                self.history = Some(Ok(std::sync::Arc::new(sample_history(&self.sessions))));
                self.scroll_settings_to = Some(560.0);
            }
            "welcome" => {
                self.history = Some(Ok(std::sync::Arc::new(sample_history(&self.sessions))));
            }
            "auto" => {
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.settings.model_mode = crate::auto::Mode::Auto;
                self.prompt = "What does parse_amount return for \"1.005\"?".into();
                if let Some(session) = &mut self.active {
                    session.turns[0].model =
                        "DeepSeek V4.1 Flash · High · Auto: a larger change or investigation"
                            .into();
                }
            }
            "menu-model-auto" => {
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.settings.model_mode = crate::auto::Mode::Auto;
                self.preview_menu = Some(crate::app::Menu::Model);
            }
            "settings-auto" => {
                self.settings.model_mode = crate::auto::Mode::Auto;
                self.store = crate::storage::Store::open().ok();
                self.view = View::Settings;
                self.scroll_settings_to = Some(0.0);
            }
            "tutorial" => {
                self.tutorial = Some(0);
            }
            "tutorial-models" => {
                self.tutorial = Some(3);
            }
            "tutorial-providers" => {
                self.tutorial = Some(4);
                self.connections = vec![crate::providers::Connection {
                    provider: "nvidia".into(),
                    credentials: vec!["cred_superopencode0nvidia".into()],
                }];
            }
            "help" => {
                self.store = crate::storage::Store::open().ok();
                self.view = View::Settings;
                self.settings_section = crate::views::Section::Help;
                self.scroll_settings_to = Some(0.0);
            }
            "pane-browser-new" => {
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.browser = Some(crate::browser::Browser::new(None));
                self.pane = Some(crate::panes::Pane::Browser);
            }
            "pane-tasks" => {
                let mut current = self.sessions[1].clone();
                current.turns[0].output = PARTIAL.into();
                self.active = Some(current);
                self.status = Status::Running;
                self.staged_running = true;
                let now = crate::runner::now_millis();
                self.task_started = now - 128_000;
                self.doing = Some(("Running cargo test --locked".into(), now - 9_000));
                self.pane = Some(crate::panes::Pane::Tasks);
                self.todos.session = Some("ses_sample1".into());
                self.todos.read_at = Some(std::time::Instant::now());
                let todo = |content: &str, status: &str, priority: &str| crate::panes::Todo {
                    content: content.into(),
                    status: status.into(),
                    priority: priority.into(),
                };
                self.todos.items = Ok(vec![
                    todo(
                        "Add a currency column to the invoices table",
                        "completed",
                        "high",
                    ),
                    todo(
                        "Backfill currency from each customer record",
                        "completed",
                        "high",
                    ),
                    todo(
                        "Run the migration against the test database",
                        "in_progress",
                        "high",
                    ),
                    todo(
                        "Update Invoice serialisation to include currency",
                        "pending",
                        "medium",
                    ),
                    todo(
                        "Add a test for invoices with no customer",
                        "pending",
                        "medium",
                    ),
                    todo(
                        "Drop the old default once every row has a value",
                        "cancelled",
                        "low",
                    ),
                ]);
            }
            "pane-changes" | "pane-diff" | "pane-wide" => {
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.pane = Some(crate::panes::Pane::Changes);
                self.pane_wide = name == "pane-wide";
                self.changes.project = Some(project.clone());
                let changed =
                    |path: &str, status: &str, added: u64, removed: u64| crate::panes::Changed {
                        path: path.into(),
                        status: status.into(),
                        added: Some(added),
                        removed: Some(removed),
                    };
                self.changes.list = Some(Ok(vec![
                    changed("migrations/0007_add_currency.sql", "??", 14, 0),
                    changed("src/billing/invoice.rs", "M", 22, 9),
                    changed("src/billing/parse.rs", "M", 6, 2),
                    changed("src/routes/invoices.rs", "M", 3, 3),
                    changed("tests/invoice_total.rs", "A", 41, 0),
                ]));
                if name != "pane-changes" {
                    self.changes.selected = Some("src/billing/invoice.rs".into());
                    self.changes.diff = Some(Ok(SAMPLE_DIFF.into()));
                }
            }
            "pane-files" => {
                let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                self.settings.project = Some(here.clone());
                self.sessions
                    .iter_mut()
                    .for_each(|session| session.project_path = here.clone());
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.pane = Some(crate::panes::Pane::Files);
                self.files.root = Some(here.clone());
                self.files.expanded.insert(here.join("src"));
            }
            "pane-terminal" => {
                let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                self.settings.project = Some(here);
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.terminal_open = true;
                self.settings.terminal_side = false;
            }
            "queue" => {
                // A task running, a message being typed, and two waiting for it to finish.
                let mut current = self.sessions[1].clone();
                current.turns[0].output = PARTIAL.into();
                let id = current.id;
                self.active = Some(current);
                self.status = Status::Running;
                self.staged_running = true;
                let now = crate::runner::now_millis();
                self.task_started = now - 64_000;
                self.doing = Some(("Running sqlx migrate run".into(), now - 8_000));
                self.prompt = "Use EUR for customers in Germany".into();
                self.queued.insert(
                    id,
                    vec![
                        crate::app::Queued {
                            prompt: "Then add a test for invoices with no customer".into(),
                            attachments: Vec::new(),
                        },
                        crate::app::Queued {
                            prompt: "And update the README’s billing section".into(),
                            attachments: Vec::new(),
                        },
                    ],
                );
            }
            "images" => {
                // Pictures read from the capture folder, which holds earlier screenshots.
                let mut current = self.sessions[3].clone();
                current.project_path = folder.to_path_buf();
                current.turns[0].prompt = "Match the approved mockups in ui/".into();
                current.turns[0].output =
                    "Let me look at the design mockups — those are the approved look.\n\
read · completed · welcome-light.png · 0.5s\n\
read · completed · usage-light.png · 1.2s\n\
read · completed · settings-light.png · 1.1s\n\
read · completed · tutorial-light.png · 0.6s\n\
read · completed · pane-diff-light.png · 0.9s\n\
read · completed · running-light.png · 0.5s\n\
read · completed · src/styles/theme.css · 0.2s\n\
The mockups use a light ground with one blue accent and square cards."
                        .into();
                self.active = Some(current);
                self.status = Status::Complete;
            }
            "parallel" => {
                // Three tasks at once: one on screen, two in the sidebar, and one finished
                // while out of view.
                let mut current = self.sessions[1].clone();
                current.turns[0].output = PARTIAL.into();
                self.active = Some(current);
                self.status = Status::Running;
                self.staged_running = true;
                let now = crate::runner::now_millis();
                self.task_started = now - 128_000;
                self.doing = Some(("Running sqlx migrate run".into(), now - 12_000));
                let ids: Vec<Uuid> = self.sessions.iter().map(|session| session.id).collect();
                self.parked.insert(
                    ids[3],
                    crate::app::Parked::staged(
                        Status::Running,
                        Some("Reading src/webhooks/suite.rs"),
                    ),
                );
                self.parked.insert(
                    ids[5],
                    crate::app::Parked::staged(Status::Running, Some("Thinking")),
                );
                self.parked
                    .insert(ids[2], crate::app::Parked::staged(Status::Complete, None));
                self.sidebar_toggled
                    .insert(self.sessions[5].project_path.clone());
            }
            "table" => {
                let mut current = self.sessions[3].clone();
                current.turns[0].prompt =
                    "Run every test and tell me what still needs checking by hand".into();
                current.turns[0].model = "GLM 5.3 Flash · Auto: an everyday task".into();
                current.turns[0].output = TABLE_REPLY.into();
                self.active = Some(current);
                self.status = Status::Complete;
                self.settings.transcript = crate::storage::Transcript::Thinking;
                self.scroll_settings_to = None;
            }
            "link" => {
                let mut current = self.sessions[3].clone();
                current.turns[0].prompt =
                    "What is the GitHub link for this repo? See https://github.com/desmondknunoo"
                        .into();
                current.turns[0].output = "read · completed · .git/config · 0.0s\nThe GitHub link is: https://github.com/desmondknunoo/keyring (from the `origin` remote of this repo).\n\nYou can also open it with [the repository page](https://github.com/desmondknunoo/keyring), or clone it with **git clone https://github.com/desmondknunoo/keyring.git** from any folder. A long sentence keeps going so the link wraps onto the next line when it falls near the end of the column: see https://github.com/desmondknunoo/keyring/blob/main/README.md for the setup notes.".into();
                self.active = Some(current);
                self.status = Status::Complete;
            }
            "terminal-side" => {
                let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                self.settings.project = Some(here);
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.terminal_open = true;
                self.settings.terminal_side = true;
                self.browser = Some(crate::browser::Browser::new(None));
                self.pane = Some(crate::panes::Pane::Browser);
            }
            "menu-more" => {
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.preview_menu = Some(crate::app::Menu::More);
            }
            "browser" => {
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.settings.links_in_app = true;
                self.browser = Some(crate::browser::Browser::new(Some(
                    "https://github.com/desmondknunoo".into(),
                )));
                self.pane = Some(crate::panes::Pane::Browser);
            }
            "attachments" => {
                use crate::{
                    attach::{Kind, Sent},
                    inputs::{Pending, Prep},
                };
                let item = |name: &str, kind: Kind, prep: Prep| Pending {
                    id: Uuid::new_v4(),
                    source: project.join(name),
                    name: name.into(),
                    kind,
                    prep,
                };
                let ready = |name: &str, note: &str| {
                    Prep::Ready(Sent {
                        name: name.into(),
                        note: note.into(),
                        file: None,
                        reference: None,
                        source: None,
                    })
                };
                // Real pictures for the previews: earlier captures and the app's mark.
                let shot = folder.join("chat-light.png");
                let earlier = folder.join("welcome-light.png");
                let mark = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/logo.png"));
                self.active = Some(self.sessions[0].clone());
                if let Some(session) = &mut self.active {
                    session.turns[0].attachments = vec![
                        Sent {
                            name: "invoice-bug.png".into(),
                            note: "image, 214 KB".into(),
                            file: Some(earlier.clone()),
                            reference: None,
                            source: Some(earlier),
                        },
                        Sent {
                            name: "billing-module-notes.md".into(),
                            note: "3 KB".into(),
                            file: None,
                            reference: None,
                            source: Some(project.join("billing-module-notes.md")),
                        },
                    ];
                }
                self.status = Status::Complete;
                self.route = crate::models::index_of(crate::models::BACKUP).unwrap_or_default();
                self.prompt = "Compare the spec with the handler and list what is missing".into();
                self.inputs.pending = vec![
                    item(
                        "billing-spec.pdf",
                        Kind::Document,
                        ready("billing-spec.pdf", "text, 4.2k words"),
                    ),
                    Pending {
                        id: Uuid::new_v4(),
                        source: shot,
                        name: "screenshot.png".into(),
                        kind: Kind::Image,
                        prep: ready("screenshot.png", "image, 312 KB"),
                    },
                    Pending {
                        id: Uuid::new_v4(),
                        source: mark,
                        name: "mark.png".into(),
                        kind: Kind::Image,
                        prep: ready("mark.png", "image, 1.1 MB"),
                    },
                    item("standup.m4a", Kind::Audio, Prep::NeedsModel),
                    item("src", Kind::Folder, ready("src", "in this project")),
                    item("ledger-export.xlsx", Kind::Document, Prep::Preparing),
                ];
                self.inputs.offer_model = true;
            }
            "sent" => {
                // A sent prompt with a picture and a document, and a short reply.
                let earlier = folder.join("welcome-light.png");
                let mut current = self.sessions[3].clone();
                current.turns[0].attachments = vec![
                    crate::attach::Sent {
                        name: "hang-trace.png".into(),
                        note: "image, 214 KB".into(),
                        file: Some(earlier.clone()),
                        reference: None,
                        source: Some(earlier),
                    },
                    crate::attach::Sent {
                        name: "webhook-suite.log".into(),
                        note: "18 KB".into(),
                        file: None,
                        reference: None,
                        source: Some(project.join("webhook-suite.log")),
                    },
                    crate::attach::Sent {
                        name: "ci-run-4182.pdf".into(),
                        note: "text, 1.3k words".into(),
                        file: None,
                        reference: None,
                        source: Some(project.join("ci-run-4182.pdf")),
                    },
                ];
                current.turns[0].output = "read · completed\nThe webhook tests wait on a channel that the mock server never closes, so the suite hangs after the last test.".into();
                current.context_tokens = 21_406;
                self.active = Some(current);
                self.status = Status::Complete;
            }
            "listening" => {
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.prompt = "Add a regression test for".into();
                self.inputs.model = crate::inputs::ModelState::Ready(PathBuf::from("model.bin"));
                self.inputs.staged_listening = Some((0.18, std::time::Duration::from_secs(7)));
            }
            "providers" => {
                // NVIDIA connected from this app; ModelScope's key box open.
                self.connections = vec![crate::providers::Connection {
                    provider: "nvidia".into(),
                    credentials: vec!["cred_superopencode0nvidia".into()],
                }];
                self.provider_message = Some((
                    "NVIDIA NIM is connected. 16 free models are in the picker.".into(),
                    false,
                ));
                self.connecting = Some("modelscope");
                self.view = View::Settings;
                self.settings_section = crate::views::Section::Providers;
                self.scroll_settings_to = Some(0.0);
            }
            "confirm" => {
                // The rules OpenCode's build agent asks about on this computer.
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.confirm_full_access = true;
                self.ask_rules = Some(Ok(vec!["Read *.env".into(), "Read *.env.*".into()]));
            }
            "menu-add" | "menu-access" | "menu-model" | "menu-context" => {
                self.active = Some(self.sessions[0].clone());
                self.status = Status::Complete;
                self.preview_menu = Some(match name {
                    "menu-add" => crate::app::Menu::Add,
                    "menu-access" => crate::app::Menu::Access,
                    "menu-model" => crate::app::Menu::Model,
                    _ => crate::app::Menu::Context,
                });
            }
            "settings-lower" => {
                self.store = crate::storage::Store::open().ok();
                self.view = View::Settings;
                self.settings.auto_approve = true;
                self.settings.time_limit_minutes = 30;
                self.inputs.model = crate::inputs::ModelState::Ready(PathBuf::from(
                    "~/Library/Application Support/Super OpenCode/models/ggml-base-q5_1.bin",
                ));
                self.settings_section = crate::views::Section::Appearance;
                self.scroll_settings_to = Some(0.0);
            }
            "settings" => {
                self.discovery_status = "27 free models from 4 providers. Checked at 15:58.".into();
                // Only the folder's path is shown; nothing in it is read.
                self.store = crate::storage::Store::open().ok();
                self.view = View::Settings;
            }
            "no-project" => {
                self.settings.project = None;
                self.sessions.clear();
                self.detected = None;
                self.usage = Ok(Vec::new());
            }
            _ => {}
        }
    }
}
