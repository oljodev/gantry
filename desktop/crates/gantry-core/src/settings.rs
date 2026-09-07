//! The typed settings document (docs/plan/11 §1) and the small value types the rest of the
//! app shares with it: permission modes, model references, reasoning effort.

use serde::{Deserialize, Serialize};

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
    /// The model for new chats; `None` means [`ModelRef::default_model`].
    pub default_model: Option<ModelRef>,
    pub default_effort: ReasoningEffort,
    /// Settings → General → Custom instructions (docs/plan/10 §2, layer 4). At most 4000 chars.
    pub custom_instructions: String,
    pub suggest_connectors: bool,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            default_mode: Mode::AutoEdit,
            default_guard: true,
            default_model: None,
            default_effort: ReasoningEffort::Medium,
            custom_instructions: String::new(),
            suggest_connectors: true,
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
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            max_output_tokens: 8192,
            developer_mode: false,
        }
    }
}

/// Every setting, with a default in code. Persisted one section per row (11 §1).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct Settings {
    pub appearance: AppearanceSettings,
    pub chat: ChatSettings,
    pub advanced: AdvancedSettings,
}

impl Settings {
    /// The keys of the `settings` table, one per section.
    pub const SECTIONS: [&'static str; 3] = ["appearance", "chat", "advanced"];

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
            advanced: None,
        };
        assert_eq!(patch.apply(&mut s), vec!["appearance"]);
        assert_eq!(s.appearance.theme, Theme::Dark);
    }
}
