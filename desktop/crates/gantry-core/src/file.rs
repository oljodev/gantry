//! The edit journal as the rest of the app sees it (docs/plan/06 §3).
//!
//! Every change a connector makes to a file writes one row. That row is what the Changes pane
//! lists, what the diff drawer renders, what per-file and whole-session **Revert** replay, and
//! what `undo` reads. There is no second store of edits anywhere.

use serde::{Deserialize, Serialize};

use crate::ids::ChatId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EditOp {
    Create,
    Modify,
    Delete,
    Rename,
}

impl EditOp {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EditOp::Create => "create",
            EditOp::Modify => "modify",
            EditOp::Delete => "delete",
            EditOp::Rename => "rename",
        }
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "create" => EditOp::Create,
            "modify" => EditOp::Modify,
            "delete" => EditOp::Delete,
            "rename" => EditOp::Rename,
            _ => return None,
        })
    }
}

/// One journalled change, as the Changes pane and the diff drawer read it (15 A19).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct FileEditDto {
    pub id: String,
    pub chat_id: ChatId,
    pub tool_call_id: String,
    pub path: String,
    pub op: EditOp,
    pub added: u32,
    pub removed: u32,
    /// The change as a unified diff. Empty for an operation that has no text form.
    pub diff: String,
    pub applied_at: i64,
    pub reverted_at: Option<i64>,
}
