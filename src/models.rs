//! The free models Super OpenCode offers, with what each one reads.
//!
//! A built-in list of models known to be free with tool support is always available. When the
//! app starts, it also asks OpenCode for its model catalog and adds every model that is priced
//! at $0 with tool support from the providers you have connected, plus any model served on this
//! computer by Ollama, LM Studio, or vLLM. OpenRouter models are also checked against
//! OpenRouter's own prices before each run.

use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    sync::{LazyLock, RwLock, mpsc},
    thread,
    time::Duration,
};

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    /// The ID OpenCode uses: `provider/model`.
    pub id: String,
    pub name: String,
    /// OpenCode's provider ID, such as `opencode`, `openrouter`, `nvidia`, or `ollama`.
    pub provider: String,
    /// Image formats it reads, by file extension.
    pub images: &'static [&'static str],
    /// Video formats it reads, by file extension.
    pub video: &'static [&'static str],
    /// Context window in tokens. Zero when unknown.
    pub context: u32,
    /// Effort levels OpenCode offers for it, such as `low`, `high`, and `max`, chosen with
    /// `model#level`. Empty until OpenCode's catalog has been read.
    pub variants: Vec<String>,
}

/// An effort level as people say it.
pub fn effort_label(variant: &str) -> String {
    match variant {
        "none" => "Off".into(),
        "xhigh" => "Extra high".into(),
        other => {
            let mut letters = other.chars();
            letters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(letters).collect()
            })
        }
    }
}

const OPENCODE_IMAGES: &[&str] = &["png", "jpg", "jpeg"];
const WEB_IMAGES: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const VIDEO: &[&str] = &["mp4", "mov", "webm"];

pub const DEFAULT: &str = "opencode/glm-5.3-flash-free";
/// The model a rate-limited task continues on, unless Settings names another.
pub const BACKUP: &str = "openrouter/cohere/north-mini-code:free";

/// Providers whose models run on this computer. They cost nothing whatever their catalog says.
const LOCAL: &[&str] = &["ollama", "lmstudio", "vllm"];

/// Providers that show $0 prices but are paid another way: through a subscription plan or by
/// the provider behind a gateway. Their models are never offered.
fn billed_elsewhere(provider: &str) -> bool {
    provider.contains("coding-plan")
        || provider.ends_with("-plan")
        || provider == "github-copilot"
        || provider.contains("gateway")
}

/// Free models left out on purpose.
const SKIPPED: &[&str] = &[
    // 1.7B parameters is too small for agent work that reads, edits, and runs code.
    "openrouter/liquid/lfm-3-1.7b:free",
    "kilo/liquid/lfm-3-1.7b:free",
    // Kilo's Auto Router shows $0 but can choose a paid model behind it, like OpenRouter's.
    "kilo/openrouter/auto",
];

struct Seed(
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
    u32,
);

