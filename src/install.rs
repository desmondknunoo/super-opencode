//! Installing OpenCode when it isn't on this computer, in the background, with the installer
//! OpenCode documents: its install script on macOS and Linux, and npm or Scoop on Windows.

use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

/// Runs OpenCode's installer and returns where OpenCode now is.
pub fn install_opencode() -> Result<PathBuf, String> {
    let path = crate::runner::child_path(std::env::var_os("PATH"), dirs::home_dir());
    let mut command = if cfg!(windows) {
        let found = |name: &str| {
            path.as_ref().is_some_and(|path| {
                std::env::split_paths(path).any(|folder| folder.join(name).is_file())
            })
        };
        if found("npm.cmd") {
            let mut command = Command::new("cmd");
            command.args(["/C", "npm", "install", "-g", "opencode-ai@latest"]);
            command
        } else if found("scoop.cmd") || found("scoop.ps1") {
            let mut command = Command::new("cmd");
            command.args(["/C", "scoop", "install", "opencode"]);
            command
        } else {
            return Err(
                "Install Node.js or Scoop, then try again, or install OpenCode from opencode.ai."
                    .into(),
            );
        }
    } else {
        let mut command = Command::new("bash");
        command.args(["-c", "curl -fsSL https://opencode.ai/install | bash"]);
        command
    };
    if let Some(path) = &path {
        command.env("PATH", path);
    }
    if let Some(home) = dirs::home_dir() {
        command.current_dir(home);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("Couldn’t start the installer: {error}"))?;
    if let Some(found) = crate::runner::find_opencode() {
        return Ok(found);
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let last = said
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no message");
    Err(if output.status.success() {
        "The installer finished, but OpenCode wasn’t found. Choose it in Settings › OpenCode."
            .into()
    } else {
        format!("The installer stopped: {last}")
    })
}
