//! Memory (docs/plan/12 §B): short standing sentences that reach the model in later chats.
//!
//! Two properties run through every type here. Every entry is **visible**: nothing reaches a
//! prompt that is not a row on the Memory page. And every entry has **provenance**: which chat
//! and which message it came from, and whether a person or the model asked for it — because an
//! assistant-written memory may have been decided while the model was reading someone else's
//! text, and the user is the one who says whether it stands.

use serde::{Deserialize, Serialize};

use crate::ids::{ChatId, MemoryId, MessageId, ProjectId};

/// What kind of sentence this is (12 §B2). The kind decides where it is spent: instructions
/// and preferences are standing behaviour and live in the frozen prompt, facts and notes are
/// looked up per message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// How to behave: "answer in Norwegian".
    Instruction,
    /// Tool and style preferences: "prefer pnpm".
    Preference,
    /// About the user, their projects, their machine: "the API lives in services/api".
    Fact,
    /// A working note for a project.
    Note,
}

impl MemoryKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryKind::Instruction => "instruction",
            MemoryKind::Preference => "preference",
            MemoryKind::Fact => "fact",
            MemoryKind::Note => "note",
        }
    }

    /// Whether this kind belongs to the core set that is frozen into a chat's prompt (12 §B4).
    #[must_use]
    pub fn is_standing(self) -> bool {
        matches!(self, MemoryKind::Instruction | MemoryKind::Preference)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScopeKind {
    Global,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    User,
    Assistant,
}

/// One `memories` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MemoryDto {
    pub id: MemoryId,
    pub scope_kind: MemoryScopeKind,
    /// The project, when the scope is a project.
    pub scope_id: Option<ProjectId>,
    pub kind: MemoryKind,
    pub text: String,
    /// In every chat's frozen prompt whatever its kind, because the user said so.
    pub always_include: bool,
    pub source: MemorySource,
    pub origin_chat_id: Option<ChatId>,
    pub origin_message_id: Option<MessageId>,
    pub tags: Vec<String>,
    pub enabled: bool,
    pub use_count: u32,
    #[specta(type = Option<specta_typescript::Number>)]
    pub last_used_at: Option<i64>,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub updated_at: i64,
    /// Deleted, and restorable from Recently deleted for `RECENTLY_DELETED_DAYS` (12 §B5).
    #[specta(type = Option<specta_typescript::Number>)]
    pub archived_at: Option<i64>,
}

/// What the Memory page's editor and `/remember` hand in; `None` leaves a field alone on an
/// update and takes the default on a create.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
pub struct MemoryInput {
    pub text: Option<String>,
    pub kind: Option<MemoryKind>,
    pub scope_kind: Option<MemoryScopeKind>,
    pub scope_id: Option<ProjectId>,
    pub always_include: Option<bool>,
    pub enabled: Option<bool>,
    pub tags: Option<Vec<String>>,
}

/// Remembering something, or forgetting it. Both are cards, and only the user resolves one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAction {
    Remember,
    Forget,
}

/// What a memory card shows (12 §B3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MemoryProposal {
    pub action: MemoryAction,
    /// The proposed sentence, or the existing one when the action is to forget it.
    pub text: String,
    pub kind: MemoryKind,
    pub scope_kind: MemoryScopeKind,
    pub scope_id: Option<ProjectId>,
    /// The model's sentence about why, which the user reads before deciding.
    pub reason: String,
    /// The entry this replaces, or the one it would forget.
    pub target: Option<MemoryDto>,
    /// Already saved, because Settings → Memory has auto-save on for this scope; the card
    /// offers Undo instead of Save (12 §B3).
    pub auto_saved: bool,
}

/// How a memory card ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryProposalOutcome {
    Saved { id: MemoryId },
    Forgotten { id: MemoryId },
    Discarded,
}

/// One entry as the prompt sees it, for the `context.injected` event and the "Context used"
/// row (05 §2, 15 A15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct InjectedMemory {
    pub id: MemoryId,
    pub kind: MemoryKind,
    pub text: String,
}

/// One skill as the prompt sees it, the other half of the same row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct InjectedSkill {
    pub name: String,
    pub source: crate::skill::SkillSource,
    /// Why it was picked: `matched`, `pinned`, `always` or `invoked`.
    pub how: String,
}

/// Everything one turn added to the model's context beyond the transcript (10 §5).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
pub struct InjectedContext {
    pub skills: Vec<InjectedSkill>,
    pub memories: Vec<InjectedMemory>,
}

impl InjectedContext {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skills.is_empty() && self.memories.is_empty()
    }
}

/// A memory is one sentence, not a document (12 §B2).
pub const TEXT_MAX: usize = 500;
/// How long a deleted entry stays restorable (12 §B5).
pub const RECENTLY_DELETED_DAYS: i64 = 30;
/// Roughly what the core set may cost in a frozen prompt, in characters — four per token, so
/// 1,500 tokens (12 §B4). Counted in characters because nothing here talks to a tokenizer.
pub const CORE_SET_MAX_CHARS: usize = 6_000;
/// The same ceiling for the long tail, at ~800 tokens.
pub const LONG_TAIL_MAX_CHARS: usize = 3_200;
/// How many long-tail entries one message may pull in.
pub const LONG_TAIL_LIMIT: usize = 8;
/// Entries and skills injected within this many turns are not injected again: the model still
/// has them, and a second copy only costs tokens (12 §A4, §B4).
pub const RECENT_TURNS: usize = 6;
/// At most this many proposals per turn, so a chatty model does not bury the feed in cards.
pub const MAX_PROPOSALS_PER_TURN: u32 = 2;

/// The sentence a card refuses with when a proposal is too long, or `None` when it fits.
#[must_use]
pub fn text_problem(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return Some("A memory needs text.".to_owned());
    }
    let count = text.chars().count();
    if count > TEXT_MAX {
        return Some(format!(
            "{count} characters; a memory is at most {TEXT_MAX}. Keep it to the standing fact, \
             not the task it came from."
        ));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_kinds_are_the_frozen_ones() {
        assert!(MemoryKind::Instruction.is_standing());
        assert!(MemoryKind::Preference.is_standing());
        assert!(!MemoryKind::Fact.is_standing());
        assert!(!MemoryKind::Note.is_standing());
    }

    #[test]
    fn a_memory_is_one_sentence() {
        assert!(text_problem("prefer pnpm").is_none());
        assert!(text_problem("   ").is_some());
        assert!(text_problem(&"x".repeat(TEXT_MAX + 1)).is_some());
    }
}