/// Models known to be free with tool support, used before and without OpenCode's catalog.
const BUILT_IN: &[Seed] = &[
    Seed(
        DEFAULT,
        "GLM 5.3 Flash",
        OPENCODE_IMAGES,
        &["mp4", "mkv", "mov"],
        200_000,
    ),
    Seed(
        "opencode/deepseek-v4.1-flash-free",
        "DeepSeek V4.1 Flash",
        WEB_IMAGES,
        &[],
        200_000,
    ),
    Seed(
        "opencode/mimo-v2.6-flash-free",
        "MiMo V2.6 Flash",
        &["png", "jpg", "jpeg", "gif", "webp", "bmp"],
        &["mp4", "mov", "avi"],
        200_000,
    ),
    Seed(
        "opencode/nemotron-3.5-super-vl-free",
        "Nemotron 3.5 Super VL",
        OPENCODE_IMAGES,
        &[],
        1_048_576,
    ),
    Seed(
        "opencode/nemotron-3.5-lightning-free",
        "Nemotron 3.5 Lightning",
        &[],
        &[],
        262_144,
    ),
    Seed(
        "opencode/longcat-2.5-free",
        "LongCat 2.5",
        WEB_IMAGES,
        &[],
        1_000_000,
    ),
    Seed(
        "opencode/inkling-2-small-free",
        "Inkling 2 Small",
        WEB_IMAGES,
        &[],
        1_048_576,
    ),
    Seed("opencode/flint-40b-free", "Flint 40B", &[], &[], 262_144),
    Seed("opencode/iceman-free", "Iceman", &[], &[], 262_144),
    Seed("opencode/beam-free", "Beam", &[], &[], 1_048_576),
    Seed("opencode/big-pickle", "Big Pickle", &[], &[], 200_000),
    Seed(BACKUP, "North Mini Code", &[], &[], 256_000),
    Seed(
        "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free",
        "Nemotron 3 Ultra",
        &[],
        &[],
        1_000_000,
    ),
    Seed(
        "openrouter/google/gemma-4-31b-it:free",
        "Gemma 4 31B",
        WEB_IMAGES,
        VIDEO,
        262_144,
    ),
    Seed(
        "openrouter/google/gemma-4-26b-a4b-it:free",
        "Gemma 4 26B A4B",
        WEB_IMAGES,
        VIDEO,
        262_144,
    ),
    Seed(
        "openrouter/baidu/ernie-5.5-flash-preview:free",
        "ERNIE 5.5 Flash",
        &[],
        &[],
        1_024_000,
    ),
    Seed(
        "openrouter/inclusionai/ling-3.2-flash:free",
        "Ling 3.2 Flash",
        WEB_IMAGES,
        &[],
        1_048_576,
    ),
    Seed(
        "openrouter/dots-studio/dots-3-note-preview:free",
        "Dots 3 Note",
        WEB_IMAGES,
        &[],
        512_000,
    ),
    Seed(
        "openrouter/nvidia/nemotron-3.5-super-vl:free",
        "Nemotron 3.5 Super VL",
        WEB_IMAGES,
        VIDEO,
        1_000_000,
    ),
    Seed(
        "openrouter/nvidia/nemotron-3.5-lightning:free",
        "Nemotron 3.5 Lightning",
        &[],
        &[],
        1_000_000,
    ),
    Seed(
        "openrouter/nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free",
        "Nemotron 3 Nano Omni",
        WEB_IMAGES,
        VIDEO,
        256_000,
    ),
    Seed(
        "openrouter/particle-ai/particle-1:free",
        "Particle-1",
        WEB_IMAGES,
        &[],
        1_048_576,
    ),
    Seed(
        "openrouter/recursive/flint-v1.0:free",
        "Flint",
        &[],
        &[],
        262_144,
    ),
    Seed(
        "openrouter/thinkingmachines/inkling-2-small:free",
        "Inkling 2 Small",
        WEB_IMAGES,
        &[],
        1_048_576,
    ),
];

fn built_in() -> Vec<Model> {
    BUILT_IN
        .iter()
        .map(|Seed(id, name, images, video, context)| Model {
            id: (*id).to_owned(),
            name: (*name).to_owned(),
            provider: id.split('/').next().unwrap_or_default().to_owned(),
            images,
            video,
            context: *context,
            variants: Vec::new(),
        })
        .collect()
}

/// Built-in models first, in their order, then discovered ones grouped by provider.
static CATALOG: LazyLock<RwLock<Vec<Model>>> = LazyLock::new(|| RwLock::new(built_in()));

pub fn all() -> Vec<Model> {
    CATALOG
        .read()
        .map(|list| list.clone())
        .unwrap_or_else(|_| built_in())
}

/// The model at `index`, or the default when the list has changed under it.
pub fn at(index: usize) -> Model {
    let list = all();
    list.get(index).cloned().unwrap_or_else(|| list[0].clone())
}

pub fn find(id: &str) -> Option<Model> {
    all().into_iter().find(|model| model.id == id)
}

pub fn index_of(id: &str) -> Option<usize> {
    all().iter().position(|model| model.id == id)
}

/// The display name, or the ID itself for a model this app doesn't list.
pub fn name_of(id: &str) -> String {
    find(id).map_or_else(|| id.to_owned(), |model| model.name)
}

/// Whether `id` may run without charges: a listed model, or any OpenRouter model tagged
/// `:free`, which is also checked against OpenRouter's prices before it runs.
pub fn is_free(id: &str) -> bool {
    find(id).is_some() || (id.starts_with("openrouter/") && id.ends_with(":free"))
}

