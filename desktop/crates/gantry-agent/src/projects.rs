//! Projects: the CRUD side (09 M11). What a project *does* to a chat is in `turn_manager`,
//! because that is where a prompt is frozen and where an open chat is told something changed.
//!
//! Knowledge files come through the same ingest as attachments, which is how a PDF added to a
//! project becomes text the same way a PDF dropped on the composer does. The two differ in one
//! rule: an image is a fine attachment and cannot be knowledge, because knowledge is frozen into
//! every chat's prompt and a picture cannot be.

use std::sync::Arc;

use gantry_core::{
    AttachmentInput, GantryError, NewProject, PROJECT_INSTRUCTIONS_MAX_CHARS, ProjectDetail,
    ProjectFileDto, ProjectFileId, ProjectId, ProjectPatch, ProjectSummary, now_ms,
};
use gantry_store::{BlobStore, Store, repos::projects};

use crate::attachments;

pub struct Projects {
    store: Arc<Store>,
    blobs: Arc<BlobStore>,
}

impl std::fmt::Debug for Projects {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Projects").finish()
    }
}

fn store_err(e: gantry_store::StoreError) -> GantryError {
    GantryError::Store(e.to_string())
}

impl Projects {
    #[must_use]
    pub fn new(store: Arc<Store>, blobs: Arc<BlobStore>) -> Self {
        Self { store, blobs }
    }

    pub fn create(&self, new: NewProject) -> Result<ProjectSummary, GantryError> {
        let name = new.name.trim().to_owned();
        if name.is_empty() {
            return Err(GantryError::invalid("a project needs a name"));
        }
        let now = now_ms();
        let record = projects::ProjectRecord {
            id: ProjectId::new(),
            name,
            description: new.description.trim().to_owned(),
            instructions: cap(&new.instructions),
            workspace_path: new.workspace_path.filter(|p| !p.trim().is_empty()),
            defaults: gantry_core::ProjectDefaults::default(),
            pinned: false,
            sort_order: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
        };
        let summary = projects::summary(&record, 0, 0);
        self.store
            .write_blocking(move |c| projects::insert(c, &record))
            .map_err(store_err)?;
        Ok(summary)
    }

    pub fn list(&self) -> Result<Vec<ProjectSummary>, GantryError> {
        self.store.read(projects::list).map_err(store_err)
    }

    pub fn get(&self, id: ProjectId) -> Result<Option<ProjectDetail>, GantryError> {
        self.store
            .read(move |c| projects::detail(c, id))
            .map_err(store_err)
    }

    /// Applies a patch and returns the whole project, the way settings do: the caller has the
    /// new truth without a second read, and an editor that raced with another window sees what
    /// actually landed.
    pub fn update(&self, id: ProjectId, patch: ProjectPatch) -> Result<ProjectDetail, GantryError> {
        self.store
            .write_blocking(move |c| {
                let Some(mut row) = projects::get(c, id)? else {
                    return Ok(());
                };
                if let Some(name) = patch.name {
                    let name = name.trim().to_owned();
                    if !name.is_empty() {
                        row.name = name;
                    }
                }
                if let Some(description) = patch.description {
                    row.description = description.trim().to_owned();
                }
                if let Some(instructions) = patch.instructions {
                    row.instructions = cap(&instructions);
                }
                if let Some(path) = patch.workspace_path {
                    row.workspace_path = path.filter(|p| !p.trim().is_empty());
                }
                if let Some(defaults) = patch.defaults {
                    row.defaults = defaults;
                }
                if let Some(pinned) = patch.pinned {
                    row.pinned = pinned;
                }
                if let Some(archived) = patch.archived {
                    row.archived_at = archived.then(now_ms);
                }
                row.updated_at = now_ms();
                projects::update(c, &row)
            })
            .map_err(store_err)?;
        self.get(id)?
            .ok_or_else(|| GantryError::not_found(format!("project {id}")))
    }

    /// Deletes the project and releases its chats (migration 0014). Archiving is what hides a
    /// project you want to keep; this is for one you do not.
    pub fn delete(&self, id: ProjectId) -> Result<(), GantryError> {
        self.store
            .write_blocking(move |c| projects::delete(c, id))
            .map_err(store_err)
    }

    /// Adds one knowledge file, extracting its text (09 M11, 06 §8).
    pub fn add_file(
        &self,
        id: ProjectId,
        input: AttachmentInput,
    ) -> Result<ProjectFileDto, GantryError> {
        let knowledge = attachments::knowledge(&self.blobs, input)?;
        let file = ProjectFileDto {
            id: ProjectFileId::new(),
            project_id: id,
            name: knowledge.name,
            mime: knowledge.mime,
            size: knowledge.size,
            blob_hash: knowledge.blob_hash,
            text_chars: u32::try_from(knowledge.text.chars().count()).unwrap_or(u32::MAX),
            created_at: now_ms(),
        };
        let written = file.clone();
        self.store
            .write_blocking(move |c| projects::add_file(c, &file, Some(&knowledge.text)))
            .map_err(store_err)?;
        Ok(written)
    }

    pub fn remove_file(&self, id: ProjectId, file_id: ProjectFileId) -> Result<(), GantryError> {
        self.store
            .write_blocking(move |c| projects::remove_file(c, id, file_id))
            .map_err(store_err)
    }

    /// Pins a skill to the project, which pins it for every chat in it (12 §A6).
    pub fn pin_skill(
        &self,
        id: ProjectId,
        skill_id: &str,
        pinned: bool,
    ) -> Result<(), GantryError> {
        let skill_id = skill_id.to_owned();
        self.store
            .write_blocking(move |c| {
                if pinned {
                    gantry_store::repos::skills::pin_to_project(c, id, &skill_id)
                } else {
                    gantry_store::repos::skills::unpin_from_project(c, id, &skill_id)
                }
            })
            .map_err(store_err)
    }

    /// The chats filed in this project, newest activity first.
    pub fn chat_ids(&self, id: ProjectId) -> Result<Vec<gantry_core::ChatId>, GantryError> {
        self.store
            .read(move |c| projects::chat_ids(c, id))
            .map_err(store_err)
    }
}

fn cap(text: &str) -> String {
    text.trim()
        .chars()
        .take(PROJECT_INSTRUCTIONS_MAX_CHARS)
        .collect()
}
