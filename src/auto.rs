//! Auto model choice: a quick look at each task picks one of three free models chosen in
//! Settings, so short questions don't take the slow, thorough model and hard tasks don't
//! get the quick one. The pick happens on this computer, from the prompt and what is
//! attached, without asking a model. Each reply names the model it ran on and why.

use serde::{Deserialize, Serialize};

use crate::{attach::Kind, models};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Every task runs on the model chosen in the composer.
    #[default]
    Manual,
    /// Each task runs on the model its tier names.
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Quick,
    Everyday,
    Hard,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Self::Quick => "Quick",
            Self::Everyday => "Everyday",
            Self::Hard => "Hard",
        }
    }
}

/// A model and its effort level. `None` runs the model's default effort.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub model: String,
    pub effort: Option<String>,
}

impl Choice {
    fn new(model: &str, effort: Option<&str>) -> Self {
        Self {
            model: model.into(),
            effort: effort.map(str::to_owned),
        }
    }

    /// “GLM 5.3 Flash · High”.
    pub fn label(&self) -> String {
        let name = models::name_of(&self.model);
        match &self.effort {
            Some(effort) => format!("{name} · {}", models::effort_label(effort)),
            None => name,
        }
    }
}

/// The model for each tier. All three need no account and cost nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Models {
    pub quick: Choice,
    pub everyday: Choice,
    pub hard: Choice,
}

impl Default for Models {
    fn default() -> Self {
        Self {
            quick: Choice::new("opencode/mimo-v2.6-flash-free", None),
            everyday: Choice::new(models::DEFAULT, None),
            hard: Choice::new("opencode/deepseek-v4.1-flash-free", Some("high")),
        }
    }
}

impl Models {
    pub fn of(&self, tier: Tier) -> &Choice {
        match tier {
            Tier::Quick => &self.quick,
            Tier::Everyday => &self.everyday,
            Tier::Hard => &self.hard,
        }
    }

    pub fn of_mut(&mut self, tier: Tier) -> &mut Choice {
        match tier {
            Tier::Quick => &mut self.quick,
            Tier::Everyday => &mut self.everyday,
            Tier::Hard => &mut self.hard,
        }
    }
}

/// What Auto chose for a task, and why, in words for the transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub tier: Tier,
    pub choice: Choice,
    pub reason: String,
}

/// Words that mark work needing a careful, thorough model.
const HARD_WORDS: &[&str] = &[
    "refactor",
    "architect",
    "redesign",
    "migrate",
    "migration",
    "implement",
    "build a",
    "build the",
    "create a",
    "debug",
    "investigate",
    "root cause",
    "why does",
    "why is",
    "performance",
    "optimize",
    "optimise",
    "security",
    "vulnerab",
    "rewrite",
    "port ",
    "across the",
    "every file",
    "all files",
    "whole project",
    "end-to-end",
    "end to end",
    "race condition",
    "deadlock",
    "memory leak",
    "concurren",
    "test suite",
    "failing test",
];

/// Openings of short questions and small edits.
const QUICK_OPENINGS: &[&str] = &[
    "what ",
    "what's",
    "where ",
    "which ",
    "who ",
    "when ",
    "how many",
    "how do i",
    "is there",
    "does ",
    "do we",
    "can you explain",
    "explain ",
    "summarize",
    "summarise",
    "list ",
    "show ",
    "find ",
    "rename ",
    "fix the typo",
    "fix typo",
    "typo",
    "format ",
    "add a comment",
    "update the readme",
    "bump ",
    "print ",
    "count ",
];

/// Sorts a task into a tier, with the reason in a few words.
pub fn tier(prompt: &str, attachments: usize, folder: bool, plan: bool) -> (Tier, &'static str) {
    let text = prompt.trim().to_lowercase();
    let words = text.split_whitespace().count();
    if plan {
        return (Tier::Hard, "planning work");
    }
    if HARD_WORDS.iter().any(|word| text.contains(word)) {
        return (Tier::Hard, "a larger change or investigation");
    }
    if words > 120 || text.lines().count() > 12 {
        return (Tier::Hard, "a long, detailed task");
    }
    if folder || attachments >= 3 {
        return (Tier::Hard, "several files to read");
    }
    if attachments == 0
        && words <= 24
        && (QUICK_OPENINGS
            .iter()
            .any(|opening| text.starts_with(opening))
            || (words <= 10 && text.ends_with('?')))
    {
        return (Tier::Quick, "a short question or small edit");
    }
    (Tier::Everyday, "an everyday task")
}

