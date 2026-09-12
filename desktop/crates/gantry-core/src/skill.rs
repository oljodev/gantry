//! Skills (docs/plan/12 §A): a playbook is Markdown with frontmatter and nothing else. Gantry
//! never runs anything it finds in a skill folder, so the whole type is text and the rules
//! below are about size and shape rather than about capability.
//!
//! The format is the open Agent Skills file (agentskills.io) so a skill written here opens in
//! Claude Code and one written there opens here; Gantry's own fields live under `metadata` with
//! a `gantry-` prefix, which the specification reserves for exactly this.

use serde::{Deserialize, Serialize};

/// Where a skill came from (12 §A3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// Shipped inside the binary from `desktop/skills/`. Editable only by shadowing it.
    Bundled,
    /// Written here, in `<app_data>/skills/`.
    User,
    /// Brought in from a file, a folder or a URL, and living in the same place as `User`.
    Imported,
}

impl SkillSource {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SkillSource::Bundled => "bundled",
            SkillSource::User => "user",
            SkillSource::Imported => "imported",
        }
    }

    /// A bundled skill has no file of its own: it is read from the binary and cannot be
    /// edited, renamed or deleted, only switched off or shadowed by a skill with another name.
    #[must_use]
    pub fn is_editable(self) -> bool {
        !matches!(self, SkillSource::Bundled)
    }
}

/// Why a version was written (12 §A3). Every one of them keeps the full text, so Replace is
/// always reversible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SkillVersionSource {
    Bundled,
    UserEdit,
    Import,
    AiProposal,
    /// The file changed under us — the user edited it in their own editor (12 §A3).
    ExternalChange,
}

/// One row of the `skills` index. The body is not here: it is read from the file, or from the
/// binary for a bundled skill, when something actually needs it (12 §A4's progressive
/// disclosure is the same idea one layer down).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SkillDto {
    /// The skill's name, which is its id and its folder name.
    pub id: String,
    pub source: SkillSource,
    /// Absolute path of the folder; absent for a bundled skill.
    pub path: Option<String>,
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,
    /// `metadata.gantry-always`: in every chat's frozen prompt rather than matched per message.
    pub always_include: bool,
    pub enabled: bool,
    pub content_hash: String,
    /// Bytes of `SKILL.md`.
    pub size: u32,
    pub version: u32,
    pub author: Option<String>,
    pub license: Option<String>,
    /// `references/*.md` beside the skill, by file name.
    pub references: Vec<String>,
    #[specta(type = specta_typescript::Number)]
    pub installed_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub updated_at: i64,
    #[specta(type = Option<specta_typescript::Number>)]
    pub last_used_at: Option<i64>,
    pub use_count: u32,
    /// Chats and projects this skill is pinned to, counted for the list's badge.
    pub pinned_count: u32,
}

/// A skill with its text, for the editor and for `gantry__load_skill`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SkillDetail {
    #[serde(flatten)]
    pub skill: SkillDto,
    /// The Markdown below the frontmatter.
    pub body: String,
}

/// What the editor and an import both hand back: the fields, and the body they belong to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
pub struct SkillInput {
    pub name: String,
    pub description: String,
    pub triggers: Vec<String>,
    pub always_include: bool,
    pub author: Option<String>,
    pub license: Option<String>,
    pub body: String,
    pub references: Vec<SkillReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SkillReference {
    /// The file name inside `references/`, without a path.
    pub file: String,
    pub text: String,
}

/// What a proposal card shows (12 §A5 flow 4): the skill the model wrote, and the installed
/// one it would replace when the name is taken.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SkillProposal {
    pub input: SkillInput,
    /// The model's sentence about why this is worth keeping.
    pub reason: String,
    /// Set when `input.name` is already installed: its current version and body, so the card
    /// can say "Replaces v3" and show the difference. A bundled skill can never be replaced.
    pub replaces: Option<SkillReplaces>,
    /// A name that is free, offered as **Save as …** when `replaces` is set.
    pub suggested_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SkillReplaces {
    pub name: String,
    pub version: u32,
    pub source: SkillSource,
    pub body: String,
}

/// How a proposal card ended. The user may have edited every field before saving, so the
/// resolution carries what was saved rather than only that something was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillProposalOutcome {
    Saved { id: String, name: String },
    Discarded,
}

