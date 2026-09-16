//! First-party connector: Code editor (`docs/connectors/code-editor.md`).
//!
//! Surgical changes to files that already exist: replace a passage, insert at an anchor, apply
//! a patch, undo the last change. Four tools and nothing else. It does not read files, list
//! them or search them — that is `filesystem` — and it does not run anything — that is `shell`.
//! Everything about where a file may be, how it is written and how a change is recorded lives
//! in `gantry-workspace`, which all three share.

#![forbid(unsafe_code)]

use std::sync::Arc;

use async_trait::async_trait;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::{InstanceId, PlanModePolicy, RiskTier, ToolDef};
use gantry_workspace::{Anchor, Applied, Change, TextFile, Workspace, WorkspaceError};
use tokio_util::sync::CancellationToken;

/// The connector manifest, embedded at build time (`docs/plan/03-connector-system.md` §3).
pub const MANIFEST: &str = include_str!("../manifest.json");

/// The connector id; equals the folder name and the tool namespace prefix.
pub const ID: &str = "code-editor";

pub struct CodeEditor {
    descriptor: ConnectorDescriptor,
    workspace: Arc<Workspace>,
}

impl CodeEditor {
    #[must_use]
    pub fn new(namespace: String, instance_id: InstanceId, workspace: Arc<Workspace>) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: namespace,
                name: "Code editor".to_owned(),
                instance_id: Some(instance_id),
                first_party: true,
            },
            workspace,
        }
    }
}

#[async_trait]
impl Connector for CodeEditor {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        Ok(definitions())
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        _sink: Arc<dyn ToolEventSink>,
        cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        // One check, at the top, and none inside: every call here is a single file operation,
        // and abandoning one halfway would leave a half-written file behind — worse than the
        // second it takes to finish. What Stop guarantees is that the calls still queued behind
        // this one do not happen (03 §4).
        if cancel.is_cancelled() {
            return Ok(ToolOutcome::cancelled());
        }
        let path = string(&req.args, "path")
            .ok_or_else(|| ConnectorError::InvalidArgs("`path` is required".to_owned()))?;
        let chat = req.scope.chat_id;
        let roots = match self.workspace.roots(chat) {
            Ok(roots) => roots,
            Err(err) => return Ok(refusal(&err)),
        };

        if req.tool == "undo" {
            return Ok(self.undo(&req, &roots, &path).await);
        }

        let change = match parse_change(&req.tool, &req.args) {
            Ok(change) => change,
            Err(err) => return Err(err),
        };
        match self
            .workspace
            .apply(&roots, chat, req.call_id.as_str(), &path, &change)
            .await
        {
            Ok(applied) => Ok(outcome(&applied, None)),
            Err(err) => Ok(refusal(&err)),
        }
    }
}

impl CodeEditor {
    /// Replays this session's own edits backwards (`code-editor.md` §3). It never crosses a
    /// session boundary and never undoes past an edit whose file has changed since, and it
    /// writes a new journal row rather than deleting one: the history stays true.
    async fn undo(
        &self,
        req: &ToolCallRequest,
        roots: &gantry_workspace::Roots,
        path: &str,
    ) -> ToolOutcome {
        let chat = req.scope.chat_id;
        let steps = req
            .args
            .get("steps")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1)
            .max(1) as usize;

        let scoped = match roots.resolve(path) {
            Ok(scoped) => scoped,
            Err(err) => return refusal(&WorkspaceError::Scope(err)),
        };
        let display = scoped.path.display().to_string();
        let history = match self.workspace.journal().history(chat, &display) {
            Ok(history) => history,
            Err(err) => return refusal(&WorkspaceError::Journal(err)),
        };
        let mut undoable: Vec<_> = history
            .into_iter()
            .filter(|e| e.reverted_at.is_none())
            .collect();
        undoable.reverse();
        if undoable.is_empty() {
            return ToolOutcome::error(format!(
                "this session has not edited {display}, so there is nothing to undo."
            ));
        }
        let steps = steps.min(undoable.len());
        let chosen = &undoable[..steps];

        // The newest edit's result must still be what is on disk, or something else has
        // written the file and undoing would throw that away.
        let current = match scoped.read() {
            Ok(bytes) => gantry_workspace::text::hash(&bytes),
            Err(err) => return refusal(&WorkspaceError::Scope(err)),
        };
        if chosen[0].after_blob_hash.as_deref() != Some(current.as_str()) {
            return ToolOutcome::error(format!(
                "{display} has changed since Gantry last edited it, so undoing would discard \
                 that change. Read the file and edit it instead."
            ));
        }

        let oldest = &chosen[steps - 1];
        let Some(hash) = oldest.before_blob_hash.as_deref() else {
            return ToolOutcome::error(format!(
                "{display} did not exist before that edit; delete it yourself if that is what \
                 you meant."
            ));
        };
        let restored = match self.workspace.journal().content(hash) {
            Ok(bytes) => bytes,
            Err(err) => return refusal(&WorkspaceError::Journal(err)),
        };
        let text = match TextFile::decode(&display, &restored) {
            Ok(file) => file.text,
            Err(err) => return refusal(&WorkspaceError::Text(err)),
        };

        match self
            .workspace
            .apply(
                roots,
                chat,
                req.call_id.as_str(),
                &display,
                &Change::Restore { text },
            )
            .await
        {
            Ok(applied) => {
                let ids: Vec<String> = chosen.iter().map(|e| e.id.clone()).collect();
                let undone = ids.len();
                if let Err(err) = self
                    .workspace
                    .journal()
                    .mark_reverted(ids, applied.edit_id.clone())
                    .await
                {
                    log::warn!("{display}: the undone edits stay open in the journal: {err}");
                }
                outcome(
                    &applied,
                    Some(format!(
                        "undid {undone} {} to this file",
                        if undone == 1 { "edit" } else { "edits" }
                    )),
                )
            }
            Err(err) => refusal(&err),
        }
    }
}

