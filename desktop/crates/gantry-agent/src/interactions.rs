//! Pending decisions (docs/plan/04 §10): one `oneshot` per interaction the turn waits on. The
//! rows in the store are written by the persister from the `decision.*` events; this is the
//! live side that a command resolves.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use gantry_core::{
    ChatId, GantryError, Interaction, InteractionId, InteractionResolution, InteractionStatus,
    TurnId, now_ms,
};
use tokio::sync::oneshot;

struct Pending {
    interaction: Interaction,
    tx: oneshot::Sender<InteractionResolution>,
}

#[derive(Default)]
pub struct Interactions {
    pending: Mutex<HashMap<InteractionId, Pending>>,
}

impl Interactions {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Registers a pending interaction; the receiver completes when someone resolves it.
    pub fn request(&self, interaction: Interaction) -> oneshot::Receiver<InteractionResolution> {
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(interaction.id, Pending { interaction, tx });
        rx
    }

    /// Completes a pending interaction and returns it resolved. A resolution of the wrong kind
    /// is refused; an unknown id means the turn already moved on.
    pub fn resolve(
        &self,
        id: InteractionId,
        resolution: InteractionResolution,
    ) -> Result<Interaction, GantryError> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let entry = pending.get(&id).ok_or_else(|| {
            GantryError::not_found(format!("interaction {id} is not waiting for a decision"))
        })?;
        let matches = match (&entry.interaction.payload, &resolution) {
            (
                gantry_core::InteractionPayload::Permission { .. },
                InteractionResolution::Permission { .. },
            ) => true,
            (_, InteractionResolution::Cancelled) => true,
        };
        if !matches {
            return Err(GantryError::invalid(
                "the resolution does not match the interaction's kind",
            ));
        }
        let Pending {
            mut interaction,
            tx,
        } = pending.remove(&id).expect("checked above");
        drop(pending);
        interaction.status = match resolution {
            InteractionResolution::Cancelled => InteractionStatus::Cancelled,
            _ => InteractionStatus::Resolved,
        };
        interaction.resolution = Some(resolution.clone());
        interaction.resolved_at = Some(now_ms());
        // A dropped receiver means the turn ended first; the answer is simply late.
        let _ = tx.send(resolution);
        Ok(interaction)
    }

    /// Cancels everything a turn was waiting on; the runner is told through the receivers.
    pub fn cancel_turn(&self, turn_id: TurnId) -> Vec<Interaction> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let ids: Vec<InteractionId> = pending
            .values()
            .filter(|p| p.interaction.turn_id == turn_id)
            .map(|p| p.interaction.id)
            .collect();
        let mut out = Vec::new();
        for id in ids {
            if let Some(Pending {
                mut interaction,
                tx,
            }) = pending.remove(&id)
            {
                interaction.status = InteractionStatus::Cancelled;
                interaction.resolution = Some(InteractionResolution::Cancelled);
                interaction.resolved_at = Some(now_ms());
                let _ = tx.send(InteractionResolution::Cancelled);
                out.push(interaction);
            }
        }
        out
    }

    /// Pending interactions, oldest first, for one chat or every chat.
    #[must_use]
    pub fn list_pending(&self, chat_id: Option<ChatId>) -> Vec<Interaction> {
        let mut out: Vec<Interaction> = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|p| chat_id.is_none_or(|c| p.interaction.chat_id == c))
            .map(|p| p.interaction.clone())
            .collect();
        out.sort_by_key(|i| (i.created_at, i.id));
        out
    }

    #[must_use]
    pub fn pending_count(&self, chat_id: ChatId) -> u32 {
        u32::try_from(self.list_pending(Some(chat_id)).len()).unwrap_or(u32::MAX)
    }
}
