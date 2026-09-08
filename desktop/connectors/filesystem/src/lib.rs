//! First-party connector: Filesystem (`docs/connectors/filesystem.md`).
//!
//! Ten tools over the folders a chat has attached: list, read, stat, find by name, find by
//! content, write, make a folder, move, copy and delete. Changing a file *in place* is the code
//! editor's; both sit on `gantry-workspace`, which owns the roots, the containment algorithm of
//! §4, the atomic write and the journal, so neither connector decides for itself where the
//! boundary is.
//!
//! Every refusal here is a tool result the model reads rather than a failed turn, and every one
//! says what to do next: which folder to ask for, which tool to use instead, how to reach the
//! rest of a long file. That is §11, and it is the difference between a model that recovers on
//! its next call and one that gives up.

#![forbid(unsafe_code)]

use std::sync::Arc;

use async_trait::async_trait;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::{InstanceId, RiskTier, ToolDef};
use gantry_workspace::{Roots, Scoped, TextFile, Workspace, WorkspaceError, walk};
use tokio_util::sync::CancellationToken;

/// The connector manifest, embedded at build time (`docs/plan/03-connector-system.md` §3).
pub const MANIFEST: &str = include_str!("../manifest.json");

/// The connector id; equals the folder name and the tool namespace prefix.
pub const ID: &str = "filesystem";

/// Lines returned when the model does not say. Enough for most source files, small enough that
/// a stray read of something enormous does not cost a turn's context.
const DEFAULT_LIMIT: usize = 2000;

/// A file this size is not read whole into a prompt, whatever the line window says.
const MAX_READ_BYTES: u64 = 10 * 1024 * 1024;

pub struct Filesystem {
    descriptor: ConnectorDescriptor,
    workspace: Arc<Workspace>,
}

impl Filesystem {
    #[must_use]
    pub fn new(namespace: String, instance_id: InstanceId, workspace: Arc<Workspace>) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: namespace,
                name: "Filesystem".to_owned(),
                instance_id: Some(instance_id),
                first_party: true,
            },
            workspace,
        }
    }
}

#[async_trait]
impl Connector for Filesystem {
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
        _cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        let chat = req.scope.chat_id;
        let roots = match self.workspace.roots(chat) {
            Ok(roots) => roots,
            Err(err) => return Ok(ToolOutcome::error(err.to_string())),
        };
        let args = &req.args;
        let call_id = req.call_id.as_str();

        let outcome = match req.tool.as_str() {
            "read_file" => self.read_file(&roots, chat, args),
            "list_directory" => list_directory(&roots, args),
            "stat" => stat(&roots, args),
            "glob" => glob(&roots, args),
            "grep" => grep(&roots, args),
            "write_file" => self.write_file(&roots, chat, call_id, args).await,
            "create_directory" => self.create_directory(&roots, chat, call_id, args).await,
            "move_path" | "copy_path" => {
                self.transfer(&roots, chat, call_id, args, req.tool == "copy_path")
                    .await
            }
            "delete_path" => self.delete_path(&roots, chat, call_id, args).await,
            other => return Err(ConnectorError::UnknownTool(other.to_owned())),
        };
        Ok(outcome?)
    }
}

