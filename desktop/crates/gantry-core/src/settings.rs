//! The typed settings document (docs/plan/11 §1) and the small value types the rest of the
//! app shares with it: permission modes, model references, reasoning effort.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::guardrail::GuardrailSettings;

/// A provider account id: `anthropic`, `openai`, `google`, `xai`, `openrouter` or `custom:<ulid>`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(transparent)]
#[specta(transparent)]
pub struct ProviderId(pub String);

impl ProviderId {
    pub const OPENROUTER: &'static str = "openrouter";

    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[must_use]
    pub fn openrouter() -> Self {
        Self(Self::OPENROUTER.to_owned())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A model on a provider.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct ModelRef {
    pub provider: ProviderId,
    pub model: String,
}

impl ModelRef {
    /// The model Gantry proposes until the user picks another.
    #[must_use]
    pub fn default_model() -> Self {
        Self {
            provider: ProviderId::openrouter(),
            model: "deepseek/deepseek-v4-flash".to_owned(),
        }
    }
}

/// The permission mode of a chat (docs/plan/04 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Manual,
    AutoEdit,
    Plan,
    Auto,
}

impl Mode {
    /// The name used in prompts, settings and the database.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Manual => "manual",
            Mode::AutoEdit => "auto_edit",
            Mode::Plan => "plan",
            Mode::Auto => "auto",
        }
    }

    pub const ALL: [Mode; 4] = [Mode::Manual, Mode::AutoEdit, Mode::Plan, Mode::Auto];
}

/// How much the model should think before answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    High,
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Comfortable,
    Compact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct AppearanceSettings {
    pub theme: Theme,
    pub density: Density,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            density: Density::Comfortable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct ChatSettings {
    /// Mode for chats created outside a project.
    pub default_mode: Mode,
    /// Whether the judge guards Auto mode by default.
    pub default_guard: bool,
    /// The same pair for the Code surface, which starts somewhere else (docs/plan/16 §9).
    pub code_default_mode: Mode,
    pub code_default_guard: bool,
    /// The model for new chats; `None` means [`ModelRef::default_model`].
    pub default_model: Option<ModelRef>,
    pub default_effort: ReasoningEffort,
    /// Settings → General → Custom instructions (docs/plan/10 §2, layer 4). At most 4000 chars.
    pub custom_instructions: String,
    pub suggest_connectors: bool,
    /// Connectors a new chat starts with attached, by namespace (03 §11). Ships with
    /// `filesystem`: a chat that cannot see your files is the commonest dead end, and its
    /// writes are still `write` tier and still decided by the mode. The shell and the code
    /// editor are deliberately not here — those belong to the Code surface (16).
    pub default_connectors: Vec<String>,
    /// Open the right pane the first time a turn creates an artifact (13 §4).
    pub open_artifact_panel: bool,
    /// Models starred in the model dialog, newest first. Recents are not stored beside them:
    /// they are what the chat list already says, and a second record of the same fact drifts.
    pub favourite_models: Vec<ModelRef>,
    /// What was chosen for a model that makes something other than text, by `provider/model`.
    /// Per model rather than per chat: a voice is a property of the voice you picked, and
    /// choosing it again in every new chat is the kind of work software should not ask for.
    pub model_options: BTreeMap<String, MediaOptions>,
}

/// What a media model lets a person choose (docs/plan/02 §5). Every field is optional and
/// nothing is sent unless it was picked: a model's own default is better than Gantry's guess,
/// and an aspect ratio the model does not support is an error rather than a near miss.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct MediaOptions {
    /// A named voice, for a model that reads text aloud.
    pub voice: Option<String>,
    pub aspect_ratio: Option<String>,
    /// `720p`, `4K` — a video model's own spelling, whatever that is.
    pub resolution: Option<String>,
    /// Seconds of finished video.
    pub duration_seconds: Option<u32>,
    /// An image model's quality tier, where it has them.
    pub quality: Option<String>,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            default_mode: Mode::AutoEdit,
            default_guard: true,
            code_default_mode: Mode::AutoEdit,
            code_default_guard: true,
            default_model: None,
            default_effort: ReasoningEffort::Medium,
            custom_instructions: String::new(),
            suggest_connectors: true,
            default_connectors: vec!["filesystem".to_owned()],
            open_artifact_panel: true,
            favourite_models: Vec::new(),
            model_options: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct AdvancedSettings {
    /// Upper bound on one assistant message, in tokens.
    pub max_output_tokens: u32,
    /// Show the assembled system prompt and log raw provider requests (never the key).
    pub developer_mode: bool,
    /// How many tool rounds one reply may take before Gantry stops it (01 §3 step 6).
    pub max_tool_rounds: u32,
    /// What one tool result contributes to the transcript, in kilobytes (05 §8, 02 §6). The
    /// head and the tail are kept with a marker between them; the whole output stays in the
    /// activity row, which reads from the call record rather than from the transcript. Capping
    /// at ingestion is not an edit to history — the message is written down capped.
    pub max_result_kb: u32,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            max_output_tokens: 8192,
            developer_mode: false,
            max_tool_rounds: 50,
            max_result_kb: 50,
        }
    }
}