pub fn is_local(provider: &str) -> bool {
    LOCAL.contains(&provider)
}

pub fn provider_name(provider: &str) -> String {
    match provider {
        "opencode" => "OpenCode",
        "openrouter" => "OpenRouter",
        "nvidia" => "NVIDIA NIM",
        "modelscope" => "ModelScope",
        "zai" => "Z.ai",
        "zhipuai" => "Zhipu AI",
        "google" => "Google AI Studio",
        "huggingface" => "Hugging Face",
        "kilo" => "Kilo Code",
        "agnes" => "Agnes AI",
        "cohere" => "Cohere",
        "groq" => "Groq",
        "mistral" => "Mistral",
        "cloudflare-workers-ai" => "Cloudflare Workers AI",
        "ovhcloud" => "OVHcloud",
        "ollama-cloud" => "Ollama Cloud",
        "chutes" => "Chutes",
        "xai" => "xAI",
        "siliconflow" => "SiliconFlow",
        "ollama" => "Ollama",
        "lmstudio" => "LM Studio",
        "vllm" => "vLLM",
        other => other,
    }
    .to_owned()
}

/// What using a provider takes, and what makes it free.
pub fn provider_access(provider: &str) -> &'static str {
    match provider {
        "opencode" => "No account needed",
        "openrouter" => "Free models · needs an OpenRouter connection",
        "google" => {
            "Free tier on your account · Google may use free-tier prompts to improve its products"
        }
        local if is_local(local) => "Runs on this computer",
        _ => "Priced $0 in OpenCode’s catalog · your account’s limits apply",
    }
}

/// Where a provider's group sits in the model picker.
fn rank(provider: &str) -> usize {
    match provider {
        "opencode" => 0,
        "openrouter" => 1,
        local if is_local(local) => 3,
        _ => 2,
    }
}

/// The providers in `models`, in picker order.
pub fn providers(models: &[Model]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for model in models {
        if !found.contains(&model.provider) {
            found.push(model.provider.clone());
        }
    }
    found.sort_by_key(|provider| (rank(provider), provider_name(provider)));
    found
}

impl Model {
    /// "Text and images", for the model picker.
    pub fn reads(&self) -> &'static str {
        match (self.images.is_empty(), self.video.is_empty()) {
            (true, _) => "Text",
            (false, true) => "Text and images",
            (false, false) => "Text, images, and video",
        }
    }

    /// "200K", "1M", or nothing when unknown.
    pub fn context_label(&self) -> String {
        match self.context {
            0 => String::new(),
            1_000_000.. => "1M".into(),
            tokens => format!("{}K", tokens / 1000),
        }
    }

    pub fn provider_name(&self) -> String {
        provider_name(&self.provider)
    }
}

/// Adds discovered models after the built-in ones, without repeating any.
pub fn set_discovered(discovered: Vec<Model>) {
    let list = merge(discovered);
    if let Ok(mut catalog) = CATALOG.write() {
        *catalog = list;
    }
}

fn merge(discovered: Vec<Model>) -> Vec<Model> {
    let mut list = built_in();
    // The catalog knows each built-in model's effort levels.
    for model in &mut list {
        if let Some(found) = discovered.iter().find(|found| found.id == model.id) {
            model.variants = found.variants.clone();
        }
    }
    let mut extra: Vec<Model> = discovered
        .into_iter()
        .filter(|model| !list.iter().any(|known| known.id == model.id))
        .collect();
    extra.sort_by(|a, b| {
        (rank(&a.provider), provider_name(&a.provider), &a.name).cmp(&(
            rank(&b.provider),
            provider_name(&b.provider),
            &b.name,
        ))
    });
    list.extend(extra);
    list
}

/// A catalog name without the "(free)" or "Free" tag and the "Publisher: " prefix.
fn tidy_name(name: &str) -> String {
    let name = name.rsplit_once(": ").map_or(name, |(_, rest)| rest);
    let name = name
        .trim_end_matches("(free)")
        .trim_end()
        .trim_end_matches(" Free")
        .trim();
    name.to_owned()
}

