//! The persister (docs/plan/05 §3): the persisted subset of every batch goes to the store's
//! writer in one transaction per flush, off the UI path. Deltas and snapshots stay transient.

use std::sync::Arc;

use gantry_core::{AgentEventBatch, AgentEventKind, ChatId, EventId};
use gantry_store::{Store, repos::events::EventRecord};

use crate::events::EventSink;

pub struct PersistSink {
    store: Arc<Store>,
    chat_id: ChatId,
}

impl PersistSink {
    #[must_use]
    pub fn new(store: Arc<Store>, chat_id: ChatId) -> Self {
        Self { store, chat_id }
    }
}

/// Whether an event kind is part of the activity log (05 §2, the "persisted" column).
#[must_use]
pub fn is_persisted(kind: &AgentEventKind) -> bool {
    matches!(
        kind,
        AgentEventKind::TurnStarted { .. }
            | AgentEventKind::MessageStarted { .. }
            | AgentEventKind::ProviderNotice { .. }
            | AgentEventKind::MessageCompleted { .. }
            | AgentEventKind::TurnCompleted { .. }
            | AgentEventKind::Error { .. }
    )
}

impl EventSink for PersistSink {
    fn emit(&self, batch: AgentEventBatch) {
        let records: Vec<EventRecord> = batch
            .events
            .into_iter()
            .filter(|e| is_persisted(&e.event))
            .map(|event| EventRecord {
                id: EventId::new(),
                chat_id: self.chat_id,
                event,
                ref_id: None,
            })
            .collect();
        if records.is_empty() {
            return;
        }
        self.store
            .write_detached(move |conn| gantry_store::repos::events::insert_batch(conn, &records));
    }
}
