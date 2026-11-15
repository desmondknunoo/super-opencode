//! Connecting the providers whose free models need an account.
//!
//! Keys are handed to OpenCode, which stores them with its other credentials. A key goes to
//! `opencode auth import` on standard input, never on a command line, where other programs on
//! the computer could read it. Super OpenCode keeps no copy.

use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use serde_json::{Value, json};

pub struct Connectable {
    /// OpenCode's integration and provider ID.
    pub id: &'static str,
    pub name: &'static str,
    /// The free models it adds, from OpenCode's catalog on 15 November 2026.
    pub models: &'static str,
    /// Where to create a free key.
    pub key_page: &'static str,
    /// What to know before using it.
    pub note: &'static str,
}

pub const CONNECTABLE: &[Connectable] = &[
    Connectable {
        id: "openrouter",
        name: "OpenRouter",
        models: "North Mini Code, Nemotron 3 Ultra, Gemma 4, ERNIE 5.5 Flash, and the other :free models",
        key_page: "https://openrouter.ai/settings/keys",
        note: "Create a key in your OpenRouter account. Only models tagged :free are offered, and their prices are checked before each run. Free models allow a limited number of requests a day.",
    },
    Connectable {
        id: "nvidia",
        name: "NVIDIA NIM",
        models: "GLM 5.3 Flash, DeepSeek V4.1 Flash, MiniMax M3.1 Flash, Gemma 4, GPT-OSS 20B, and Nemotron",
        key_page: "https://build.nvidia.com/settings/api-keys",
        note: "Sign in at build.nvidia.com and generate an API key.",
    },
    Connectable {
        id: "modelscope",
        name: "ModelScope",
        models: "Qwen3 Coder 30B, Qwen3 235B, and GLM-4.6",
        key_page: "https://modelscope.cn/my/myaccesstoken",
        note: "Uses modelscope.cn. Create an access token there; ModelScope may ask you to link an Alibaba Cloud account first.",
    },
    Connectable {
        id: "zai",
        name: "Z.ai",
        models: "GLM-4.7-Flash, GLM-4.6V-Flash, and GLM-4.5-Flash",
        key_page: "https://z.ai/manage-apikey/apikey-list",
        note: "Create an API key in the Z.ai console. Only the Flash models are free.",
    },
    Connectable {
        id: "google",
        name: "Google AI Studio",
        models: "Gemma 4",
        key_page: "https://aistudio.google.com/app/apikey",
        note: "Create a key in AI Studio. On the free tier, Google may use your prompts to improve its products.",
    },
    Connectable {
        id: "huggingface",
        name: "Hugging Face",
        models: "GLM-4.7-Flash",
        key_page: "https://huggingface.co/settings/tokens",
        note: "Create an access token with permission to make calls to Inference Providers. Free use is limited by monthly credits.",
    },
];

pub fn find(id: &str) -> Option<&'static Connectable> {
    CONNECTABLE.iter().find(|provider| provider.id == id)
}

/// The credential ID Super OpenCode gives keys it stores, so it removes only its own.
fn credential_id(provider: &str) -> String {
    format!("cred_superopencode0{provider}")
}

/// One provider's saved credentials in OpenCode, without their secrets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Connection {
    pub provider: String,
    pub credentials: Vec<String>,
}

impl Connection {
    /// Whether one of its credentials was added by this app.
    pub fn added_here(&self) -> bool {
        self.credentials.contains(&credential_id(&self.provider))
    }
}

fn opencode(binary: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .arg("--standalone")
        .current_dir(std::env::temp_dir())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = crate::runner::child_path(std::env::var_os("PATH"), dirs::home_dir()) {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

/// The last line OpenCode printed, without its drawing characters, for error messages.
fn last_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|line| {
            line.trim_matches(|c: char| c.is_whitespace() || "│┌└■◇◆●".contains(c))
                .replace("\u{1b}[?25h", "")
        })
        .rfind(|line| !line.is_empty())
        .unwrap_or_default()
}