impl Filesystem {
    fn read_file(
        &self,
        roots: &Roots,
        chat: gantry_core::ChatId,
        args: &serde_json::Value,
    ) -> Result<ToolOutcome, ConnectorError> {
        let path = required(args, "path")?;
        let offset = number(args, "offset").unwrap_or(0) as usize;
        let limit = number(args, "limit").map_or(DEFAULT_LIMIT, |n| n.max(1) as usize);

        let scoped = match roots.resolve(&path) {
            Ok(scoped) => scoped,
            Err(err) => return Ok(refuse(roots, &path, &err.into())),
        };
        match scoped.stat() {
            Ok(stat) if stat.is_dir => {
                return Ok(ToolOutcome::error(format!(
                    "{} is a folder, not a file. Use list_directory to see what is in it.",
                    scoped.path.display()
                )));
            }
            Ok(stat) if stat.size > MAX_READ_BYTES => {
                return Ok(ToolOutcome::error(format!(
                    "{} is {} bytes, which is too large to read whole. Use grep to find what \
                     you need in it.",
                    scoped.path.display(),
                    stat.size
                )));
            }
            Ok(_) => {}
            Err(err) => return Ok(refuse(roots, &path, &err.into())),
        }

        let read = match self.workspace.read(roots, chat, &path) {
            Ok(read) => read,
            Err(err) => return Ok(refuse(roots, &path, &err)),
        };
        let lines: Vec<&str> = read.file.text.lines().collect();
        let total = lines.len();
        let end = offset.saturating_add(limit).min(total);
        let shown = lines.get(offset..end).unwrap_or_default().join("\n");

        // Position is metadata, never a prefix inside the text (D8): a line number in the
        // content bleeds into what the model writes back, and shows up as an edit indented one
        // level too deep. Nothing is silently truncated either (D9).
        Ok(ToolOutcome::json(serde_json::json!({
            "path": read.scoped.path.display().to_string(),
            "content": shown,
            "first_line": offset + 1,
            "last_line": end,
            "total_lines": total,
            "more": end < total,
            "encoding": read.file.encoding(),
            "line_ending": read.file.line_ending(),
        })))
    }

    async fn write_file(
        &self,
        roots: &Roots,
        chat: gantry_core::ChatId,
        call_id: &str,
        args: &serde_json::Value,
    ) -> Result<ToolOutcome, ConnectorError> {
        let path = required(args, "path")?;
        let content = required(args, "content")?;
        // A file is written the way it was spelled when it existed; a new one gets the plain
        // form, which is what every editor on every platform reads.
        let existing = roots
            .resolve(&path)
            .ok()
            .and_then(|s| s.read().ok())
            .and_then(|bytes| TextFile::decode(&path, &bytes).ok());
        let bytes = existing.map_or_else(
            || content.clone().into_bytes(),
            |file| file.encode(&content),
        );
        match self
            .workspace
            .write(roots, chat, call_id, &path, &bytes)
            .await
        {
            Ok(wrote) => Ok(ToolOutcome::json(serde_json::json!({
                "path": wrote.path,
                "created": wrote.created,
                "added": wrote.diff.added,
                "removed": wrote.diff.removed,
            }))),
            Err(err) => Ok(refuse(roots, &path, &err)),
        }
    }

    async fn create_directory(
        &self,
        roots: &Roots,
        chat: gantry_core::ChatId,
        call_id: &str,
        args: &serde_json::Value,
    ) -> Result<ToolOutcome, ConnectorError> {
        let path = required(args, "path")?;
        match self.workspace.create_dir(roots, chat, call_id, &path).await {
            Ok(path) => Ok(ToolOutcome::json(serde_json::json!({ "path": path }))),
            Err(err) => Ok(refuse(roots, &path, &err)),
        }
    }

    async fn transfer(
        &self,
        roots: &Roots,
        chat: gantry_core::ChatId,
        call_id: &str,
        args: &serde_json::Value,
        copy: bool,
    ) -> Result<ToolOutcome, ConnectorError> {
        let from = required(args, "from")?;
        let to = required(args, "to")?;
        match self
            .workspace
            .transfer(roots, chat, call_id, &from, &to, copy)
            .await
        {
            Ok(moved) => Ok(ToolOutcome::json(serde_json::json!({
                "from": moved.from,
                "to": moved.to,
                "copied": copy,
            }))),
            Err(err) => Ok(refuse(roots, &from, &err)),
        }
    }

    async fn delete_path(
        &self,
        roots: &Roots,
        chat: gantry_core::ChatId,
        call_id: &str,
        args: &serde_json::Value,
    ) -> Result<ToolOutcome, ConnectorError> {
        let path = required(args, "path")?;
        let recursive = args
            .get("recursive")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        match self
            .workspace
            .delete(roots, chat, call_id, &path, recursive, true)
            .await
        {
            Ok(deleted) => Ok(ToolOutcome::json(serde_json::json!({
                "path": deleted.path,
                // "deleted" and "moved to the trash" are different promises, so the result says
                // which one was kept rather than letting the model assume the safer one.
                "trashed": deleted.trashed,
                "revertible": deleted.trashed || !deleted.directory,
                "was_directory": deleted.directory,
            }))),
            Err(err) => Ok(refuse(roots, &path, &err)),
        }
    }
}

