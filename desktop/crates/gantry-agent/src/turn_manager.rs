//! Active turns: start, cancel, subscribe, list, resolve decisions (docs/plan/01 §3, 04 §10,
//! 05 §3). A turn is a detached task; the UI is just a subscriber. Chats live in the store
//! through the [`ChatBook`].

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, RwLock},
};

use gantry_connectors::ConnectorRegistry;
use gantry_core::{
    AgentEventKind, AttachmentInput, ChatId, ChatSummary, ContentPart, GantryError, Interaction,
    InteractionId, InteractionResolution, Message, MessageId, ModelRef, ProviderId, Role, Settings,
    ToolCallDto, TurnId, TurnSnapshot, TurnStatus, Usage, now_ms,
};
use gantry_providers::Provider;
use tokio_util::sync::CancellationToken;

use crate::{
    attachments,
    chats::{ChatBook, ChatPatch, NewAttachment},
    events::{Batcher, EventSink, FanoutSink},
    interactions::Interactions,
    persist::PersistSink,
    runner::{self, RunContext},
    system_prompt::{CORE_VERSION, PromptContext, SystemPromptBuilder, mode_note},
    title,
    tools::ToolSet,
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

/// Told when chats changed outside a command (a turn ended, a title arrived) or when a chat's
/// pending decisions changed, so the app can emit the global events.
pub trait ChatNotifier: Send + Sync {
    fn chats_changed(&self, chat_ids: Vec<ChatId>);
    fn interactions_changed(&self, chat_id: ChatId, pending: u32) {
        let _ = (chat_id, pending);
    }
}

/// One message of a running turn, parts by block index.
#[derive(Debug, Clone)]
pub struct LiveMessage {
    pub id: MessageId,
    pub role: Role,
    pub parts: BTreeMap<u32, ContentPart>,
}

impl LiveMessage {
    #[must_use]
    pub fn finished(m: &Message) -> Self {
        Self {
            id: m.id,
            role: m.role,
            parts: m
                .parts
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, p)| (u32::try_from(i).unwrap_or(u32::MAX), p))
                .collect(),
        }
    }

    #[must_use]
    pub fn to_message(&self) -> Message {
        Message {
            id: self.id,
            role: self.role,
            parts: self.parts.values().cloned().collect(),
            origin: None,
            created_at: 0,
        }
    }
}

/// The live state of a running turn, kept for snapshots.
#[derive(Debug)]
pub struct TurnState {
    pub chat_id: ChatId,
    pub status: TurnStatus,
    pub messages: Vec<LiveMessage>,
    pub tool_calls: Vec<ToolCallDto>,
    pub pending: Vec<Interaction>,
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
    connectors: Arc<ConnectorRegistry>,
    interactions: Arc<Interactions>,
    settings: Arc<RwLock<Settings>>,
    context: PromptContext,
    /// Turns run here whatever thread starts them; commands arrive on the UI thread.
    runtime: tokio::runtime::Handle,
    active: Mutex<HashMap<TurnId, Arc<ActiveTurn>>>,
    notifier: RwLock<Option<Arc<dyn ChatNotifier>>>,
}

impl TurnManager {
    #[must_use]
    pub fn new(
        chats: Arc<ChatBook>,
        providers: Arc<dyn ProviderSource>,
        connectors: Arc<ConnectorRegistry>,
        settings: Arc<RwLock<Settings>>,
        context: PromptContext,
        runtime: tokio::runtime::Handle,
    ) -> Arc<Self> {
        Arc::new(Self {
            chats,
            providers,
            connectors,
            interactions: Interactions::new(),
            settings,
            context,
            runtime,
            active: Mutex::new(HashMap::new()),
            notifier: RwLock::new(None),
        })
    }

    pub fn set_notifier(&self, notifier: Arc<dyn ChatNotifier>) {
        *self.notifier.write().unwrap_or_else(|e| e.into_inner()) = Some(notifier);
    }

