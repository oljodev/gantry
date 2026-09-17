//! Agent types: what a sub agent is made of (docs/plan/18 §3).
//!
//! A type is a record, not a document. A skill is a document because a person writes prose in
//! it and may keep it in Git; a type is a name, a paragraph and eight switches, and a document
//! whose body is a form is a form.
//!
//! The field that carries the design is [`AgentType::open`]. Everything a type does not open is
//! fixed by the type, and the parent model naming it in a call is refused rather than ignored —
//! an argument that is quietly dropped is how a model learns to keep sending one.

use serde::{Deserialize, Serialize};

use crate::{Mode, ModelRef};

/// The sentinel inside [`AgentType::connectors`] meaning "whatever the parent chat has".
pub const INHERIT: &str = "inherit";

/// One entry of the library (18 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AgentType {
    /// The slug the model names in a call, and the id of the row.
    pub id: String,
    pub name: String,
    /// One line, read by the **parent** model when it chooses between types.
    pub description: String,
    /// The system prompt fragment the sub agent runs under.
    pub instructions: String,
    pub model: AgentModel,
    /// The namespaces it may use. [`INHERIT`] stands for the parent chat's own list.
    pub connectors: Vec<String>,
    /// `None` means the parent chat's mode, whatever that is.
    pub mode: Option<Mode>,
    pub guard: Option<bool>,
    /// Whether it may change files in the session's folders, or only read them.
    pub write_files: bool,
    /// Whether the user's memories reach its prompt, and whether it may load skills (12).
    pub memory: bool,
    pub skills: bool,
    /// The fields the parent may set in the call. Everything else is the type's own.
    pub open: Vec<OpenField>,
    /// Shipped with Gantry. Editable; **Reset** puts the original back.
    pub builtin: bool,
    pub enabled: bool,
}

/// Which model a sub agent runs (18 §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentModel {
    /// The parent's own model. The default, and the only one that needs no configuration.
    Inherit,
    /// Let the parent choose from the rules the user wrote, which are shown to it as a menu.
    Rules,
    /// One model, named here and not up for discussion.
    Named { model: ModelRef },
}

impl AgentModel {
    /// How the row stores it: `inherit`, `rules`, or `provider/model`.
    #[must_use]
    pub fn as_stored(&self) -> String {
        match self {
            Self::Inherit => "inherit".to_owned(),
            Self::Rules => "rules".to_owned(),
            Self::Named { model } => format!("{}/{}", model.provider.as_str(), model.model),
        }
    }

    /// The inverse, tolerant of anything it does not recognise: a row written by a newer build
    /// falls back to the parent's model rather than refusing to load the library.
    #[must_use]
    pub fn from_stored(s: &str) -> Self {
        match s {
            "inherit" => Self::Inherit,
            "rules" => Self::Rules,
            other => match other.split_once('/') {
                Some((provider, model)) if !provider.is_empty() && !model.is_empty() => {
                    Self::Named {
                        model: ModelRef {
                            provider: crate::ProviderId::new(provider.to_owned()),
                            model: model.to_owned(),
                        },
                    }
                }
                _ => Self::Inherit,
            },
        }
    }
}

/// A field a type hands to the parent model instead of deciding itself (18 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OpenField {
    Instructions,
    Connectors,
    Write,
    Model,
    Mode,
    Memory,
    Skills,
}

impl OpenField {
    /// The argument name on the tool, which is also how the row stores it.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Instructions => "instructions",
            Self::Connectors => "connectors",
            Self::Write => "write",
            Self::Model => "model",
            Self::Mode => "mode",
            Self::Memory => "memory",
            Self::Skills => "skills",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [
            Self::Instructions,
            Self::Connectors,
            Self::Write,
            Self::Model,
            Self::Mode,
            Self::Memory,
            Self::Skills,
        ]
        .into_iter()
        .find(|f| f.key() == key)
    }
}

impl AgentType {
    #[must_use]
    pub fn opens(&self, field: OpenField) -> bool {
        self.open.contains(&field)
    }
}