fn list_directory(roots: &Roots, args: &serde_json::Value) -> Result<ToolOutcome, ConnectorError> {
    let path = required(args, "path")?;
    let all = flag(args, "all");
    let scoped = match roots.resolve(&path) {
        Ok(scoped) => scoped,
        Err(err) => return Ok(refuse(roots, &path, &err.into())),
    };
    match scoped.stat() {
        Ok(stat) if !stat.is_dir => {
            return Ok(ToolOutcome::error(format!(
                "{} is a file, not a folder. Use read_file to read it.",
                scoped.path.display()
            )));
        }
        Ok(_) => {}
        Err(err) => return Ok(refuse(roots, &path, &err.into())),
    }
    let found = walk::list(&scoped.path, all);
    Ok(ToolOutcome::json(serde_json::json!({
        "path": scoped.path.display().to_string(),
        "entries": found.items,
        "not_listed": found.more,
        "hidden_and_ignored_shown": all,
    })))
}

fn stat(roots: &Roots, args: &serde_json::Value) -> Result<ToolOutcome, ConnectorError> {
    let path = required(args, "path")?;
    let scoped = match roots.resolve(&path) {
        Ok(scoped) => scoped,
        Err(err) => return Ok(refuse(roots, &path, &err.into())),
    };
    match scoped.stat() {
        Ok(stat) => Ok(ToolOutcome::json(serde_json::json!({
            "path": scoped.path.display().to_string(),
            "kind": if stat.is_dir { "dir" } else if stat.is_symlink { "symlink" } else { "file" },
            "size": stat.size,
            "modified": stat.modified,
            "readonly": stat.readonly,
        }))),
        Err(err) => Ok(refuse(roots, &path, &err.into())),
    }
}

fn glob(roots: &Roots, args: &serde_json::Value) -> Result<ToolOutcome, ConnectorError> {
    let pattern = required(args, "pattern")?;
    let all = flag(args, "all");
    let mut results = Vec::new();
    let mut more = 0;
    for root in search_roots(roots, args)? {
        let scoped = match roots.resolve(&root) {
            Ok(scoped) => scoped,
            Err(err) => return Ok(refuse(roots, &root, &err.into())),
        };
        match walk::glob(&scoped.path, &pattern, all) {
            Ok(found) => {
                results.extend(found.items);
                more += found.more;
            }
            Err(err) => {
                return Ok(ToolOutcome::error(format!(
                    "that is not a valid file pattern: {err}"
                )));
            }
        }
    }
    Ok(ToolOutcome::json(serde_json::json!({
        "pattern": pattern,
        "paths": results,
        "not_listed": more,
    })))
}

fn grep(roots: &Roots, args: &serde_json::Value) -> Result<ToolOutcome, ConnectorError> {
    let pattern = required(args, "pattern")?;
    let filter = args.get("files").and_then(serde_json::Value::as_str);
    let case_sensitive = flag(args, "case_sensitive");
    let all = flag(args, "all");
    let mode = args
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("lines");

    let mut found = walk::Found::default();
    for root in search_roots(roots, args)? {
        let scoped = match roots.resolve(&root) {
            Ok(scoped) => scoped,
            Err(err) => return Ok(refuse(roots, &root, &err.into())),
        };
        match walk::grep(&scoped.path, &pattern, filter, case_sensitive, all) {
            Ok(hits) => {
                found.items.extend(hits.items);
                found.more += hits.more;
            }
            Err(message) => return Ok(ToolOutcome::error(message)),
        }
    }
    let by_file = walk::by_file(&found);
    let body = match mode {
        // "Which files mention this" and "show me the lines" have very different token costs,
        // and a model that can only have the expensive one will use it for both.
        "files" => {
            serde_json::json!({ "files": by_file.iter().map(|(p, _)| p).collect::<Vec<_>>() })
        }
        "count" => serde_json::json!({
            "counts": by_file.iter().map(|(p, n)| serde_json::json!({ "path": p, "matches": n })).collect::<Vec<_>>()
        }),
        _ => serde_json::json!({ "matches": found.items }),
    };
    Ok(ToolOutcome::json(serde_json::json!({
        "pattern": pattern,
        "files_with_matches": by_file.len(),
        "not_listed": found.more,
        "result": body,
    })))
}