    fn notifier(&self) -> Option<Arc<dyn ChatNotifier>> {
        self.notifier
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn notify(&self, chat_id: ChatId) {
        if let Some(n) = self.notifier() {
            n.chats_changed(vec![chat_id]);
        }
    }

    #[must_use]
    pub fn chats(&self) -> &Arc<ChatBook> {
        &self.chats
    }

    #[must_use]
    pub fn connectors(&self) -> &Arc<ConnectorRegistry> {
        &self.connectors
    }

    #[must_use]
    pub fn interactions(&self) -> &Arc<Interactions> {
        &self.interactions
    }

    fn settings(&self) -> Settings {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn build_prompt(&self, settings: &Settings, mode: gantry_core::Mode) -> String {
        SystemPromptBuilder::new(mode, self.context.clone())
            .global_instructions(&settings.chat.custom_instructions)
            .build()
    }

    /// A new chat with the settings' defaults and a freshly assembled system prompt.
    pub fn create_chat(&self, model: Option<ModelRef>) -> Result<ChatSummary, GantryError> {
        let settings = self.settings();
        let model = model.unwrap_or_else(|| settings.default_model());
        let prompt = self.build_prompt(&settings, settings.chat.default_mode);
        self.chats.create(
            model,
            settings.chat.default_mode,
            settings.chat.default_guard,
            settings.chat.default_effort,
            prompt,
            CORE_VERSION,
        )
    }

    /// Applies a patch; a mode change on a chat with turns appends the mode note (04 §3).
    pub fn update_chat(
        &self,
        chat_id: ChatId,
        patch: ChatPatch,
    ) -> Result<ChatSummary, GantryError> {
        let mode = patch.mode;
        let before = self.chats.get(chat_id)?;
        let summary = self.chats.update(chat_id, patch)?;
        if let (Some(mode), Some(before)) = (mode, before)
            && before.mode != mode
        {
            if self.chats.has_turns(chat_id)? {
                self.chats.append_system_note(chat_id, mode_note(mode))?;
            } else {
                let settings = self.settings();
                self.chats.replace_snapshot(
                    chat_id,
                    self.build_prompt(&settings, mode),
                    CORE_VERSION,
                )?;
            }
        }
        Ok(summary)
    }

    /// Settings → Custom instructions changed (10 §4): chats without turns get a new snapshot,
    /// the others a `SystemNote` carrying the whole new layer.
    pub fn global_instructions_changed(&self) -> Result<(), GantryError> {
        let settings = self.settings();
        let text = settings.chat.custom_instructions.trim().to_owned();
        let note = if text.is_empty() {
            "The user removed their global instructions; earlier <instructions scope=\"global\"> no longer apply.".to_owned()
        } else {
            format!(
                "Updated global instructions (replacing any earlier ones):\n<instructions scope=\"global\">\n{text}\n</instructions>"
            )
        };
        for id in self.chats.open_chat_ids()? {
            if self.chats.has_turns(id)? {
                self.chats.append_system_note(id, note.clone())?;
            } else if let Some(chat) = self.chats.get(id)? {
                self.chats.replace_snapshot(
                    id,
                    self.build_prompt(&settings, chat.mode),
                    CORE_VERSION,
                )?;
            }
        }
        Ok(())
    }

    /// Starts a turn for `text` with `attachments` and returns at once; `sink` receives the
    /// batches.
    pub fn start(
        self: &Arc<Self>,
        chat_id: ChatId,
        text: String,
        attachments: Vec<AttachmentInput>,
        sink: Arc<dyn EventSink>,
    ) -> Result<TurnId, GantryError> {
        let text = text.trim().to_owned();
        if text.is_empty() && attachments.is_empty() {
            return Err(GantryError::invalid("the message is empty"));
        }
        let ingested = attachments::ingest(self.chats.blobs(), attachments)?;
        let mut user = Message::user_text(text);
        if user.text().is_empty() {
            user.parts.clear();
        }
        let mut records = Vec::with_capacity(ingested.len());
        for i in ingested {
            user.parts.push(i.part);
            records.push(i.record);
        }
        self.start_message(chat_id, user, records, sink)
    }

    /// Re-runs the chat's last turn: the old turn is dropped and its user message sent again.
    pub fn retry(
        self: &Arc<Self>,
        chat_id: ChatId,
        turn_id: TurnId,
        sink: Arc<dyn EventSink>,
    ) -> Result<TurnId, GantryError> {
        let (user, attachments) = self.chats.take_last_turn(chat_id, turn_id)?;
        self.start_message(chat_id, user, attachments, sink)
    }

    fn start_message(
        self: &Arc<Self>,
        chat_id: ChatId,
        user: Message,
        attachments: Vec<NewAttachment>,
        sink: Arc<dyn EventSink>,
    ) -> Result<TurnId, GantryError> {
        let input = self.chats.begin_turn(chat_id, user, attachments)?;
        let turn_id = input.turn_id;
        let provider = self.providers.provider(&input.model.provider);
        let settings = self.settings();

        let fanout = Arc::new(FanoutSink::new());
        fanout.add(Arc::new(PersistSink::new(
            self.chats.store().clone(),
            chat_id,
        )));
        fanout.add(sink);
        let batcher = Batcher::start(turn_id, fanout.clone(), &self.runtime);
        let active = Arc::new(ActiveTurn {
            id: turn_id,
            chat_id,
            state: Mutex::new(TurnState {
                chat_id,
                status: TurnStatus::Running,
                messages: Vec::new(),
                tool_calls: Vec::new(),
                pending: Vec::new(),
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
            .insert(turn_id, active.clone());

        let manager = Arc::clone(self);
        let chats = self.chats.clone();
        let connectors = self.connectors.clone();
        let interactions = self.interactions.clone();
        let notifier = self.notifier();
        let first_turn = input.first_turn;
        let user_text = input.messages.last().map(Message::text).unwrap_or_default();
        let model = input.model.clone();
        let mode = input.mode;
        self.runtime.spawn(async move {
            let tools = ToolSet::assemble(&connectors, mode).await;
            runner::run_turn(RunContext {
                input,
                provider: provider.clone(),
                max_output_tokens: settings.advanced.max_output_tokens,
                max_tool_rounds: settings.advanced.max_tool_rounds,
                active: active.clone(),
                chats: chats.clone(),
                tools,
                interactions,
                notifier,
            })
            .await;
            manager
                .active
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&active.id);
            manager.notify(chat_id);
            if first_turn && let Some(provider) = provider {
                manager
                    .name_chat(chat_id, turn_id, provider, &model, &user_text)
                    .await;
            }
        });
        Ok(turn_id)
    }

    /// Names the chat from its first exchange (01 §3 step 7); failures only log.
    async fn name_chat(
        &self,
        chat_id: ChatId,
        turn_id: TurnId,
        provider: Arc<dyn Provider>,
        model: &ModelRef,
        user_text: &str,
    ) {
        let assistant_text = match self.chats.get(chat_id) {
            Ok(Some(detail)) => detail
                .turns
                .iter()
                .find(|t| t.id == turn_id)
                .filter(|t| t.status == TurnStatus::Completed)
                .map(|t| t.assistant_text()),
            _ => None,
        };
        let Some(assistant_text) = assistant_text.filter(|t| !t.trim().is_empty()) else {
            return;
        };
        let judge = title::judge_model(model.provider.as_str(), provider.kind(), &model.model);
        match title::generate_title(provider, judge, user_text, &assistant_text).await {
            Ok(t) if !t.is_empty() => match self.chats.set_auto_title(chat_id, t) {
                Ok(true) => self.notify(chat_id),
                Ok(false) => {}
                Err(err) => log::warn!("could not store the title of {chat_id}: {err}"),
            },
            Ok(_) => log::warn!("the title generator returned nothing for {chat_id}"),
            Err(err) => log::warn!("title generation failed for {chat_id}: {err}"),
        }
    }

    /// Trips the turn's cancellation token; pending prompts resolve as cancelled through it.
    /// Returns whether the turn was running.
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

    /// Answers a pending decision; the waiting turn continues (04 §10).
    pub fn resolve_interaction(
        &self,
        id: InteractionId,
        resolution: InteractionResolution,
    ) -> Result<Interaction, GantryError> {
        self.interactions.resolve(id, resolution)
    }

    /// Sends one snapshot to `sink`, then every later batch. Fails when the turn is not
    /// running (a finished turn is read from the chat). The snapshot carries every message,
    /// tool call and pending decision, so `since_seq` only marks where live events resume.
    pub fn subscribe(
        &self,
        turn_id: TurnId,
        since_seq: u32,
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
            messages: state.messages.iter().map(LiveMessage::to_message).collect(),
            tool_calls: state.tool_calls.clone(),
            pending: state.pending.clone(),
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
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .find(|t| t.chat_id == chat_id)
            .map(|t| t.id)
    }
}