/// The four tools. Every one of them is `write` rather than `write_external`: every path is
/// inside a folder the user attached, and every change is journalled and revertible, which is
/// exactly what the tier means (04 §2).
#[must_use]
pub fn definitions() -> Vec<ToolDef> {
    let mut replace = ToolDef::new(
        "replace",
        "Replace an exact passage of a file with new text. `old` must appear exactly `count` \
         times, including its indentation; widen it with surrounding lines when it does not. \
         Read the file first.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path of the file to change." },
                "old": { "type": "string", "description": "Exact text to find, indentation included." },
                "new": { "type": "string", "description": "What replaces it; empty deletes the passage." },
                "count": { "type": "integer", "minimum": 1, "default": 1,
                           "description": "How many occurrences are expected, and must be found." }
            },
            "required": ["path", "old", "new"],
            "additionalProperties": false
        }),
        RiskTier::Write,
    );
    replace.stream_args = true;

    let insert = ToolDef::new(
        "insert",
        "Insert text into a file, after an exact passage or at a line. Use it when there is \
         nothing to replace: a new import, a new function, a line in a list.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path of the file to change." },
                "text": { "type": "string", "description": "What to insert." },
                "after": { "type": "string",
                           "description": "Exact text to insert after; preferred over at_line." },
                "at_line": { "type": "integer", "minimum": 0,
                             "description": "1-based line to insert before; 0 prepends. Only when there is no anchor." }
            },
            "required": ["path", "text"],
            "additionalProperties": false
        }),
        RiskTier::Write,
    );

    let mut patch = ToolDef::new(
        "apply_patch",
        "Apply a unified diff to one file. Context must match exactly; a hunk that does not \
         apply fails the whole call. Use it for several changes to one file at once.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path of the file to change." },
                "patch": { "type": "string", "description": "A unified diff, with context lines." }
            },
            "required": ["path", "patch"],
            "additionalProperties": false
        }),
        RiskTier::Write,
    );
    patch.stream_args = true;

    let undo = ToolDef::new(
        "undo",
        "Undo this session's own edits to a file, newest first. It cannot undo anything the \
         user or another session did.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Absolute path of the file to restore." },
                "steps": { "type": "integer", "minimum": 1, "default": 1,
                           "description": "How many of this session's edits to that file to undo." }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
        RiskTier::Write,
    );

    for def in [&replace, &insert, &patch, &undo] {
        debug_assert_eq!(def.plan_mode, PlanModePolicy::Deny);
    }
    vec![replace, insert, patch, undo]
}

fn parse_change(tool: &str, args: &serde_json::Value) -> Result<Change, ConnectorError> {
    match tool {
        "replace" => Ok(Change::Replace {
            old: string(args, "old")
                .ok_or_else(|| ConnectorError::InvalidArgs("`old` is required".to_owned()))?,
            new: string(args, "new").unwrap_or_default(),
            count: args
                .get("count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1)
                .max(1) as usize,
        }),
        "insert" => {
            let text = string(args, "text")
                .ok_or_else(|| ConnectorError::InvalidArgs("`text` is required".to_owned()))?;
            let anchor = match (string(args, "after"), args.get("at_line")) {
                (Some(after), _) if !after.is_empty() => Anchor::After(after),
                (_, Some(line)) => Anchor::AtLine(line.as_u64().unwrap_or(0) as usize),
                _ => {
                    return Err(ConnectorError::InvalidArgs(
                        "give either `after` or `at_line`".to_owned(),
                    ));
                }
            };
            Ok(Change::Insert { text, anchor })
        }
        "apply_patch" => Ok(Change::Patch {
            patch: string(args, "patch")
                .ok_or_else(|| ConnectorError::InvalidArgs("`patch` is required".to_owned()))?,
        }),
        other => Err(ConnectorError::UnknownTool(other.to_owned())),
    }
}

/// The result the model reads and the row renders: what changed, and the hunks, so the feed
/// can show a diff without a second call.
fn outcome(applied: &Applied, note: Option<String>) -> ToolOutcome {
    let note = note.or_else(|| applied.note.clone());
    ToolOutcome::json(serde_json::json!({
        "path": applied.path,
        "added": applied.diff.added,
        "removed": applied.diff.removed,
        "hunks": applied.diff.hunks,
        "note": note,
    }))
}

/// Every refusal is a tool error the model can read and recover from, never a failed turn: a
/// near-miss that produces a corrected second call is the whole point (§7).
fn refusal(err: &WorkspaceError) -> ToolOutcome {
    ToolOutcome::error(err.to_string())
}

fn string(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    #[test]
    fn manifest_is_valid_json_with_the_right_id() {
        let manifest: serde_json::Value = serde_json::from_str(super::MANIFEST).unwrap();
        assert_eq!(manifest["manifest_version"], "1");
        assert_eq!(manifest["id"], super::ID);
        assert_eq!(manifest["runtime"]["kind"], "native");
        assert_eq!(manifest["runtime"]["crate"], env!("CARGO_PKG_NAME"));
    }

    #[test]
    fn the_four_tools_are_write_tier_and_hidden_in_plan_mode() {
        let defs = super::definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["replace", "insert", "apply_patch", "undo"]);
        assert!(defs.iter().all(|d| d.tier == gantry_core::RiskTier::Write));
        assert!(
            defs.iter()
                .all(|d| d.plan_mode == gantry_core::PlanModePolicy::Deny)
        );
        // A patch is worth watching as it streams; a whole-file argument is not.
        assert!(defs[2].stream_args);
    }
}