/// The model for `tier`, unless what is attached needs one that reads pictures or video.
/// Then the first tier model that reads them is used, from the most capable down.
pub fn pick(prompt: &str, kinds: &[Kind], plan: bool, choices: &Models) -> Pick {
    let folder = kinds.contains(&Kind::Folder);
    let (tier, reason) = tier(prompt, kinds.len(), folder, plan);
    let mut choice = choices.of(tier).clone();
    // A tier whose model is no longer offered falls back to the default model.
    if models::find(&choice.model).is_none() {
        choice = Choice::new(models::DEFAULT, None);
    }
    let needs_video = kinds.contains(&Kind::Video);
    let needs_images = kinds.contains(&Kind::Image);
    let reads = |choice: &Choice| {
        models::find(&choice.model).is_some_and(|model| {
            (!needs_images || !model.images.is_empty()) && (!needs_video || !model.video.is_empty())
        })
    };
    let mut reason = reason.to_owned();
    if (needs_images || needs_video) && !reads(&choice) {
        let seeing = [Tier::Hard, Tier::Everyday, Tier::Quick]
            .into_iter()
            .map(|tier| choices.of(tier).clone())
            .chain([Choice::new(models::DEFAULT, None)])
            .find(|candidate| reads(candidate));
        if let Some(seeing) = seeing {
            choice = seeing;
            reason.push_str(if needs_video {
                ", with video attached"
            } else {
                ", with pictures attached"
            });
        }
    }
    // An effort level the model doesn't offer would be refused.
    if let (Some(effort), Some(model)) = (&choice.effort, models::find(&choice.model))
        && !model.variants.is_empty()
        && !model.variants.contains(effort)
    {
        choice.effort = None;
    }
    Pick {
        tier,
        choice,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_questions_are_quick() {
        assert_eq!(
            tier("What does parse_amount return?", 0, false, false).0,
            Tier::Quick
        );
        assert_eq!(tier("rename Foo to Bar", 0, false, false).0, Tier::Quick);
        assert_eq!(tier("tests pass?", 0, false, false).0, Tier::Quick);
    }

    #[test]
    fn larger_work_is_hard() {
        assert_eq!(
            tier(
                "Refactor the billing module to use integer cents",
                0,
                false,
                false
            )
            .0,
            Tier::Hard
        );
        assert_eq!(
            tier("Why does cargo test hang?", 0, false, false).0,
            Tier::Hard
        );
        assert_eq!(tier("Add a button", 0, false, true).0, Tier::Hard);
        assert_eq!(tier("Look at these", 0, true, false).0, Tier::Hard);
        assert_eq!(tier(&"word ".repeat(150), 0, false, false).0, Tier::Hard);
    }

    #[test]
    fn everything_else_is_everyday() {
        assert_eq!(
            tier(
                "Add a currency column to invoices and backfill it",
                0,
                false,
                false
            )
            .0,
            Tier::Everyday
        );
        assert_eq!(
            tier("What is in this file?", 1, false, false).0,
            Tier::Everyday
        );
    }

    #[test]
    fn video_moves_to_a_model_that_watches_it() {
        let choices = Models::default();
        // DeepSeek reads pictures but not video, so a hard task with a video goes elsewhere.
        let picked = pick(
            "Debug the crash in this recording",
            &[Kind::Video],
            false,
            &choices,
        );
        assert_eq!(picked.tier, Tier::Hard);
        assert_ne!(picked.choice.model, choices.hard.model);
        assert!(picked.reason.ends_with("with video attached"));
        let plain = pick("Debug the crash", &[], false, &choices);
        assert_eq!(plain.choice, choices.hard);
    }

    #[test]
    fn a_missing_model_falls_back_to_the_default() {
        let choices = Models {
            quick: Choice::new("openrouter/gone/model:free", None),
            ..Models::default()
        };
        let picked = pick("what is this?", &[], false, &choices);
        assert_eq!(picked.choice.model, models::DEFAULT);
    }
}
