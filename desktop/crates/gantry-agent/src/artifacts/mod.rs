//! The artifact service (docs/plan/13): versions in the store, content in the blob store,
//! and the render-verified handshake with the panel (§2): a tool that created or changed an
//! executable artifact waits, up to [`RENDER_TIMEOUT`], for the panel's report before it
//! answers the model.

pub mod edits;
pub mod registry;
pub mod sandbox;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use gantry_core::{
    ArtifactContent, ArtifactDto, ArtifactId, ChatId, GantryError, MessageId, RenderReport,
    VersionSource, artifact,
};
use gantry_store::{
    BlobStore, Store,
    repos::{
        artifacts::{self, NewVersion},
        blobs, chats, tool_calls,
    },
};
use tokio::sync::oneshot;

pub use edits::{Edit, apply_edits};
pub use registry::{Execution, TypeInfo};

/// How long a tool result waits for the panel (13 §2).
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(3);

/// Where a model-made version came from.
#[derive(Debug, Clone, Default)]
pub struct Origin {
    pub tool_call_id: Option<String>,
    pub message_id: Option<MessageId>,
}

#[derive(Debug, Clone)]
pub struct CreateRequest {
    pub artifact_type: String,
    pub title: String,
    pub language: Option<String>,
    pub content: String,
    pub summary: Option<String>,
}

/// What went wrong, phrased for the model (tools) or the UI (commands).
#[derive(Debug, Clone, thiserror::Error)]
pub enum ArtifactError {
    #[error("{0}")]
    Invalid(String),
    #[error("artifact {0} not found")]
    NotFound(ArtifactId),
    #[error("artifact {0} belongs to another chat")]
    Forbidden(ArtifactId),
    #[error("{0}")]
    Store(String),
}

impl From<ArtifactError> for GantryError {
    fn from(err: ArtifactError) -> Self {
        match err {
            ArtifactError::Invalid(m) => GantryError::invalid(m),
            ArtifactError::NotFound(id) => GantryError::not_found(format!("artifact {id}")),
            ArtifactError::Forbidden(id) => {
                GantryError::invalid(format!("artifact {id} belongs to another chat"))
            }
            ArtifactError::Store(m) => GantryError::Store(m),
        }
    }
}

impl From<gantry_store::StoreError> for ArtifactError {
    fn from(err: gantry_store::StoreError) -> Self {
        ArtifactError::Store(err.to_string())
    }
}

type Waiters = Mutex<HashMap<(ArtifactId, u32), oneshot::Sender<RenderReport>>>;

pub struct Artifacts {
    store: Arc<Store>,
    blobs: Arc<BlobStore>,
    waiters: Waiters,
    render_timeout: Duration,
}

impl std::fmt::Debug for Artifacts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Artifacts").finish_non_exhaustive()
    }
}

impl Artifacts {
    #[must_use]
    pub fn new(store: Arc<Store>, blobs: Arc<BlobStore>) -> Self {
        Self {
            store,
            blobs,
            waiters: Mutex::new(HashMap::new()),
            render_timeout: RENDER_TIMEOUT,
        }
    }

    /// Tests shorten the wait.
    #[must_use]
    pub fn with_render_timeout(mut self, timeout: Duration) -> Self {
        self.render_timeout = timeout;
        self
    }