/// Which folders a search covers: the one named, or every folder attached to the chat.
fn search_roots(roots: &Roots, args: &serde_json::Value) -> Result<Vec<String>, ConnectorError> {
    Ok(match args.get("path").and_then(serde_json::Value::as_str) {
        Some(path) => vec![path.to_owned()],
        None => roots.paths(),
    })
}

/// The failure text is part of the interface (§11). A path outside the folders says which folder
/// would need adding; a path that does not exist says what does, which catches most typos.
fn refuse(roots: &Roots, path: &str, err: &WorkspaceError) -> ToolOutcome {
    let message = match err {
        WorkspaceError::Scope(gantry_workspace::ScopeError::Outside { path, .. }) => format!(
            "{path} is not inside any folder attached to this chat. Attached: {}. Ask the user \
             to attach the folder that contains it rather than working around this.",
            roots.paths().join(", ")
        ),
        WorkspaceError::Scope(gantry_workspace::ScopeError::NotFound(missing)) => {
            match nearest_existing(roots, path) {
                Some(ancestor) => format!("{missing} does not exist. {ancestor} does."),
                None => format!("{missing} does not exist."),
            }
        }
        other => other.to_string(),
    };
    ToolOutcome::error(message)
}

/// The nearest ancestor that is really there, which turns "no such file" into a usable hint.
fn nearest_existing(roots: &Roots, path: &str) -> Option<String> {
    let mut current = std::path::Path::new(path).parent()?;
    for _ in 0..16 {
        let candidate = current.display().to_string();
        if roots.resolve(&candidate).as_ref().is_ok_and(Scoped::exists) {
            return Some(candidate);
        }
        current = current.parent()?;
    }
    None
}

fn required(args: &serde_json::Value, key: &str) -> Result<String, ConnectorError> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ConnectorError::InvalidArgs(format!("`{key}` is required")))
}

fn number(args: &serde_json::Value, key: &str) -> Option<u64> {
    args.get(key).and_then(serde_json::Value::as_u64)
}

