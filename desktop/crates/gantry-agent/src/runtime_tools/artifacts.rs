//! `gantry__create_artifact`, `update_artifact`, `edit_artifact`, `read_artifact`
//! (docs/plan/13 §2): `app` tier, present in every chat. Create and update stream their
//! arguments; create, update and edit answer with the panel's render report.

use std::sync::Arc;

use gantry_connectors::{ToolCallRequest, ToolEventSink, ToolOutcome};
use gantry_core::{AgentEventKind, ArtifactId, RiskTier, ToolDef, VersionSource, artifact};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::artifacts::{ArtifactError, Artifacts, CreateRequest, Edit, Origin, registry};

pub const CREATE: &str = "create_artifact";
pub const UPDATE: &str = "update_artifact";
pub const EDIT: &str = "edit_artifact";
pub const READ: &str = "read_artifact";

pub const NAMES: [&str; 4] = [CREATE, UPDATE, EDIT, READ];

/// Whether a model-facing name (`gantry__create_artifact`) is one of these tools.
#[must_use]
pub fn is_artifact_tool(model_tool_name: &str) -> bool {
    model_tool_name
        .strip_prefix("gantry__")
        .is_some_and(|t| NAMES.contains(&t))
}

fn type_schema() -> Value {
    json!({
        "type": "string",
        "enum": registry::ids(),
        "description": "markdown for documents; code for a single source file (set language); svg for an illustration; mermaid for a diagram (flowchart, sequence, class, state, ER, gantt); html for a complete page (a whole document, inline CSS and scripts only, no network); react for an interactive component (default export, hooks allowed, imports only from react, react-dom, react/jsx-runtime, lucide-react, recharts, clsx)."
    })
}

#[must_use]
pub fn definitions() -> Vec<ToolDef> {
    let mut create = ToolDef::new(
        CREATE,
        "Create an artifact: substantial, self-contained content shown in a panel beside the \
         chat and kept as versions (a document, a file, a page, a diagram, a component). Not \
         for answers or explanations; introduce it in one sentence in the chat. Choose the \
         narrowest type. The result reports whether it rendered; on a render error fix it, at \
         most twice, then explain.",
        json!({
            "type": "object",
            "properties": {
                "type": type_schema(),
                "title": { "type": "string", "maxLength": artifact::TITLE_MAX_CHARS, "description": "Short and specific." },
                "language": { "type": "string", "description": "code: the language id (rust, python, typescript…); react: tsx (default) or jsx." },
                "content": { "type": "string", "description": "The whole content. At most 1 MB." },
                "summary": { "type": "string", "maxLength": artifact::SUMMARY_MAX_CHARS, "description": "One line shown in lists." }
            },
            "required": ["type", "title", "content"],
            "additionalProperties": false
        }),
        RiskTier::App,
    );
    create.stream_args = true;
    let mut update = ToolDef::new(
        UPDATE,
        "Rewrite an artifact in full as a new version. Use it for large changes; for small \
         ones use edit_artifact. Read an artifact you did not write in this turn before \
         changing it.",
        json!({
            "type": "object",
            "properties": {
                "artifact_id": { "type": "string" },
                "content": { "type": "string", "description": "The whole new content." },
                "title": { "type": "string", "maxLength": artifact::TITLE_MAX_CHARS },
                "summary": { "type": "string", "maxLength": artifact::SUMMARY_MAX_CHARS }
            },
            "required": ["artifact_id", "content"],
            "additionalProperties": false
        }),
        RiskTier::App,
    );
    update.stream_args = true;
    let edit = ToolDef::new(
        EDIT,
        "Change parts of an artifact by exact text replacement, as a new version. Each \
         old_string must match exactly once (copy it verbatim, whitespace included) unless \
         replace_all is set. Prefer this over update_artifact for changes under about a \
         third of the content.",
        json!({
            "type": "object",
            "properties": {
                "artifact_id": { "type": "string" },
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": { "type": "string" },
                            "new_string": { "type": "string" },
                            "replace_all": { "type": "boolean", "default": false }
                        },
                        "required": ["old_string", "new_string"],
                        "additionalProperties": false
                    }
                },
                "summary": { "type": "string", "maxLength": artifact::SUMMARY_MAX_CHARS }
            },
            "required": ["artifact_id", "edits"],
            "additionalProperties": false
        }),
        RiskTier::App,
    );
    let mut read = ToolDef::new(
        READ,
        "Read an artifact's content and version history: the current version, or the one \
         asked for. Call it before editing an artifact you did not write in this turn, and \
         after the user edited one.",
        json!({
            "type": "object",
            "properties": {
                "artifact_id": { "type": "string" },
                "version": { "type": "integer", "minimum": 1 }
            },
            "required": ["artifact_id"],
            "additionalProperties": false
        }),
        RiskTier::App,
    );
    read.parallel_safe = true;
    vec![create, update, edit, read]
}

#[derive(Deserialize)]
struct CreateArgs {
    #[serde(rename = "type")]
    artifact_type: String,
    title: String,
    language: Option<String>,
    content: String,
    summary: Option<String>,
}

#[derive(Deserialize)]
struct UpdateArgs {
    artifact_id: String,
    content: String,
    title: Option<String>,
    summary: Option<String>,
}

#[derive(Deserialize)]
struct EditArgs {
    artifact_id: String,
    edits: Vec<Edit>,
    summary: Option<String>,
}

