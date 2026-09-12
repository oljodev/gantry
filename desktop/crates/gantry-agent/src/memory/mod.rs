//! Memory (docs/plan/12 §B): the store, and the two selections that spend it.
//!
//! Everything here obeys one rule from §B1: **nothing reaches a prompt that is not a row on the
//! Memory page**. So there is no hidden cache, no derived summary and no second store — the
//! selector reads the same table the page lists, and the `context.injected` event names exactly
//! what it took.

pub mod selector;

use std::sync::Arc;

use gantry_core::{
    ChatId, GantryError, MemoryDto, MemoryId, MemoryInput, MemoryKind, MemoryScopeKind,
    MemorySource, MessageId, ProjectId, memory, now_ms,
};
use gantry_store::{
    Store,
    repos::{self, memories::MemoryFilter},
};

pub struct Memories {
    store: Arc<Store>,
}

impl Memories {
    #[must_use]
    pub fn new(store: Arc<Store>) -> Arc<Self> {
        Arc::new(Self { store })
    }

    pub fn list(&self, filter: MemoryFilter, query: &str) -> Result<Vec<MemoryDto>, GantryError> {
        let query = query.to_owned();
        Ok(self
            .store
            .read(move |c| repos::memories::list(c, filter, &query))?)
    }

    pub fn get(&self, id: MemoryId) -> Result<Option<MemoryDto>, GantryError> {
        Ok(self.store.read(move |c| repos::memories::get(c, id))?)
    }

    /// Writes one entry. Every path that creates a memory — the page, `/remember`, **Remember
    /// this**, a card the user saved — comes through here, so the length rule and the
    /// provenance are in one place.
    pub fn create(
        &self,
        text: &str,
        kind: MemoryKind,
        scope_kind: MemoryScopeKind,
        scope_id: Option<ProjectId>,
        source: MemorySource,
        origin: Option<(ChatId, Option<MessageId>)>,
    ) -> Result<MemoryDto, GantryError> {
        let text = text.trim().to_owned();
        if let Some(problem) = memory::text_problem(&text) {
            return Err(GantryError::invalid(problem));
        }
        let now = now_ms();
        let entry = MemoryDto {
            id: MemoryId::new(),
            scope_kind,
            scope_id: match scope_kind {
                MemoryScopeKind::Project => scope_id,
                MemoryScopeKind::Global => None,
            },
            kind,
            text,
            always_include: false,
            source,
            origin_chat_id: origin.map(|(c, _)| c),
            origin_message_id: origin.and_then(|(_, m)| m),
            tags: Vec::new(),
            enabled: true,
            use_count: 0,
            last_used_at: None,
            created_at: now,
            updated_at: now,
            archived_at: None,
        };
        let written = entry.clone();
        self.store
            .write_blocking(move |c| repos::memories::insert(c, &entry))?;
        Ok(written)
    }

    /// Edits an entry in place. The page allows this for a memory of either source: the user
    /// owns every row, whoever proposed it.
    pub fn update(&self, id: MemoryId, patch: MemoryInput) -> Result<MemoryDto, GantryError> {
        let mut entry = self
            .get(id)?
            .ok_or_else(|| GantryError::not_found(format!("memory {id}")))?;
        if let Some(text) = patch.text {
            let text = text.trim().to_owned();
            if let Some(problem) = memory::text_problem(&text) {
                return Err(GantryError::invalid(problem));
            }
            entry.text = text;
        }
        if let Some(kind) = patch.kind {
            entry.kind = kind;
        }
        if let Some(scope) = patch.scope_kind {
            entry.scope_kind = scope;
            entry.scope_id = match scope {
                MemoryScopeKind::Project => patch.scope_id.or(entry.scope_id),
                MemoryScopeKind::Global => None,
            };
        }
        if let Some(always) = patch.always_include {
            entry.always_include = always;
        }
        if let Some(enabled) = patch.enabled {
            entry.enabled = enabled;
        }
        if let Some(tags) = patch.tags {
            entry.tags = tags;
        }
        entry.updated_at = now_ms();
        let written = entry.clone();
        self.store
            .write_blocking(move |c| repos::memories::update(c, &entry))?;
        Ok(written)
    }

    /// Deletes into Recently deleted, where it stays restorable for thirty days (12 §B5).
    pub fn archive(&self, id: MemoryId) -> Result<(), GantryError> {
        self.store
            .write_blocking(move |c| repos::memories::archive(c, id))?;
        Ok(())
    }

    pub fn restore(&self, id: MemoryId) -> Result<(), GantryError> {
        self.store
            .write_blocking(move |c| repos::memories::restore(c, id))?;
        Ok(())
    }

    /// Removes one entry for good, and sweeps everything whose thirty days are up.
    pub fn forget_for_good(&self, id: MemoryId) -> Result<(), GantryError> {
        self.store
            .write_blocking(move |c| repos::memories::delete_now(c, id))?;
        Ok(())
    }

    /// Run at startup: Recently deleted is thirty days, not forever (12 §B5).
    pub fn sweep(&self) -> Result<usize, GantryError> {
        let cutoff = now_ms() - memory::RECENTLY_DELETED_DAYS * 24 * 60 * 60 * 1000;
        Ok(self
            .store
            .write_blocking(move |c| repos::memories::purge(c, cutoff))?)
    }

    /// Counted when entries actually reached a prompt, which is what the page's "used" column
    /// means.
    pub fn mark_used(&self, ids: &[MemoryId]) {
        if ids.is_empty() {
            return;
        }
        let ids = ids.to_vec();
        if let Err(err) = self
            .store
            .write_blocking(move |c| repos::memories::mark_used(c, &ids))
        {
            log::warn!("could not record which memories were used: {err}");
        }
    }

    /// What `gantry__search_memory` answers: entries in scope that match, whatever their kind.
    pub fn search(
        &self,
        project: Option<ProjectId>,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MemoryDto>, GantryError> {
        let query = query.to_owned();
        let all = self
            .store
            .read(move |c| repos::memories::list(c, MemoryFilter::default(), &query))?;
        Ok(all
            .into_iter()
            .filter(|m| m.enabled)
            .filter(|m| m.scope_kind == MemoryScopeKind::Global || m.scope_id == project)
            .take(limit)
            .collect())
    }
}
