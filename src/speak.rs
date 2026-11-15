//! Reading responses aloud with the speech built into the operating system.

use std::{
    io::Write,
    process::{Child, Command, Stdio},
};

use crate::storage::Pace;

/// A command that reads text from its standard input aloud at `pace`.
fn voice(pace: Pace) -> Option<Command> {
    let pick = |slower: &'static str, normal: &'static str, faster: &'static str| match pace {
        Pace::Slower => slower,
        Pace::Normal => normal,
        Pace::Faster => faster,
    };
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("/usr/bin/say");
        command.args(["-r", pick("150", "185", "235")]);
        Some(command)
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let script = format!(
            "[Console]::InputEncoding = [Text.Encoding]::UTF8; Add-Type -AssemblyName System.Speech; $voice = New-Object System.Speech.Synthesis.SpeechSynthesizer; $voice.Rate = {}; $voice.Speak([Console]::In.ReadToEnd())",
            pick("-2", "0", "3")
        );
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(script)
            .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        Some(command)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let found = |name: &str| {
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|folder| folder.join(name))
                    .find(|candidate| candidate.is_file())
            })
        };
        if let Some(path) = found("espeak-ng").or_else(|| found("espeak")) {
            let mut command = Command::new(path);
            command.args(["--stdin", "-s", pick("140", "175", "225")]);
            return Some(command);
        }
        found("spd-say").map(|path| {
            let mut command = Command::new(path);
            command.args(["--wait", "--pipe-mode", "-r", pick("-30", "0", "40")]);
            command
        })
    }
}

pub fn available() -> bool {
    voice(Pace::Normal).is_some()
}

#[derive(Default)]
pub struct Speaker {
    child: Option<Child>,
}

impl Speaker {
    pub fn speak(&mut self, text: &str, pace: Pace) -> Result<(), String> {
        self.stop();
        let mut command = voice(pace).ok_or(if cfg!(target_os = "linux") {
            "Reading aloud needs espeak-ng or speech-dispatcher. Install one with your package manager."
        } else {
            "This computer has no speech voice available."
        })?;
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Couldn’t start reading aloud: {error}"))?;
        if let Some(mut input) = child.stdin.take() {
            let _ = input.write_all(text.as_bytes());
        }
        self.child = Some(child);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn is_speaking(&mut self) -> bool {
        match &mut self.child {
            Some(child) => match child.try_wait() {
                Ok(None) => true,
                _ => {
                    self.child = None;
                    false
                }
            },
            None => false,
        }
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        self.stop();
    }
}