#[derive(Deserialize)]
struct ReadArgs {
    artifact_id: String,
    version: Option<u32>,
}

fn parse<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T, ToolOutcome> {
    serde_json::from_value(args.clone())
        .map_err(|e| ToolOutcome::error(format!("invalid arguments: {e}")))
}

fn artifact_id(s: &str) -> Result<ArtifactId, ToolOutcome> {
    s.parse()
        .map_err(|_| ToolOutcome::error(format!("`{s}` is not an artifact id")))
}

fn failure(err: ArtifactError) -> ToolOutcome {
    ToolOutcome::error(err.to_string())
}

pub async fn call(
    service: &Arc<Artifacts>,
    req: &ToolCallRequest,
    sink: &Arc<dyn ToolEventSink>,
) -> ToolOutcome {
    let origin = Origin {
        tool_call_id: Some(req.call_id.as_str().to_owned()),
        message_id: None,
    };
    match req.tool.as_str() {
        CREATE => {
            let args: CreateArgs = match parse(&req.args) {
                Ok(a) => a,
                Err(e) => return e,
            };
            let artifact_type = args.artifact_type.clone();
            let created = match service.create(
                req.scope.chat_id,
                CreateRequest {
                    artifact_type: args.artifact_type,
                    title: args.title,
                    language: args.language,
                    content: args.content,
                    summary: args.summary,
                },
                origin,
            ) {
                Ok(a) => a,
                Err(e) => return failure(e),
            };
            sink.event(AgentEventKind::ArtifactCreated {
                artifact_id: created.id,
                version: 1,
                artifact_type: artifact_type.clone(),
                title: created.title.clone(),
            });
            let render = service.await_render(created.id, 1, &artifact_type).await;
            ToolOutcome::json(json!({
                "artifact_id": created.id.to_string(),
                "version": 1,
                "render": render,
            }))
        }
        UPDATE => {
            let args: UpdateArgs = match parse(&req.args) {
                Ok(a) => a,
                Err(e) => return e,
            };
            let id = match artifact_id(&args.artifact_id) {
                Ok(id) => id,
                Err(e) => return e,
            };
            if let Err(e) = service.writable(id, req.scope.chat_id) {
                return failure(e);
            }
            let (a, version) =
                match service.update(id, args.content, args.title, args.summary, origin) {
                    Ok(v) => v,
                    Err(e) => return failure(e),
                };
            sink.event(AgentEventKind::ArtifactUpdated {
                artifact_id: id,
                version,
                source: VersionSource::ModelUpdate,
                title: a.title.clone(),
            });
            let render = service.await_render(id, version, &a.artifact_type).await;
            ToolOutcome::json(
                json!({ "artifact_id": id.to_string(), "version": version, "render": render }),
            )
        }
        EDIT => {
            let args: EditArgs = match parse(&req.args) {
                Ok(a) => a,
                Err(e) => return e,
            };
            let id = match artifact_id(&args.artifact_id) {
                Ok(id) => id,
                Err(e) => return e,
            };
            if let Err(e) = service.writable(id, req.scope.chat_id) {
                return failure(e);
            }
            let (a, version) = match service.edit(id, &args.edits, args.summary, origin) {
                Ok(v) => v,
                Err(e) => return failure(e),
            };
            sink.event(AgentEventKind::ArtifactUpdated {
                artifact_id: id,
                version,
                source: VersionSource::ModelEdit,
                title: a.title.clone(),
            });
            let render = service.await_render(id, version, &a.artifact_type).await;
            ToolOutcome::json(
                json!({ "artifact_id": id.to_string(), "version": version, "render": render }),
            )
        }
        READ => {
            let args: ReadArgs = match parse(&req.args) {
                Ok(a) => a,
                Err(e) => return e,
            };
            let id = match artifact_id(&args.artifact_id) {
                Ok(id) => id,
                Err(e) => return e,
            };
            if let Err(e) = service.readable(id, req.scope.chat_id) {
                return failure(e);
            }
            match service.read(id, args.version) {
                Ok(c) => ToolOutcome::json(json!({
                    "artifact_id": id.to_string(),
                    "type": c.artifact.artifact_type,
                    "title": c.artifact.title,
                    "language": c.artifact.language,
                    "summary": c.artifact.summary,
                    "version": c.version,
                    "current_version": c.artifact.current_version,
                    "content": c.content,
                    "versions": c.versions.iter().map(|v| json!({
                        "version": v.version,
                        "source": v.source,
                        "created_at": v.created_at,
                        "note": v.note,
                    })).collect::<Vec<_>>(),
                })),
                Err(e) => failure(e),
            }
        }
        other => ToolOutcome::error(format!("unknown artifact tool {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_are_app_tier_and_create_streams() {
        let defs = definitions();
        assert_eq!(defs.len(), 4);
        assert!(defs.iter().all(|d| d.tier == RiskTier::App));
        assert!(defs[0].stream_args && defs[1].stream_args);
        assert!(!defs[2].stream_args);
        let enum_values = defs[0].input_schema["properties"]["type"]["enum"]
            .as_array()
            .unwrap()
            .len();
        assert_eq!(enum_values, registry::TYPES.len());
        assert!(is_artifact_tool("gantry__create_artifact"));
        assert!(!is_artifact_tool("gantry__clock"));
    }
}