    fn validate(
        artifact_type: &str,
        title: Option<&str>,
        content: Option<&str>,
        summary: Option<&str>,
    ) -> Result<(), ArtifactError> {
        if registry::get(artifact_type).is_none() {
            return Err(ArtifactError::Invalid(format!(
                "unknown artifact type `{artifact_type}`; one of {}",
                registry::ids().join(", ")
            )));
        }
        if let Some(t) = title {
            if t.trim().is_empty() {
                return Err(ArtifactError::Invalid("title is empty".into()));
            }
            if t.chars().count() > artifact::TITLE_MAX_CHARS {
                return Err(ArtifactError::Invalid(format!(
                    "title is longer than {} characters",
                    artifact::TITLE_MAX_CHARS
                )));
            }
        }
        if let Some(c) = content
            && c.len() > artifact::CONTENT_MAX_BYTES
        {
            return Err(ArtifactError::Invalid(format!(
                "content is larger than {} KB",
                artifact::CONTENT_MAX_BYTES / 1024
            )));
        }
        if let Some(s) = summary
            && s.chars().count() > artifact::SUMMARY_MAX_CHARS
        {
            return Err(ArtifactError::Invalid(format!(
                "summary is longer than {} characters",
                artifact::SUMMARY_MAX_CHARS
            )));
        }
        Ok(())
    }

    fn new_version(
        &self,
        content: &str,
        source: VersionSource,
        origin: &Origin,
        note: Option<String>,
    ) -> Result<NewVersion, ArtifactError> {
        let hash = self.blobs.put(content.as_bytes())?;
        Ok(NewVersion {
            content_blob_hash: hash,
            size: content.len() as u64,
            source,
            tool_call_id: origin.tool_call_id.clone(),
            message_id: origin.message_id,
            note,
            text: content.to_owned(),
        })
    }

