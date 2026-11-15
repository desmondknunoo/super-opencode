# Super OpenCode guide

Super OpenCode is a Rust desktop coding harness for macOS, Windows, and Linux. It runs the OpenCode 2.x CLI in your selected project on free models, streams its output, and keeps session history and usage on your computer. It needs no Python runtime.

## Start

1. OpenCode installs itself: when the app doesn’t find the [OpenCode 2.x CLI](https://opencode.ai/docs/), it runs OpenCode’s own installer in the background (`curl -fsSL https://opencode.ai/install | bash` on macOS and Linux; npm or Scoop on Windows) and shows its progress in the status bar. **Settings › OpenCode** can install it again or use another executable.
2. On macOS, unzip the archive and open **Super OpenCode.app**. On Windows, extract the ZIP and run **super-opencode.exe**. On Linux, extract the archive and run `./super-opencode` from your graphical session. The Linux x64 package needs glibc 2.36 or later, an X11 or Wayland session with OpenGL, and ALSA for the microphone.
3. The tutorial opens on first launch: installing OpenCode, choosing a project, how models are picked, and free keys for more models. **Settings › Help › Show Tutorial** opens it again.
4. Choose a project folder, pick a model in the box where you type, and press **Return** to run the task. **Shift+Return** adds a new line. **Esc** stops a running task, dictation, or reading aloud. **⌘N** (Ctrl+N on Windows and Linux) starts a new session and **⌘,** (Ctrl+,) opens Settings. **Sessions** in the sidebar lists earlier chats grouped by project folder, the current project first; click a group to open or close it, and a session from another project switches to that project. Sessions run at the same time, in any projects: start a task, open another session or press **⌘N**, and start another. A running session shows the sign for what it is doing; one that finished while out of view shows a dot (green done, red failed, amber stopped). Right-click a running session to stop it. **Settings › Tasks › Tasks at once** sets how many may run together (3 by default). Text and attachments not sent yet stay with their session when you switch away.
5. OpenCode models need no account. For OpenRouter models, paste a key with **Connect…** next to OpenRouter in the model picker or in **Settings › Models › Free accounts**, use `/connect` in OpenCode, or start Super OpenCode with `OPENROUTER_API_KEY` already exported in its environment.

The Mac app reads the previous Swift app’s saved chats and usage database. Each chat updated by the Rust app is saved separately.

## The composer

- **Project bar** above the box: the folder OpenCode works in and its Git branch. Click it to choose another folder.
- **+**: attach files or a folder, choose the project, or turn on **Plan mode**, which runs OpenCode’s plan agent. It explores and plans without editing files, though it can still run commands.
- **Access**: what OpenCode may do without asking. A task can’t stop to ask you, so anything not allowed is declined and the task carries on.
  - **Standard access** (the default) runs commands and edits files in the project; requests OpenCode’s rules mark “ask”, such as reading `.env` files, are declined.
  - **Full access** also approves those requests. Actions its rules deny stay denied. Choosing it first asks you to confirm, listing what your OpenCode rules would ask about on this computer.
- **Model and effort**: choose **Manual** to run every task on one model, or **Auto** to have a model picked for each task (see below). In Manual, click the model to choose another, or an effort level such as Low, High, or Max where the model offers one. The effort is remembered per model.
- **Context ring**, under the box on the right: how much of the model’s context window the session has used, with what is left beside it. Click it for the numbers, this session’s steps and tokens, and today’s total. A new conversation starts at about 9K tokens, mostly OpenCode’s own instructions and tool descriptions.
- **While a task runs**, a line under the reply says what OpenCode is doing now, such as *Thinking*, *Reading src/main.rs*, or *Running cargo test*, with how long that step has taken and the task’s total. The app learns this from a private OpenCode server it starts for each task, listening only on this computer behind a password made for that task, and stops when the task ends. Each finished step stays in the reply as a row, such as *Read src/main.rs 0.4s* or *Thought for 2.1s*; click a row that names a file to open it. Pictures OpenCode reads show as thumbnails under one *Read 6 pictures* row. **Copy** leaves the rows out.
- **While a task runs**, type to add to it. **Return** sends the message now: OpenCode gives it to the model at its next step, and the reply shows *You added …*. **⌘Return** (Ctrl+Enter) queues it instead, to run as the next task once this one finishes; with **⌘Return sends** on, ⇧⌘Return queues. Queued messages sit above the box with **Send Now** and ×. A queue behind a task that failed or was stopped waits until you send it.
- **Attachments** show as square cards above the text, with a preview for pictures (PNG, JPEG, GIF, WebP, BMP, TIFF). Click a card to open the file; the × on hover removes it. Sent prompts show the same cards.

## Models

The picker lists every model OpenCode can use that is priced at $0 and supports tools, grouped by provider with what each reads and its context size. When the app starts, it reads OpenCode’s model catalog. **Settings › Models › Look for Models Again** reads it again, for example after you connect a provider.

- **OpenCode, no account:** GLM 5.3 Flash, DeepSeek V4.1 Flash, MiMo V2.6 Flash, MiniMax M3.1 Flash, Nemotron 3.5 Super VL and Lightning, LongCat 2.5, Inkling 2 Small, Flint 40B, Iceman, Beam, Big Pickle.
- **OpenRouter `:free` models:** North Mini Code, Nemotron 3 Ultra, Gemma 4 31B and 26B, ERNIE 5.5 Flash, Ling 3.2 Flash, Dots 3 Note, Nemotron 3.5 Super VL and Lightning, Nemotron 3 Nano Omni, Particle-1, Flint, Inkling 2 Small. Prices are checked against OpenRouter’s catalog again before each run.
- **Providers you connect:** their models priced at $0 appear once you add a free account’s key. Click **Connect…** next to the provider in the model picker or in **Settings › Models › Free accounts**, follow the link to create a free key, paste it, and choose **Save Key**. The key goes to OpenCode on standard input, never on a command line, and OpenCode stores it with its other credentials; Super OpenCode keeps no copy. **Disconnect** removes only the key the app added. `/connect` in OpenCode’s terminal works too. On 15 November 2026 that covered NVIDIA NIM (GLM 5.3 Flash, DeepSeek V4.1 Flash, MiniMax M3.1 Flash, Gemma 4, GPT-OSS 20B, Nemotron), ModelScope (Qwen3 Coder 30B, Qwen3 235B, GLM-4.6), Z.ai (GLM-4.7-Flash, GLM-4.6V-Flash, GLM-4.5-Flash), Google AI Studio (Gemma 4), and Hugging Face (GLM-4.7-Flash). Each account sets its own limits, and Google may use free-tier prompts to improve its products.
- **On this computer:** models served by Ollama, LM Studio, or vLLM appear while the server is running, and cost nothing.

Left out on purpose: subscription plans that show $0 because you pay for the plan (GitHub Copilot, the Z.ai and Zhipu coding plans), gateways billed by the provider behind them, OpenRouter’s routers (the Auto Router can choose paid models), and LFM3 1.7B, which is too small for agent work. No Kimi model is free anywhere in OpenCode’s catalog today. The runner refuses any model not on the list, so a typed model ID cannot reach a paid model.

**Auto** looks at each task on your computer, without asking a model, and picks one of three models set in **Settings › Models**:

- **Quick**, for short questions and small edits: MiMo V2.6 Flash by default.
- **Everyday**, for the rest: GLM 5.3 Flash.
- **Hard**, for larger changes, investigations, long prompts, several files or a folder, and Plan mode: DeepSeek V4.1 Flash at High effort.

A task with pictures or video goes to one of the three that reads them. The model button shows what Auto would pick for the prompt being typed, and each reply names the model it ran on and why. Every choice comes from the same free list, so Auto can’t reach a paid model. `--run` follows Auto too when no `--model` is given.

When a task hits a rate limit or a free usage limit, tick **Continue on [backup] after a rate limit** to continue in the same session on the backup model from Settings. Without it, the failed task offers **Continue on [backup]** and **Try Again**. A backup on a different provider receives the session.

## Attachments

Click the paperclip, or drop files and folders on the window. Each attachment shows what was done with it before you run the task.

- **Text and code** of any kind (`.md`, `.txt`, `.html`, `.css`, `.js`, source files, data files) go to OpenCode as they are.
- **Documents** (PDF, Word `.docx`, `.doc` on macOS, RTF, OpenDocument, PowerPoint `.pptx`, Excel `.xlsx`, EPUB) are turned into text on your computer first. A PDF made of scanned pages has no text to read.
- **Images** go to models that read them. The app says when the selected model can’t, and offers one that can.
- **Video** goes to the models that read it: GLM 5.3 Flash, MiMo V2.6 Flash, Gemma 4, and the Nemotron vision models.
- **Audio** (WAV, MP3, M4A, AAC, FLAC, Ogg, AIFF, CAF) is transcribed on your computer and sent as text.
- **Folders** inside the project are named in the message so OpenCode reads them itself. A folder outside the project is gathered into one text file of up to 4 MB, leaving out hidden, generated, and binary files.

OpenCode accepts attachments up to 10 MB each.

## Voice

**Dictation.** Click the microphone or press **⌘D** (Ctrl+D on Windows and Linux), speak, and do the same again. The text lands in the box for you to edit. Transcription runs on your computer with the Whisper base speech model, which downloads once (59.7 MB from huggingface.co/ggerganov/whisper.cpp) and is checked against its published SHA-256. The same model transcribes attached audio. Dictation stops itself after five minutes. On macOS, allow the microphone when asked, or later in System Settings › Privacy & Security › Microphone.

**Reading aloud.** **Read aloud** under a response speaks it with the system voice, leaving out code and tool notices. Settings can read every response when it finishes. Linux needs `espeak-ng` or speech-dispatcher.

Links in responses and prompts open in your browser, or, with **Settings › Appearance › Open links › In Super OpenCode**, in a panel beside the conversation with back, forward, reload, an address field, and a button that moves the page to your browser. The panel uses the system’s web engine (WebKit on macOS, WebView2 on Windows) and doesn’t share your browser’s saved logins or extensions. Linux builds open links in the default browser. **Copy** copies a response.

## The side pane

The buttons at the top right open a pane beside the conversation, as in Claude Code. Click a button again, or ×, to close it; the arrows make the pane fill the window.

- **Tasks:** what OpenCode is doing now, and its to-do list for the session, read from OpenCode’s own database without changing it.
- **Terminal:** your shell in the project folder, with colours, full-screen programs, and 5,000 lines to scroll back through. It docks at the bottom under the conversation, or at the side under the open pane; the button beside × moves it. **+** opens another shell in a new tab. Click it to type; Esc and shortcuts go to it while it has the keyboard.
- **Changes:** the files Git sees changed since the last commit, with lines added and removed. Click one for its diff. A folder that isn’t a Git repository says so.
- **Browser:** tabs, with **+** for a new tab, an address field, back, forward, and reload. A new tab offers **Detect Dev Server**, which finds an app being developed on this computer on the usual ports and opens it. The **…** menu opens the page in your browser, copies its address, prints or saves it as a PDF, opens an HTML file, sizes the page as a phone, tablet, or laptop, turns on opening links and HTML files OpenCode writes here, keeps or drops cookies, and clears browsing data. The browser comes with the macOS and Windows builds.
- **Files:** the project’s folders and files. Click a file to preview it, attach it to the prompt, open it, or view an HTML file in the browser.
- **…:** show the project in Finder, open it in Terminal or VS Code, copy its path or the OpenCode session ID, and go to Usage or Settings.

## Usage

**Usage** in the sidebar shows:

- A year of activity drawn like GitHub’s contribution graph, one square per day shaded by tokens, with earlier years to pick from. Hover a day for its tokens and steps.
- Your current and longest streaks of days in a row, active days, the busiest day, and, for OpenCode’s record, what it charged (always $0.00 for free models).
- Daily tokens for the last 7, 30, or 90 days, split by model, with each model’s share.
- The sessions that used the most in those days. Sessions run in the app open with a click.
- Today’s steps and tokens by model in Super OpenCode.

**All OpenCode use** reads OpenCode’s own database on this computer without changing it, so it covers the terminal and other OpenCode apps too. **Super OpenCode only** counts tasks run in the app. Tokens are what the model read fresh and wrote, leaving out cache reads. The start screen shows your streak and the last months of activity; click it to open Usage. `--usage` prints the same totals, streaks, and a 20-week grid in the terminal.

## Settings

Settings has a category list on the left and a search box that narrows it.

- **Models:** Manual or Auto and Auto’s three models, the model new sessions start on, and the backup after a rate limit.
- **Free accounts:** connect and disconnect the providers whose free models need a key.
- **Tasks:** a time limit per task; keeping the computer awake while a task runs (macOS and Linux); letting OpenCode approve its own requests to run commands, edit files, and read outside the project (off by default; when off, requests that need permission are declined and the task carries on); getting your attention when a task finishes in the background; and whether Return or ⌘Return sends.
- **Voice:** the speech model, the dictation language, reading responses aloud, and reading speed.
- **Appearance:** light, dark, or matching the system, the size of text and controls, how much of each task’s working replies show (Normal, Thinking, or Verbose), the conversation’s width, motion, and where links open.
- **OpenCode:** which `opencode` executable runs.
- **Storage:** where data lives, a button to show the folder, and clearing the usage history.
- **Help:** the tutorial, and credits: Super OpenCode is made by [desmondknunoo](https://github.com/desmondknunoo).

OpenCode starts with the app’s PATH plus `~/.opencode/bin`, `~/.local/bin`, `/opt/homebrew/bin`, and `/usr/local/bin` where they exist. An app opened from Finder or a desktop launcher does not see folders that only your shell profile adds, such as a version manager’s. Start the app from a terminal if a task needs those tools.

## Command line

The same executable runs without the window. On macOS it is `Super OpenCode.app/Contents/MacOS/SuperOpenCode`.

```sh
super-opencode --run "Explain the project layout" --project ~/code/app
super-opencode --run "Summarise the spec" --attach spec.pdf --attach notes.md
super-opencode --run "Plan the migration" --plan --effort high
super-opencode --run "Fix the failing test" --fallback-on-429
super-opencode --models
super-opencode --usage
super-opencode --download-speech-model
super-opencode --help
```

`--run` prints output as it arrives, records usage in the same place as the app, and prints the OpenCode session ID at the end so `--session <id>` can continue it. It follows the app’s settings for the backup model, time limit, and approvals. `--private` allows only GLM 5.3 Flash with no fallback and no automatic approvals. Ctrl+C stops OpenCode as well.

## Build for your platform

Install Rust 1.99.1 with rustup and CMake. The speech model’s engine (whisper.cpp) is compiled from source, which also needs Clang. On Windows, install the Microsoft C++ build tools and LLVM for native MSVC builds. On Linux, install the usual C development toolchain, pkg-config, CMake, libclang, and the OpenGL, xkbcommon, and ALSA development libraries. The file pickers use the desktop portal on Linux.

```sh
cargo test --locked
cargo build --release --locked
```

Build each platform on that platform, or with the workflow below. Cross-compiling for Windows from macOS does not work: the speech engine’s build script detects the build machine’s OS instead of the target’s and asks for macOS libraries.

Run the resulting binary under `target/release`. To make the macOS bundle:

```sh
MACOSX_DEPLOYMENT_TARGET=12.0 cargo build --release --locked
scripts/package-macos.sh target/release/super-opencode
```

The [desktop build workflow](../.github/workflows/build.yml) builds app archives on GitHub-hosted runners. Start it manually under **Actions → Desktop builds** after putting this directory at the root of a GitHub repository. It also runs on version tags. It produces macOS Apple silicon and Intel ZIPs, a Windows x64 ZIP, and a Linux x64 tarball. Release files are unsigned for Windows and ad hoc signed on macOS, so Gatekeeper and SmartScreen warn on other computers until they are signed and notarized with your publisher credentials.
