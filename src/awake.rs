//! Keeping the computer awake while a task runs, so a long task isn't paused by sleep.
//! macOS uses `caffeinate`, Linux `systemd-inhibit`. The display may still turn off.

use std::process::{Child, Command, Stdio};

pub const SUPPORTED: bool = cfg!(any(target_os = "macos", target_os = "linux"));

/// Holds the computer awake until dropped.
pub struct Awake(Child);

impl Awake {
    pub fn start() -> Option<Self> {
        let mut command = if cfg!(target_os = "macos") {
            // -i prevents idle sleep; -w ends it with this app, should the app quit first.
            let mut command = Command::new("caffeinate");
            command.args(["-i", "-w", &std::process::id().to_string()]);
            command
        } else if cfg!(target_os = "linux") {
            let mut command = Command::new("systemd-inhibit");
            command.args([
                "--what=idle:sleep",
                "--who=Super OpenCode",
                "--why=A coding task is running",
                "sleep",
                "infinity",
            ]);
            command
        } else {
            return None;
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
            .map(Self)
    }
}

impl Drop for Awake {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