    pub fn create(
        &self,
        chat_id: ChatId,
        req: CreateRequest,
        origin: Origin,
    ) -> Result<ArtifactDto, ArtifactError> {
        Self::validate(
            &req.artifact_type,
            Some(&req.title),
            Some(&req.content),
            req.summary.as_deref(),
        )?;
        let version = self.new_version(&req.content, VersionSource::ModelCreate, &origin, None)?;
        let mime = registry::get(&req.artifact_type).map(|t| t.mime.to_owned());
        self.store
            .write_blocking(move |conn| {
                let chat = chats::get(conn, chat_id)?.ok_or_else(|| {
                    gantry_store::StoreError::Other(format!("chat {chat_id} not found"))
                })?;
                blobs::record(
                    conn,
                    &version.content_blob_hash,
                    i64::try_from(version.size).unwrap_or(i64::MAX),
                    mime.as_deref(),
                )?;
                // The persister writes the call's row within a flush; when it is already
                // there it names the message, otherwise the origin does.
                let message_id = origin.message_id.or_else(|| {
                    origin
                        .tool_call_id
                        .as_deref()
                        .and_then(|c| {
                            tool_calls::get(conn, &gantry_core::CallId(c.to_owned()))
                                .ok()
                                .flatten()
                        })
                        .map(|c| c.message_id)
                });
                artifacts::create(
                    conn,
                    chat_id,
                    chat.project_id.map(|p| p.to_string()),
                    &req.artifact_type,
                    req.title.trim(),
                    req.language
                        .as_deref()
                        .map(str::trim)
                        .filter(|l| !l.is_empty()),
                    req.summary
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty()),
                    message_id,
                    &version,
                )
            })
            .map_err(ArtifactError::from)
    }

    /// The artifact when `chat_id` may read it: its own chat, or another chat of the same
    /// project (13 §9).
    pub fn readable(&self, id: ArtifactId, chat_id: ChatId) -> Result<ArtifactDto, ArtifactError> {
        let a = self.get(id)?;
        if a.chat_id == chat_id {
            return Ok(a);
        }
        let project = self
            .store
            .read(move |conn| Ok(chats::get(conn, chat_id)?.and_then(|c| c.project_id)))?;
        match (project, a.project_id) {
            (Some(p), Some(q)) if p == q => Ok(a),
            _ => Err(ArtifactError::Forbidden(id)),
        }
    }

    /// The artifact when `chat_id` may change it: only its own chat.
    pub fn writable(&self, id: ArtifactId, chat_id: ChatId) -> Result<ArtifactDto, ArtifactError> {
        let a = self.get(id)?;
        if a.chat_id == chat_id {
            Ok(a)
        } else {
            Err(ArtifactError::Forbidden(id))
        }
    }

    pub fn get(&self, id: ArtifactId) -> Result<ArtifactDto, ArtifactError> {
        self.store
            .read(move |conn| artifacts::get(conn, id))?
            .ok_or(ArtifactError::NotFound(id))
    }

    #[allow(clippy::too_many_arguments)]
    fn append(
        &self,
        id: ArtifactId,
        content: &str,
        title: Option<&str>,
        summary: Option<&str>,
        source: VersionSource,
        origin: Origin,
        note: Option<String>,
    ) -> Result<(ArtifactDto, u32), ArtifactError> {
        let current = self.get(id)?;
        Self::validate(&current.artifact_type, title, Some(content), summary)?;
        let version = self.new_version(content, source, &origin, note)?;
        let mime = registry::get(&current.artifact_type).map(|t| t.mime.to_owned());
        let title = title.map(str::trim).map(str::to_owned);
        let summary = summary.map(str::trim).map(str::to_owned);
        let number = self
            .store
            .write_blocking(move |conn| {
                blobs::record(
                    conn,
                    &version.content_blob_hash,
                    i64::try_from(version.size).unwrap_or(i64::MAX),
                    mime.as_deref(),
                )?;
                artifacts::add_version(conn, id, title.as_deref(), summary.as_deref(), &version)
            })
            .map_err(ArtifactError::from)?;
        Ok((self.get(id)?, number))
    }

    /// A full rewrite by the model.
    pub fn update(
        &self,
        id: ArtifactId,
        content: String,
        title: Option<String>,
        summary: Option<String>,
        origin: Origin,
    ) -> Result<(ArtifactDto, u32), ArtifactError> {
        self.append(
            id,
            &content,
            title.as_deref(),
            summary.as_deref(),
            VersionSource::ModelUpdate,
            origin,
            None,
        )
    }

    /// Targeted replacements by the model.
    pub fn edit(
        &self,
        id: ArtifactId,
        edits: &[Edit],
        summary: Option<String>,
        origin: Origin,
    ) -> Result<(ArtifactDto, u32), ArtifactError> {
        let current = self.read(id, None)?;
        let next = apply_edits(&current.content, edits).map_err(ArtifactError::Invalid)?;
        self.append(
            id,
            &next,
            None,
            summary.as_deref(),
            VersionSource::ModelEdit,
            origin,
            Some(format!("{} edit(s)", edits.len())),
        )
    }

    /// The user saved the source in the panel.
    pub fn save_user_version(
        &self,
        id: ArtifactId,
        content: String,
    ) -> Result<(ArtifactDto, u32), ArtifactError> {
        self.append(
            id,
            &content,
            None,
            None,
            VersionSource::UserEdit,
            Origin::default(),
            None,
        )
    }

    /// The user restored an earlier version: a new version with the same content.
    pub fn restore(
        &self,
        id: ArtifactId,
        version: u32,
    ) -> Result<(ArtifactDto, u32), ArtifactError> {
        let old = self.read(id, Some(version))?;
        self.append(
            id,
            &old.content,
            None,
            None,
            VersionSource::UserRestore,
            Origin::default(),
            Some(format!("restored v{version}")),
        )
    }

    /// One version's content with the artifact and its history; the current version by default.
    pub fn read(
        &self,
        id: ArtifactId,
        version: Option<u32>,
    ) -> Result<ArtifactContent, ArtifactError> {
        let (artifact, versions, hash) = self.store.read(move |conn| {
            let Some(a) = artifacts::get(conn, id)? else {
                return Ok((None, Vec::new(), None));
            };
            let v = version.unwrap_or(a.current_version);
            let hash = artifacts::content_hash(conn, id, v)?;
            let versions = artifacts::versions(conn, id)?;
            Ok((Some(a), versions, hash))
        })?;
        let artifact = artifact.ok_or(ArtifactError::NotFound(id))?;
        let v = version.unwrap_or(artifact.current_version);
        let hash = hash
            .ok_or_else(|| ArtifactError::Invalid(format!("artifact {id} has no version {v}")))?;
        let bytes = self.blobs.get(&hash)?;
        Ok(ArtifactContent {
            artifact,
            version: v,
            content: String::from_utf8_lossy(&bytes).into_owned(),
            versions,
        })
    }

    pub fn list_for_chat(&self, chat_id: ChatId) -> Result<Vec<ArtifactDto>, ArtifactError> {
        Ok(self
            .store
            .read(move |conn| artifacts::list_for_chat(conn, chat_id))?)
    }

    pub fn list_for_project(&self, project_id: &str) -> Result<Vec<ArtifactDto>, ArtifactError> {
        let project_id = project_id.to_owned();
        Ok(self
            .store
            .read(move |conn| artifacts::list_for_project(conn, &project_id))?)
    }

    /// Every artifact across every chat, newest change first (the library, 13 §9).
    pub fn list_all(&self) -> Result<Vec<ArtifactDto>, ArtifactError> {
        Ok(self.store.read(artifacts::list_all)?)
    }

    /// The panel's report for one version; `true` when a tool was waiting for it.
    pub fn report_render(&self, id: ArtifactId, version: u32, report: RenderReport) -> bool {
        let waiter = self
            .waiters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&(id, version));
        match waiter {
            Some(tx) => tx.send(report).is_ok(),
            None => false,
        }
    }

    /// Waits for the panel's report on an executable type; parent-rendered types are `ok` at
    /// once. Times out to `pending` (13 §2).
    pub async fn await_render(
        &self,
        id: ArtifactId,
        version: u32,
        artifact_type: &str,
    ) -> RenderReport {
        let executable =
            registry::get(artifact_type).is_some_and(|t| t.execution == Execution::Sandbox);
        if !executable {
            return RenderReport::ok();
        }
        let (tx, rx) = oneshot::channel();
        self.waiters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((id, version), tx);
        match tokio::time::timeout(self.render_timeout, rx).await {
            Ok(Ok(report)) => report,
            _ => {
                self.waiters
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&(id, version));
                RenderReport::pending()
            }
        }
    }
}