fn parse_connections(text: &str) -> Result<Vec<Connection>, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|_| "OpenCode’s account list came back in an unexpected format.".to_owned())?;
    Ok(value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(Connection {
                        provider: item["id"].as_str()?.to_owned(),
                        credentials: item["connections"]
                            .as_array()
                            .map(|list| {
                                list.iter()
                                    .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// The providers OpenCode holds credentials for.
pub fn connections(binary: &Path) -> Result<Vec<Connection>, String> {
    let output = opencode(binary)
        .args(["auth", "list", "--format", "json"])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Couldn’t ask OpenCode for its accounts: {error}"))?;
    parse_connections(&String::from_utf8_lossy(&output.stdout))
}

/// What OpenCode imports: one key credential for `provider`.
fn import_payload(provider: &str, key: &str) -> String {
    json!([{
        "id": credential_id(provider),
        "integrationID": provider,
        "label": "API key (Super OpenCode)",
        "active": true,
        "value": {"type": "key", "key": key},
    }])
    .to_string()
}

/// Removes the key this app stored for `provider`, if there is one.
pub fn disconnect(binary: &Path, provider: &str) -> Result<(), String> {
    let output = opencode(binary)
        .args(["auth", "logout", provider, &credential_id(provider)])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Couldn’t remove the key: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Couldn’t remove the key: {}",
            last_line(&output.stderr)
        ))
    }
}

/// Stores `key` for `provider` in OpenCode, replacing a key this app stored before.
pub fn connect(binary: &Path, provider: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return Err("Paste the key exactly as the provider shows it, without spaces.".into());
    }
    if connections(binary)?
        .iter()
        .any(|connection| connection.provider == provider && connection.added_here())
    {
        disconnect(binary, provider)?;
    }
    let mut child = opencode(binary)
        .args(["auth", "import"])
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Couldn’t start OpenCode: {error}"))?;
    if let Some(mut input) = child.stdin.take() {
        input
            .write_all(import_payload(provider, key).as_bytes())
            .map_err(|error| format!("Couldn’t pass the key to OpenCode: {error}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("OpenCode didn’t finish saving the key: {error}"))?;
    // OpenCode reports the import on either stream, depending on the version.
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() && said.contains("Imported 1") {
        Ok(())
    } else {
        let detail = last_line(&output.stderr);
        let detail = if detail.is_empty() {
            last_line(&output.stdout)
        } else {
            detail
        };
        Err(format!("OpenCode didn’t save the key: {detail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_import_carries_the_key_and_our_credential_id() {
        let payload: Value =
            serde_json::from_str(&import_payload("nvidia", "nv-\"quoted\"")).unwrap();
        assert_eq!(payload[0]["id"], "cred_superopencode0nvidia");
        assert_eq!(payload[0]["integrationID"], "nvidia");
        assert_eq!(payload[0]["value"]["key"], "nv-\"quoted\"");
        assert_eq!(payload[0]["active"], true);
    }

    #[test]
    fn connections_are_read_without_secrets() {
        let listed = r#"[{"id":"openrouter","name":"OpenRouter","connections":[{"type":"credential","id":"cred_abc","label":"API key","method":"key"}]},
            {"id":"nvidia","name":"Nvidia","connections":[{"type":"credential","id":"cred_superopencode0nvidia","label":"API key","method":"key"}]}]"#;
        let found = parse_connections(listed).unwrap();
        assert_eq!(found.len(), 2);
        assert!(!found[0].added_here());
        assert!(found[1].added_here());
        assert!(parse_connections("Failed").is_err());
    }

    #[test]
    fn every_connectable_provider_has_a_key_page() {
        for provider in CONNECTABLE {
            assert!(provider.key_page.starts_with("https://"), "{}", provider.id);
        }
        assert!(find("modelscope").is_some());
    }

    #[test]
    fn last_line_drops_the_prompt_drawing() {
        assert_eq!(
            last_line("┌  Connect\n│\n■  Unknown form field: key\n│\n└  Failed\n".as_bytes()),
            "Failed"
        );
    }

    /// Connects and disconnects a placeholder key in a throwaway OpenCode profile:
    /// `SUPER_OPENCODE_SANDBOX=<folder> cargo test sandbox_round_trip -- --ignored`
    /// The folder must hold `data`, `config`, `state`, and `cache` folders for OpenCode.
    #[test]
    #[ignore]
    fn sandbox_round_trip() {
        let sandbox = std::path::PathBuf::from(std::env::var("SUPER_OPENCODE_SANDBOX").unwrap());
        // SAFETY: this ignored test runs on its own, before any other thread reads the environment.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", sandbox.join("data"));
            std::env::set_var("XDG_CONFIG_HOME", sandbox.join("config"));
            std::env::set_var("XDG_STATE_HOME", sandbox.join("state"));
            std::env::set_var("XDG_CACHE_HOME", sandbox.join("cache"));
        }
        let binary = crate::runner::find_opencode().unwrap();
        connect(&binary, "nvidia", "placeholder-one").unwrap();
        // Connecting again replaces the key instead of being skipped.
        connect(&binary, "nvidia", "placeholder-two").unwrap();
        let found = connections(&binary).unwrap();
        assert!(
            found
                .iter()
                .any(|c| c.provider == "nvidia" && c.added_here()),
            "{found:?}"
        );
        let models = crate::models::discover(&binary).unwrap();
        assert!(
            models.iter().any(|m| m.provider == "nvidia"),
            "no NVIDIA models offered"
        );
        disconnect(&binary, "nvidia").unwrap();
        assert!(
            connections(&binary)
                .unwrap()
                .iter()
                .all(|c| c.provider != "nvidia")
        );
    }
}
