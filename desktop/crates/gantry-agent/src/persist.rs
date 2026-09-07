//! The persister (docs/plan/05 §3): the persisted subset of every batch goes to the store's
//! writer in one transaction per flush, off the UI path, and the `tool_calls` and
//! `interactions` projections are updated in the same write. Deltas and snapshots stay
//! transient.

use std::sync::Arc;

use gantry_core::{AgentEventBatch, AgentEventKind, ChatId, EventId};
use gantry_store::{
    Store,
    repos::{events::EventRecord, projections},
};

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
    kind.is_persisted()
}

/// The id an event refers to, for the `ref_id` column.
fn ref_id(kind: &AgentEventKind) -> Option<String> {
    match kind {
        AgentEventKind::ToolCallStarted { call_id, .. }
        | AgentEventKind::ToolCallReady { call_id, .. }
        | AgentEventKind::ToolCallExecuting { call_id, .. }
        | AgentEventKind::ToolCallCompleted { call_id, .. } => Some(call_id.to_string()),
        AgentEventKind::DecisionRequested { interaction } => Some(interaction.id.to_string()),
        AgentEventKind::DecisionResolved { interaction_id, .. } => Some(interaction_id.to_string()),
        _ => None,
    }
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
                ref_id: ref_id(&event.event),
                event,
            })
            .collect();
        if records.is_empty() {
            return;
        }
        let chat_id = self.chat_id;
        self.store.write_detached(move |conn| {
            gantry_store::repos::events::insert_batch(conn, &records)?;
            for r in &records {
                projections::apply(conn, chat_id, &r.event)?;
            }
            Ok(())
        });
    }
}
