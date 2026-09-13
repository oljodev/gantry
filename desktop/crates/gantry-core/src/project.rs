//! Projects (docs/plan/09 M11, 06 §3): a place to put related work, and the context it shares.
//!
//! A project is four things a chat would otherwise have to be told again every time: standing
//! instructions, knowledge files, a folder, and the defaults a new chat opens with. Nothing here
//! behaves; it is all context and configuration, which is why none of it needs a permission.
//!
//! The defaults are `Option`s and that is the point. `None` means "whatever the settings say",
//! which is not the same as a value that happens to equal today's setting: a project that does
//! not care about the mode must not freeze this week's default into every chat it ever opens.

use serde::{Deserialize, Serialize};

use crate::{
    ids::{ProjectFileId, ProjectId},
    settings::Mode,
};

/// Prompt layer 5 is capped here (10 §2), twice the global layer's cap because a project is
/// where the specifics live.
pub const PROJECT_INSTRUCTIONS_MAX_CHARS: usize = 8000;

/// How much project knowledge one prompt carries, in characters.
///
/// Knowledge is frozen into every chat of the project, so this is a per-turn cost for the life of
/// every one of those chats: roughly 15,000 tokens at the usual four characters a token. Past
/// this the files are cut, each one told how much of it was taken, because a prompt that quietly
/// holds the first third of a specification is worse than one that says it holds a third.
pub const PROJECT_KNOWLEDGE_MAX_CHARS: usize = 60_000;

/// One row of the sidebar's Projects list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub name: String,
    pub description: String,
    pub pinned: bool,
    pub archived: bool,
    /// The folder every chat here starts with, when there is one.
    pub workspace_path: Option<String>,
    pub chat_count: u32,
    pub file_count: u32,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub updated_at: i64,
}

/// A project with everything its page shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ProjectDetail {
    pub id: ProjectId,
    pub name: String,
    pub description: String,
    /// Prompt layer 5 (10 §2).
    pub instructions: String,
    pub workspace_path: Option<String>,
    pub defaults: ProjectDefaults,
    pub pinned: bool,
    pub archived: bool,
    pub files: Vec<ProjectFileDto>,
    /// Skill ids pinned to the project (12 §A6): in the frozen prompt of every chat here.
    pub skills: Vec<String>,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub updated_at: i64,
}

/// What a new chat in this project starts with. Every field unset means "ask the settings".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ProjectDefaults {
    pub mode: Option<Mode>,
    pub guard: Option<bool>,
    /// Connector namespaces to attach, in place of the settings' list.
    pub connectors: Option<Vec<String>>,
    /// Standing permissions every chat here is opened with (04 §8), written as
    /// `GrantSource::ProjectDefault` so the Permissions page can say where they came from.
    pub grants: Option<Vec<ProjectGrant>>,
}

/// One standing permission a project hands to its chats. A narrower shape than `ChatGrant` on
/// purpose: a project default is a rule the user wrote in a settings panel, not a decision made
/// about one call, so it has no id, no timestamp and nothing to revoke.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ProjectGrant {
    /// The connector namespace, or `gantry` for the runtime tools.
    pub instance_name: String,
    /// One tool, or every tool of that namespace when absent.
    pub tool_name: Option<String>,
    /// The highest tier this grant answers for, `None` meaning the tool's own tier.
    pub tier_ceiling: Option<crate::tool::RiskTier>,
}

/// A knowledge file: the bytes as the user added them, and the text that goes into prompts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ProjectFileDto {
    pub id: ProjectFileId,
    pub project_id: ProjectId,
    pub name: String,
    pub mime: String,
    #[specta(type = specta_typescript::Number)]
    pub size: i64,
    pub blob_hash: String,
    /// How many characters of text came out of it, which is what it costs a prompt. The text
    /// itself is not sent to the frontend: a page listing ten files does not need ten documents.
    pub text_chars: u32,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
}

/// What a new project is created with: a name, and nothing else that cannot be changed after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct NewProject {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub workspace_path: Option<String>,
}

/// Every field optional, and absent means "leave it alone". The defaults are the one place this
/// is ambiguous — `Some(ProjectDefaults { mode: None, .. })` clears the mode default — which is
/// why the whole block is replaced at once rather than merged field by field.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ProjectPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub instructions: Option<String>,
    /// `Some(None)` removes the folder, `None` leaves it.
    pub workspace_path: Option<Option<String>>,
    pub defaults: Option<ProjectDefaults>,
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
}
