//! Active turns: start, cancel, subscribe, list (docs/plan/01 §3, 05 §3). A turn is a detached
//! task; the UI is just a subscriber.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use gantry_core::{
    AgentEventKind, ChatId, ChatSummary, ContentPart, GantryError, Message, ModelRef, ProviderId,
    Settings, TurnId, TurnSnapshot, TurnStatus, Usage, now_ms,
};
use gantry_providers::Provider;
use tokio_util::sync::CancellationToken;

use crate::{
    chats::ChatBook,
    events::{Batcher, EventSink, FanoutSink},
    runner,
    system_prompt::{CORE_VERSION, PromptContext, SystemPromptBuilder},
};

/// Where providers come from; the registry in the app, a mock in tests.
pub trait ProviderSource: Send + Sync {
    fn provider(&self, id: &ProviderId) -> Option<Arc<dyn Provider>>;
}

impl ProviderSource for gantry_providers::ProviderRegistry {
    fn provider(&self, id: &ProviderId) -> Option<Arc<dyn Provider>> {
        self.get(id)
    }
}

/// The live state of a running turn, kept for snapshots.
#[derive(Debug)]
pub struct TurnState {
    pub chat_id: ChatId,
    pub status: TurnStatus,
    pub message_id: Option<gantry_core::MessageId>,
    /// Block index → part, in block order.
    pub parts: std::collections::BTreeMap<u32, ContentPart>,
    pub usage: Option<Usage>,
    pub started_at: i64,
}

pub struct ActiveTurn {
    pub id: TurnId,
    pub chat_id: ChatId,
    pub state: Mutex<TurnState>,
    pub cancel: CancellationToken,
    pub fanout: Arc<FanoutSink>,
    pub batcher: Arc<Batcher>,
}

pub struct TurnManager {
    chats: Arc<ChatBook>,
    providers: Arc<dyn ProviderSource>,
    settings: Arc<RwLock<Settings>>,
    context: PromptContext,
    active: Mutex<HashMap<TurnId, Arc<ActiveTurn>>>,
}

impl TurnManager {
    #[must_use]
    pub fn new(
        chats: Arc<ChatBook>,
        providers: Arc<dyn ProviderSource>,
        settings: Arc<RwLock<Settings>>,
        context: PromptContext,
    ) -> Arc<Self> {
        Arc::new(Self {
            chats,
            providers,
            settings,
            context,
            active: Mutex::new(HashMap::new()),
        })
    }

    #[must_use]
    pub fn chats(&self) -> &Arc<ChatBook> {
        &self.chats
    }

    /// A new chat with the settings' defaults and a freshly assembled system prompt.
    pub fn create_chat(&self, model: Option<ModelRef>) -> ChatSummary {
        let settings = self
            .settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let model = model.unwrap_or_else(|| settings.default_model());
        let prompt = SystemPromptBuilder::new(settings.chat.default_mode, self.context.clone())
            .global_instructions(&settings.chat.custom_instructions)
            .build();
        self.chats.create(
            model,
            settings.chat.default_mode,
            settings.chat.default_guard,
            settings.chat.default_effort,
            prompt,
            CORE_VERSION,
        )
    }

    /// Starts a turn for `text` and returns at once; `sink` receives the batches.
    pub fn start(
        self: &Arc<Self>,
        chat_id: ChatId,
        text: String,
        sink: Arc<dyn EventSink>,
    ) -> Result<TurnId, GantryError> {
        let text = text.trim().to_owned();
        if text.is_empty() {
            return Err(GantryError::invalid("the message is empty"));
        }
        let input = self.chats.begin_turn(chat_id, Message::user_text(text))?;
        let provider = self.providers.provider(&input.model.provider);
        let max_output_tokens = self
            .settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .advanced
            .max_output_tokens;

        let fanout = Arc::new(FanoutSink::new());
        fanout.add(sink);
        let batcher = Batcher::start(input.turn_id, fanout.clone());
        let active = Arc::new(ActiveTurn {
            id: input.turn_id,
            chat_id,
            state: Mutex::new(TurnState {
                chat_id,
                status: TurnStatus::Running,
                message_id: None,
                parts: Default::default(),
                usage: None,
                started_at: now_ms(),
            }),
            cancel: CancellationToken::new(),
            fanout,
            batcher,
        });
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(input.turn_id, active.clone());

        let manager = Arc::clone(self);
        let chats = self.chats.clone();
        tokio::spawn(async move {
            runner::run_turn(input, provider, max_output_tokens, active.clone(), chats).await;
            manager
                .active
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&active.id);
        });
        Ok(input_turn_id(&self.active, chat_id).unwrap_or_default())
    }

    /// Trips the turn's cancellation token. Returns whether the turn was running.
    pub fn cancel(&self, turn_id: TurnId) -> bool {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&turn_id)
            .cloned();
        match active {
            Some(t) => {
                t.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Sends one snapshot to `sink`, then every later batch. Fails when the turn is not
    /// running (a finished turn is read from the chat).
    pub fn subscribe(
        &self,
        turn_id: TurnId,
        since_seq: u64,
        sink: Arc<dyn EventSink>,
    ) -> Result<(), GantryError> {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&turn_id)
            .cloned()
            .ok_or_else(|| GantryError::not_found(format!("turn {turn_id} is not running")))?;
        // Hold the state lock across snapshot and subscription so no event slips between them.
        let state = active.state.lock().unwrap_or_else(|e| e.into_inner());
        let seq = active.batcher.last_seq();
        let snapshot = TurnSnapshot {
            chat_id: state.chat_id,
            status: state.status,
            message_id: state.message_id,
            parts: state.parts.values().cloned().collect(),
            usage: state.usage,
            started_at: state.started_at,
            seq,
        };
        let _ = since_seq;
        sink.emit(gantry_core::AgentEventBatch {
            turn_id,
            events: vec![gantry_core::AgentEvent {
                seq,
                ts: now_ms(),
                turn_id,
                event: AgentEventKind::TurnSnapshot { snapshot },
            }],
        });
        active.fanout.add(sink);
        drop(state);
        Ok(())
    }

    /// `(chat, turn)` of every running turn.
    #[must_use]
    pub fn list_active(&self) -> Vec<(ChatId, TurnId)> {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|t| (t.chat_id, t.id))
            .collect()
    }

    #[must_use]
    pub fn active_turn_for(&self, chat_id: ChatId) -> Option<TurnId> {
        input_turn_id(&self.active, chat_id)
    }
}

fn input_turn_id(
    active: &Mutex<HashMap<TurnId, Arc<ActiveTurn>>>,
    chat_id: ChatId,
) -> Option<TurnId> {
    active
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .find(|t| t.chat_id == chat_id)
        .map(|t| t.id)
}