/// The note appended to the transcript after a user edit or restore (13 §7): the full
/// content when it is small enough, otherwise a pointer to `gantry__read_artifact`.
#[must_use]
pub fn user_change_note(
    artifact: &ArtifactDto,
    version: u32,
    content: &str,
    source: VersionSource,
) -> String {
    let verb = match source {
        VersionSource::UserRestore => "restored an earlier version of",
        _ => "edited",
    };
    let head = format!(
        "The user {verb} artifact {} (\"{}\", now v{version}).",
        artifact.id, artifact.title
    );
    if content.len() <= USER_NOTE_INLINE_BYTES {
        format!("{head} Its content is now:\n\n{content}")
    } else {
        format!(
            "{head} The content is too long to include here; call gantry__read_artifact before changing it."
        )
    }
}

/// About 8,000 tokens (13 §7).
pub const USER_NOTE_INLINE_BYTES: usize = 32_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_note_inlines_small_content_only() {
        let a = ArtifactDto {
            id: ArtifactId::new(),
            chat_id: ChatId::new(),
            project_id: None,
            artifact_type: "markdown".into(),
            title: "Plan".into(),
            language: None,
            summary: None,
            current_version: 2,
            created_by_message_id: None,
            created_at: 0,
            updated_at: 0,
        };
        let small = user_change_note(&a, 2, "# Plan", VersionSource::UserEdit);
        assert!(small.contains("edited artifact"));
        assert!(small.ends_with("# Plan"));
        let big = user_change_note(
            &a,
            3,
            &"x".repeat(USER_NOTE_INLINE_BYTES + 1),
            VersionSource::UserRestore,
        );
        assert!(big.contains("restored an earlier version"));
        assert!(big.contains("gantry__read_artifact"));
    }
}