fn flag(args: &serde_json::Value, key: &str) -> bool {
    args.get(key)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// The ten tools of §5, with the tiers and flags of §9. The read tools are `parallel_safe`; the
/// writing ones are not, because two writes to the same path in one batch have no defined
/// outcome. `delete_path` always confirms, in every mode including unguarded Auto.
#[must_use]
pub fn definitions() -> Vec<ToolDef> {
    let path = |what: &str| serde_json::json!({ "type": "string", "description": format!("Absolute path of the {what}.") });

    let mut defs = vec![
        ToolDef::new(
            "read_file",
            "Read the text of a file inside a folder attached to this chat. Read a file before \
             editing it. Line positions come back as fields, not inside the text.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": path("file to read"),
                    "offset": { "type": "integer", "minimum": 0, "default": 0,
                                "description": "First line to return, counting from 0." },
                    "limit": { "type": "integer", "minimum": 1, "default": DEFAULT_LIMIT,
                               "description": "How many lines to return." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            RiskTier::Read,
        ),
        ToolDef::new(
            "list_directory",
            "The entries of one folder, with kind, size and modified time. Hidden files and \
             files the folder's ignore rules exclude are left out unless `all` is set; either \
             way they can still be read by name.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": path("folder to list"),
                    "all": { "type": "boolean", "default": false,
                             "description": "Include hidden and ignored entries." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            RiskTier::Read,
        ),
        ToolDef::new(
            "stat",
            "What one path is: file, folder or link, its size and when it changed.",
            serde_json::json!({
                "type": "object",
                "properties": { "path": path("path to describe") },
                "required": ["path"],
                "additionalProperties": false
            }),
            RiskTier::Read,
        ),
        ToolDef::new(
            "glob",
            "Find files by name pattern, most recently changed first. Prefer this over listing \
             a large tree.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string",
                                 "description": "A glob, e.g. `**/*.rs` or `src/**/index.ts`." },
                    "path": { "type": "string",
                              "description": "Folder to search; every attached folder when absent." },
                    "all": { "type": "boolean", "default": false,
                             "description": "Include hidden and ignored files." }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
            RiskTier::Read,
        ),
        ToolDef::new(
            "grep",
            "Find files by content, with a regular expression. `mode` decides how much comes \
             back: `files` for which files match, `count` for how many matches each has, \
             `lines` for the matching lines themselves.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "A regular expression." },
                    "path": { "type": "string",
                              "description": "Folder to search; every attached folder when absent." },
                    "files": { "type": "string",
                               "description": "Glob narrowing which files are searched, e.g. `**/*.ts`." },
                    "mode": { "type": "string", "enum": ["files", "count", "lines"], "default": "lines" },
                    "case_sensitive": { "type": "boolean", "default": false },
                    "all": { "type": "boolean", "default": false,
                             "description": "Include hidden and ignored files." }
                },
                "required": ["pattern"],
                "additionalProperties": false
            }),
            RiskTier::Read,
        ),
        ToolDef::new(
            "write_file",
            "Create a file, or replace one whole. To change part of a file that already exists, \
             use the code editor instead: it is cheaper and leaves a reviewable diff. Missing \
             parent folders are created.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": path("file to write"),
                    "content": { "type": "string", "description": "The whole content of the file." }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
            RiskTier::Write,
        ),
        ToolDef::new(
            "create_directory",
            "Make a folder, and any parent folders that do not exist yet.",
            serde_json::json!({
                "type": "object",
                "properties": { "path": path("folder to create") },
                "required": ["path"],
                "additionalProperties": false
            }),
            RiskTier::Write,
        ),
        ToolDef::new(
            "move_path",
            "Move or rename a file or folder. Both ends must be inside the attached folders.",
            serde_json::json!({
                "type": "object",
                "properties": { "from": path("path to move"), "to": path("new path") },
                "required": ["from", "to"],
                "additionalProperties": false
            }),
            RiskTier::Write,
        ),
        ToolDef::new(
            "copy_path",
            "Copy a file to a new path inside the attached folders.",
            serde_json::json!({
                "type": "object",
                "properties": { "from": path("path to copy"), "to": path("new path") },
                "required": ["from", "to"],
                "additionalProperties": false
            }),
            RiskTier::Write,
        ),
        ToolDef::new(
            "delete_path",
            "Delete a file or folder, to the system trash where there is one. Deleting a folder \
             needs `recursive`.",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": path("path to delete"),
                    "recursive": { "type": "boolean", "default": false,
                                   "description": "Required to delete a folder and everything in it." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            RiskTier::Destructive,
        ),
    ];

    for def in &mut defs {
        def.parallel_safe = def.tier == RiskTier::Read;
        if def.name == "delete_path" {
            def.always_confirm = true;
        }
        if def.name == "write_file" {
            def.stream_args = true;
        }
    }
    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid_json_with_the_right_id() {
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(manifest["manifest_version"], "1");
        assert_eq!(manifest["id"], ID);
        assert_eq!(manifest["runtime"]["kind"], "native");
        assert_eq!(manifest["runtime"]["crate"], env!("CARGO_PKG_NAME"));
    }

    #[test]
    fn the_ten_tools_carry_the_tiers_and_flags_of_section_9() {
        let defs = definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "read_file",
                "list_directory",
                "stat",
                "glob",
                "grep",
                "write_file",
                "create_directory",
                "move_path",
                "copy_path",
                "delete_path",
            ]
        );
        let by_name = |n: &str| defs.iter().find(|d| d.name == n).unwrap().clone();
        for read in ["read_file", "list_directory", "stat", "glob", "grep"] {
            let def = by_name(read);
            assert_eq!(def.tier, RiskTier::Read, "{read}");
            assert!(def.parallel_safe, "{read}");
        }
        for write in ["write_file", "create_directory", "move_path", "copy_path"] {
            let def = by_name(write);
            assert_eq!(def.tier, RiskTier::Write, "{write}");
            assert!(!def.parallel_safe, "{write}");
        }
        let delete = by_name("delete_path");
        assert_eq!(delete.tier, RiskTier::Destructive);
        assert!(delete.always_confirm, "deletion asks in every mode");
    }
}