// --- Limits and validation (12 §A2) -----------------------------------------------------

pub const NAME_MAX: usize = 64;
pub const DESCRIPTION_MAX: usize = 1024;
/// The hard cap on `SKILL.md`; the recommendation of roughly 5,000 tokens is advice the editor
/// shows as a count, not a rule.
pub const BODY_MAX_BYTES: usize = 32 * 1024;
pub const REFERENCE_MAX_BYTES: usize = 64 * 1024;
pub const REFERENCES_MAX: usize = 10;
/// What one skill may contribute to a frozen prompt when it is pinned (10 §2).
pub const PINNED_MAX_BYTES: usize = 20 * 1024;

/// Everything wrong with a skill, in the order a form would show it. Empty means valid.
#[must_use]
pub fn validate(input: &SkillInput) -> Vec<String> {
    let mut problems = Vec::new();
    if let Some(p) = name_problem(&input.name) {
        problems.push(p);
    }
    let description = input.description.trim();
    if description.is_empty() {
        problems.push(
            "A description is required: say what the skill does and when to use it. It is what \
             matches a message to this skill."
                .to_owned(),
        );
    } else if description.chars().count() > DESCRIPTION_MAX {
        problems.push(format!(
            "The description is {} characters; the limit is {DESCRIPTION_MAX}.",
            description.chars().count()
        ));
    }
    if input.body.trim().is_empty() {
        problems.push("The skill has no body. A skill is its instructions.".to_owned());
    } else if input.body.len() > BODY_MAX_BYTES {
        problems.push(format!(
            "The body is {} KB; the limit is {} KB.",
            input.body.len() / 1024,
            BODY_MAX_BYTES / 1024
        ));
    }
    if input.references.len() > REFERENCES_MAX {
        problems.push(format!(
            "{} reference files; at most {REFERENCES_MAX} are kept.",
            input.references.len()
        ));
    }
    for r in &input.references {
        if r.file.contains('/') || r.file.contains('\\') || r.file.starts_with('.') {
            problems.push(format!(
                "`{}` is not a plain file name; references live directly in `references/`.",
                r.file
            ));
        }
        if r.text.len() > REFERENCE_MAX_BYTES {
            problems.push(format!(
                "`{}` is {} KB; a reference file may be {} KB.",
                r.file,
                r.text.len() / 1024,
                REFERENCE_MAX_BYTES / 1024
            ));
        }
    }
    problems
}

/// The name rule of 12 §A2, as the sentence a form shows. `None` means the name is fine.
#[must_use]
pub fn name_problem(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("A name is required; it is also the folder name.".to_owned());
    }
    if name.len() > NAME_MAX {
        return Some(format!(
            "`{name}` is {} characters; the limit is {NAME_MAX}.",
            name.len()
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Some(format!(
            "`{name}` may hold only lowercase letters, digits and hyphens."
        ));
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return Some(format!(
            "`{name}` may not start or end with a hyphen, or hold two in a row."
        ));
    }
    None
}

/// Turns free text into a name the rule above accepts: `Rust Idioms!` becomes `rust-idioms`.
/// Used by the editor's live slug preview and to repair a name a model invented.
#[must_use]
pub fn slugify(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let slug = out.trim_matches('-').to_owned();
    slug.chars().take(NAME_MAX).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_a_folder_name() {
        assert!(name_problem("rust-idioms").is_none());
        assert!(name_problem("Rust").is_some());
        assert!(name_problem("rust--idioms").is_some());
        assert!(name_problem("-rust").is_some());
        assert!(name_problem("").is_some());
        assert!(name_problem(&"a".repeat(65)).is_some());
    }

    #[test]
    fn a_title_becomes_a_slug() {
        assert_eq!(slugify("Rust Idioms!"), "rust-idioms");
        assert_eq!(slugify("  Writing a plan  "), "writing-a-plan");
        assert_eq!(slugify("C++"), "c");
        assert!(name_problem(&slugify("Review PRs — carefully")).is_none());
    }

    #[test]
    fn validation_names_every_problem_at_once() {
        let problems = validate(&SkillInput {
            name: "Bad Name".into(),
            description: String::new(),
            body: String::new(),
            ..SkillInput::default()
        });
        assert_eq!(problems.len(), 3, "{problems:?}");
    }
}
