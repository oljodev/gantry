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
    #[must_use]
    pub fn new(provider: ProviderId, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
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

    /// How much a mode lets a call happen without being asked about, least first (04 §3).
    ///
    /// Plan is the narrowest: it hides the tools it would deny. Manual runs anything the user
    /// says yes to, one call at a time. Auto-edit applies writes inside the folders by itself,
    /// and Auto applies everything the guard allows.
    #[must_use]
    pub fn freedom(self) -> u8 {
        match self {
            Mode::Plan => 0,
            Mode::Manual => 1,
            Mode::AutoEdit => 2,
            Mode::Auto => 3,
        }
    }

    /// The narrower of two modes. Used where one thing inherits another's permission and must
    /// never end up with more of it — a sub agent and the chat that started it (18 §6).
    #[must_use]
    pub fn narrower(self, other: Self) -> Self {
        if self.freedom() <= other.freedom() {
            self
        } else {
            other
        }
    }
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
    /// The model for new chats. `None` until the user picks one: Gantry proposes no model of
    /// its own, so the composer asks rather than quietly spending on a model nobody chose.
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
    /// How many tool calls one assistant message may ask for at once (05 §7). A model that
    /// asks for more has lost the thread — the case this exists for is a small model that
    /// emitted forty-six `code-editor__replace` calls in one reply, none of them answered,
    /// before anybody could stop it. The calls past the limit are refused with a result that
    /// says so, which is a thing the model can read and recover from.
    pub max_calls_per_reply: u32,
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
            // Enough for any batch of parallel reads a model has a reason to ask for, and far
            // below the dozens a runaway one produces.
            max_calls_per_reply: 16,
            max_result_kb: 50,
        }
    }
}

/// Every setting, with a default in code. Persisted one section per row (11 §1).
/// The guard of docs/plan/04 §6: which model decides, when the guard decides at all.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct GuardSettings {
    /// The model Gantry calls on the user's behalf: the guard's decisions, the title a chat is
    /// given after its first exchange, and the summary that compacts a long transcript. `None`
    /// means the cheapest fast model of the chat's own provider, from
    /// `assets/models/judge_defaults.toml` — which is what you want almost always, and is why
    /// this is an override rather than a choice the user has to make.
    ///
    /// One setting for all three on purpose. Somebody who does not want a given model called
    /// for them means every call, not the guard's alone, and a title quietly costing a request
    /// on a model they never picked is exactly the surprise this answers.
    pub judge_model: Option<ModelRef>,
}

/// Memory (docs/plan/12 §B3, §B5). Two switches, and each one is a promise: with `paused` on,
/// nothing is injected and nothing is proposed, so a chat about somebody else's data leaves no
/// trace; with an auto-save on — the default — the card still appears, already saved and with
/// **Undo**, because 12 §B1's rule is that no memory exists without the user seeing it, not
/// that they must click.
///
/// Auto-save defaults to on for both scopes. The point of the feature is a workspace that
/// learns how you work without being asked twice, and a confirmation step on every sentence
/// turns that into a chore; what keeps the promise is that the card is still there, the entry
/// is still a visible row, and Undo is one click. Forgetting follows the same rule, and is
/// even safer: a forgotten entry goes to Recently deleted for thirty days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct MemorySettings {
    /// Nothing reaches a prompt and nothing is proposed while this is on.
    pub paused: bool,
    /// Whether the model may propose at all. Off means the tools are not offered.
    pub propose: bool,
    /// Save an assistant proposal — and apply one it offers to forget — without waiting for
    /// the click, per scope. On by default.
    pub auto_save_global: bool,
    pub auto_save_project: bool,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            paused: false,
            propose: true,
            auto_save_global: true,
            auto_save_project: true,
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
    /// What a model may hand to another model (18 §9).
    pub subagents: SubAgentSettings,
    pub advanced: AdvancedSettings,
}

impl Settings {
    /// The keys of the `settings` table, one per section.
    pub const SECTIONS: [&'static str; 7] = [
        "appearance",
        "chat",
        "guardrails",
        "guard",
        "memory",
        "subagents",
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

    /// The model new chats start with, or `None` while the user has picked none.
    #[must_use]
    pub fn default_model(&self) -> Option<ModelRef> {
        self.chat.default_model.clone()
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
    pub subagents: Option<SubAgentSettings>,
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
        if let Some(s) = self.subagents
            && s != settings.subagents
        {
            settings.subagents = s;
            changed.push("subagents");
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

/// What a model may hand to another model (18 §9).
///
/// The two limits are settings rather than constants because the right numbers depend on the
/// model and the money: three at once is generous for a chat and mean for a migration, and
/// nobody here can know which one this user is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct SubAgentSettings {
    /// Who answers when a sub agent's call needs permission (18 §6).
    pub permission: SubAgentPermission,
    /// How many may run at the same time inside one turn.
    pub max_concurrent: u32,
    /// How many one turn may start altogether, however they overlap.
    pub max_per_turn: u32,
    /// The models the parent may choose between, each with the user's own note on when it is
    /// for (18 §5). Empty means every sub agent runs the parent's model.
    pub model_rules: Vec<ModelRule>,
    /// Whether an incognito chat may start sub agents at all. Off: their transcripts are rows,
    /// and rows are the thing incognito promises not to leave (15 A21).
    pub in_incognito: bool,
    /// How long a sub agent's transcript is kept, in days. Zero is forever.
    pub keep_days: u32,
}

impl Default for SubAgentSettings {
    fn default() -> Self {
        Self {
            permission: SubAgentPermission::Ask,
            max_concurrent: 3,
            max_per_turn: 10,
            model_rules: Vec::new(),
            in_incognito: false,
            keep_days: 0,
        }
    }
}

/// Who answers a sub agent's permission card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentPermission {
    /// The card appears in the user's own chat, naming the agent that asked. The default:
    /// taking the user out of decisions about their machine is not something to do by default.
    Ask,
    /// The sub agent runs in Auto with the judge, whatever the parent chat's mode is, and the
    /// user is never stopped. Its decisions are in Guard & guardrails like any other.
    Guard,
}

/// One row of the model menu the parent chooses from (18 §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ModelRule {
    pub model: ModelRef,
    /// The user's own words: "research and long documents", "anything short".
    pub when: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_survive_an_empty_document() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, Settings::default());
        // No model until the user picks one: nothing here proposes a provider or spends on it.
        assert_eq!(s.default_model(), None);
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
            subagents: None,
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