/// One model from OpenCode's `/api/model`, when it can be offered.
fn offered(entry: &Value) -> Option<Model> {
    let provider = entry["providerID"].as_str()?;
    let model = entry["id"].as_str()?;
    let id = format!("{provider}/{model}");
    if billed_elsewhere(provider) || SKIPPED.contains(&id.as_str()) {
        return None;
    }
    // OpenRouter's routers show $0 because their price varies; the Auto Router can pick paid
    // models. Only models tagged `:free` are free, and only those pass the check before a run.
    if provider == "openrouter" && !model.ends_with(":free") {
        return None;
    }
    let chat = entry["type"].as_str().is_none_or(|kind| kind == "chat");
    let active = entry["status"]
        .as_str()
        .is_none_or(|status| status == "active")
        && entry["enabled"].as_bool() != Some(false);
    let tools = entry["capabilities"]["tools"].as_bool() == Some(true);
    let priced_zero = entry["cost"].as_array().is_some_and(|costs| {
        !costs.is_empty()
            && costs.iter().all(|cost| {
                cost["input"].as_f64().unwrap_or(0.0) == 0.0
                    && cost["output"].as_f64().unwrap_or(0.0) == 0.0
            })
    });
    if !(chat && active && tools && (priced_zero || is_local(provider))) {
        return None;
    }
    let inputs: Vec<&str> = entry["capabilities"]["input"]
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let has = |kind: &str| inputs.iter().any(|input| input.starts_with(kind));
    let only_png_jpeg = has("image")
        && inputs
            .iter()
            .filter(|input| input.starts_with("image/"))
            .all(|input| matches!(*input, "image/png" | "image/jpeg"))
        && inputs.iter().any(|input| input.starts_with("image/"));
    Some(Model {
        name: tidy_name(entry["name"].as_str().unwrap_or(model)),
        id,
        provider: provider.to_owned(),
        images: if !has("image") {
            &[]
        } else if only_png_jpeg {
            OPENCODE_IMAGES
        } else {
            WEB_IMAGES
        },
        video: if has("video") { VIDEO } else { &[] },
        context: entry["limit"]["context"]
            .as_u64()
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32,
        variants: entry["variants"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|variant| variant["id"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// The free models in OpenCode's catalog JSON.
fn parse(catalog: &str) -> Result<Vec<Model>, String> {
    let value: Value = serde_json::from_str(catalog)
        .map_err(|_| "OpenCode’s model list came back in an unexpected format.".to_owned())?;
    let entries = value["data"]
        .as_array()
        .or_else(|| value.as_array())
        .ok_or("OpenCode’s model list came back in an unexpected format.")?;
    Ok(entries.iter().filter_map(offered).collect())
}

/// Asks OpenCode for every model it can use right now and returns the free ones.
pub fn discover(binary: &Path) -> Result<Vec<Model>, String> {
    let mut command = Command::new(binary);
    command
        .args(["--standalone", "api", "GET", "/api/model"])
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(path) = crate::runner::child_path(std::env::var_os("PATH"), dirs::home_dir()) {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("Couldn’t ask OpenCode for its models: {error}"))?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut text = String::new();
        let _ = sender.send(stdout.read_to_string(&mut text).map(|_| text));
    });
    let text = match receiver.recv_timeout(Duration::from_secs(90)) {
        Ok(Ok(text)) => text,
        Ok(Err(error)) => return Err(format!("Couldn’t read OpenCode’s model list: {error}")),
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("OpenCode took too long to list its models.".into());
        }
    };
    let _ = child.wait();
    parse(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_built_in_list_is_consistent() {
        let list = built_in();
        let mut ids: Vec<&str> = list.iter().map(|model| model.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), list.len(), "duplicate model IDs");
        assert_eq!(list[0].id, DEFAULT);
        assert!(list.iter().any(|model| model.id == BACKUP));
        for model in &list {
            assert!(
                model.id.starts_with("opencode/")
                    || (model.id.starts_with("openrouter/") && model.id.ends_with(":free"))
            );
        }
        assert_eq!(list[0].context_label(), "200K");
        assert!(!is_free("openrouter/anthropic/claude-opus-latest"));
        assert!(!is_free("opencode/claude-opus"));
        assert!(is_free("openrouter/someone/new-model:free"));
    }

    fn entry(provider: &str, id: &str, price: f64, tools: bool, inputs: &[&str]) -> Value {
        json!({
            "providerID": provider, "id": id, "name": format!("Maker: {id} (free)"), "type": "chat",
            "status": "active", "enabled": true,
            "capabilities": {"tools": tools, "input": inputs},
            "cost": [{"input": price, "output": price}],
            "limit": {"context": 131072},
            "variants": [{"id": "low"}, {"id": "xhigh"}]
        })
    }

    #[test]
    fn only_free_tool_models_from_paying_free_providers_are_offered() {
        let catalog = json!({"data": [
            entry("nvidia", "z-ai/glm-5.3-flash", 0.0, true, &["text", "image"]),
            entry("modelscope", "Qwen/Qwen3-Coder-30B-A3B-Instruct", 0.0, true, &["text"]),
            entry("moonshotai", "kimi-k3", 0.6, true, &["text"]),
            entry("zai", "glm-4.7-flash", 0.0, false, &["text"]),
            entry("zai-coding-plan", "glm-5.3", 0.0, true, &["text"]),
            entry("github-copilot", "kimi-k3", 0.0, true, &["text"]),
            entry("ollama", "qwen3:8b", 1.0, true, &["text"]),
            entry("openrouter", "liquid/lfm-3-1.7b:free", 0.0, true, &["text"]),
            entry("openrouter", "openrouter/auto", 0.0, true, &["text"]),
            entry("openrouter", "openrouter/free", 0.0, true, &["text"]),
            entry("kilo", "openrouter/auto", 0.0, true, &["text"]),
            entry("kilo", "liquid/lfm-3-1.7b:free", 0.0, true, &["text"]),
            entry("kilo", "reflection/beam-free", 0.0, true, &["text"]),
            entry("agnes", "agnes-3.0-flash", 0.0, true, &["text", "image/png"]),
            entry("cohere", "north-mini-code-1-0", 0.0, true, &["text"]),
            entry("opencode", "glm-5.3-flash-free", 0.0, true, &["text", "image/png", "image/jpeg", "video/mp4"]),
        ]})
        .to_string();
        let offered = parse(&catalog).unwrap();
        let ids: Vec<&str> = offered.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "nvidia/z-ai/glm-5.3-flash",
                "modelscope/Qwen/Qwen3-Coder-30B-A3B-Instruct",
                "ollama/qwen3:8b",
                "kilo/reflection/beam-free",
                "agnes/agnes-3.0-flash",
                "cohere/north-mini-code-1-0",
                "opencode/glm-5.3-flash-free",
            ]
        );
        assert_eq!(offered[0].name, "z-ai/glm-5.3-flash");
        assert_eq!(offered[0].images, WEB_IMAGES);
        let built_in = offered.last().unwrap();
        assert_eq!(built_in.images, OPENCODE_IMAGES);
        assert_eq!(built_in.video, VIDEO);
        assert_eq!(offered[0].variants, ["low", "xhigh"]);
        assert_eq!(effort_label("xhigh"), "Extra high");
        assert_eq!(effort_label("high"), "High");
        assert_eq!(effort_label("none"), "Off");
        assert!(parse("not json").is_err());
    }

    #[test]
    fn discovered_models_follow_the_built_in_ones_in_provider_order() {
        let found = parse(
            &json!({"data": [
                entry("ollama", "qwen3:8b", 0.0, true, &["text"]),
                entry("nvidia", "openai/gpt-oss-20b", 0.0, true, &["text"]),
                entry("opencode", "glm-5.3-flash-free", 0.0, true, &["text"]),
            ]})
            .to_string(),
        )
        .unwrap();
        let list = merge(found);
        assert_eq!(list.len(), built_in().len() + 2);
        // A built-in model takes its effort levels from the catalog.
        assert_eq!(list[0].variants, ["low", "xhigh"]);
        let tail: Vec<&str> = list[list.len() - 2..]
            .iter()
            .map(|m| m.provider.as_str())
            .collect();
        assert_eq!(tail, ["nvidia", "ollama"]);
        assert_eq!(
            providers(&list),
            ["opencode", "openrouter", "nvidia", "ollama"]
        );
    }
}