/// Every setting, with a default in code. Persisted one section per row (11 §1).
/// The guard of docs/plan/04 §6: which model decides, when the guard decides at all.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct GuardSettings {
    /// The model the guard asks. `None` means the cheapest fast model of the chat's own
    /// provider, from `assets/models/judge_defaults.toml` — which is what you want almost
    /// always, and is why this is an override rather than a choice the user has to make.
    pub judge_model: Option<ModelRef>,
}

/// Memory (docs/plan/12 §B3, §B5). Two switches, and each one is a promise: with `paused` on,
/// nothing is injected and nothing is proposed, so a chat about somebody else's data leaves no
/// trace; with an auto-save on, the card still appears — already saved, with **Undo** — because
/// 12 §B1's rule is that no memory exists without the user seeing it, not that they must click.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct MemorySettings {
    /// Nothing reaches a prompt and nothing is proposed while this is on.
    pub paused: bool,
    /// Whether the model may propose at all. Off means the tools are not offered.
    pub propose: bool,
    /// Save an assistant proposal without waiting for the click, per scope.
    pub auto_save_global: bool,
    pub auto_save_project: bool,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            paused: false,
            propose: true,
            auto_save_global: false,
            auto_save_project: false,
        }
    }
}

impl MemorySettings {
    /// Whether a proposal in this scope is saved before the user answers (12 §B3).
    #[must_use]
    pub fn auto_saves(&self, scope: crate::memory::MemoryScopeKind) -> bool {
        match scope {
            crate::memory::MemoryScopeKind::Global => self.auto_save_global,
            crate::memory::MemoryScopeKind::Project => self.auto_save_project,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct Settings {
    pub appearance: AppearanceSettings,
    pub chat: ChatSettings,
    /// The floor of docs/plan/04 §5, as the user's deviation from the shipped list.
    pub guardrails: GuardrailSettings,
    pub guard: GuardSettings,
    pub memory: MemorySettings,
    pub advanced: AdvancedSettings,
}

impl Settings {
    /// The keys of the `settings` table, one per section.
    pub const SECTIONS: [&'static str; 6] = [
        "appearance",
        "chat",
        "guardrails",
        "guard",
        "memory",
        "advanced",
    ];

    /// Where a new session on this surface starts (16 §9).
    #[must_use]
    pub fn defaults_for(&self, surface: crate::Surface) -> (Mode, bool) {
        match surface {
            crate::Surface::Chat => (self.chat.default_mode, self.chat.default_guard),
            crate::Surface::Code => (self.chat.code_default_mode, self.chat.code_default_guard),
        }
    }

    /// The model new chats start with.
    #[must_use]
    pub fn default_model(&self) -> ModelRef {
        self.chat
            .default_model
            .clone()
            .unwrap_or_else(ModelRef::default_model)
    }
}

/// A section-level patch: every present section replaces the stored one.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct SettingsPatch {
    pub appearance: Option<AppearanceSettings>,
    pub chat: Option<ChatSettings>,
    pub guardrails: Option<GuardrailSettings>,
    pub guard: Option<GuardSettings>,
    pub memory: Option<MemorySettings>,
    pub advanced: Option<AdvancedSettings>,
}

impl SettingsPatch {
    /// Applies the patch, returning the names of the sections that changed.
    pub fn apply(self, settings: &mut Settings) -> Vec<&'static str> {
        let mut changed = Vec::new();
        if let Some(s) = self.appearance
            && s != settings.appearance
        {
            settings.appearance = s;
            changed.push("appearance");
        }
        if let Some(s) = self.chat
            && s != settings.chat
        {
            settings.chat = s;
            changed.push("chat");
        }
        if let Some(s) = self.guardrails
            && s != settings.guardrails
        {
            settings.guardrails = s;
            changed.push("guardrails");
        }
        if let Some(s) = self.guard
            && s != settings.guard
        {
            settings.guard = s;
            changed.push("guard");
        }
        if let Some(s) = self.memory
            && s != settings.memory
        {
            settings.memory = s;
            changed.push("memory");
        }
        if let Some(s) = self.advanced
            && s != settings.advanced
        {
            settings.advanced = s;
            changed.push("advanced");
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_survive_an_empty_document() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, Settings::default());
        assert_eq!(s.default_model().model, "deepseek/deepseek-v4-flash");
    }

    #[test]
    fn a_patch_reports_only_what_changed() {
        let mut s = Settings::default();
        let patch = SettingsPatch {
            appearance: Some(AppearanceSettings {
                theme: Theme::Dark,
                ..Default::default()
            }),
            chat: Some(ChatSettings::default()),
            guardrails: None,
            memory: None,
            guard: Some(GuardSettings {
                judge_model: Some(ModelRef {
                    provider: ProviderId("anthropic".into()),
                    model: "claude-haiku-4-5".to_owned(),
                }),
            }),
            advanced: None,
        };
        assert_eq!(patch.apply(&mut s), vec!["appearance", "guard"]);
        assert_eq!(s.appearance.theme, Theme::Dark);
        assert_eq!(s.guard.judge_model.unwrap().model, "claude-haiku-4-5");
    }
}
