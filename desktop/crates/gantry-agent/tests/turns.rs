//! The turn loop on a scripted provider and a fake connector: text turns, cancel, errors,
//! snapshots, and the M3 tool loop with its permission prompts.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use gantry_agent::{
    ChatBook, ChatNotifier, ChatPatch, EventSink, PromptContext, ProviderSource, TurnManager,
};
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ConnectorRegistry, ToolCallRequest,
    ToolEventSink, ToolOutcome,
};
use gantry_core::{
    AgentEventBatch, AgentEventKind, CallId, ChatId, ContentPart, DecisionSource,
    InteractionResolution, InteractionStatus, Mode, ModelRef, PermissionDecision,
    ProviderErrorKind, ProviderId, ProviderKind, RiskTier, Role, Settings, StopReason,
    ToolCallStatus, ToolDef, TurnStatus, Usage,
};
use gantry_providers::{
    ChatRequest, ChatStream, KeyInfo, ModelInfo, Provider, ProviderError, StreamEvent,
};
use gantry_store::{BlobStore, Store};
use tokio_util::sync::CancellationToken;

type Script = Vec<Result<StreamEvent, ProviderError>>;

struct Scripted {
    id: ProviderId,
    /// One script per request, in order; an exhausted provider answers "done".
    rounds: Mutex<VecDeque<Script>>,
    delay: Duration,
    /// Requests seen, newest last (the title generator sends one more).
    requests: Mutex<Vec<ChatRequest>>,
    /// What this provider says the model's window is, for the context budget (02 §6).
    window: Option<u32>,
    /// What the guard answers, in order (04 §6); when it runs out it allows. Each entry is the
    /// model's whole reply, so a test can also script an unreadable one.
    guard: Mutex<VecDeque<String>>,
    /// What the guard was asked about, newest last.
    guard_inputs: Mutex<Vec<String>>,
}

#[async_trait]
impl Provider for Scripted {
    fn id(&self) -> &ProviderId {
        &self.id
    }
    fn kind(&self) -> ProviderKind {
        ProviderKind::OpenAiChat
    }
    fn has_key(&self) -> bool {
        true
    }
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }
    fn model_info(&self, model: &str) -> Option<ModelInfo> {
        let window = self.window?;
        Some(ModelInfo {
            id: model.to_owned(),
            display_name: model.to_owned(),
            created_at: None,
            context_window: Some(window),
            max_output: None,
            pricing: None,
            capabilities: gantry_providers::ModelCapabilities::default(),
        })
    }
    async fn check_key(&self) -> Result<KeyInfo, ProviderError> {
        Ok(KeyInfo::default())
    }
    async fn stream(&self, req: ChatRequest) -> Result<ChatStream, ProviderError> {
        let title_request = req.system.starts_with("You name conversations");
        let summary_request = req.system.starts_with("You are summarizing");
        let guard_request = req.system.starts_with("You are the guard in Gantry");
        if guard_request {
            self.guard_inputs.lock().unwrap().push(
                req.messages
                    .last()
                    .map(gantry_core::Message::text)
                    .unwrap_or_default(),
            );
        }
        self.requests.lock().unwrap().push(req);
        let delay = self.delay;
        let events = if guard_request {
            let reply = self.guard.lock().unwrap().pop_front().unwrap_or_else(|| {
                r#"{"decision":"allow","confidence":0.9,"reason":"Part of the task"}"#.into()
            });
            vec![
                Ok(StreamEvent::TextDelta {
                    index: 0,
                    text: reply,
                }),
                Ok(StreamEvent::MessageEnd {
                    stop_reason: StopReason::EndTurn,
                }),
            ]
        } else if title_request || summary_request {
            let reply = if title_request {
                "\"A generated title.\""
            } else {
                "**Goal.** Ship the thing."
            };
            vec![
                Ok(StreamEvent::TextDelta {
                    index: 0,
                    text: reply.into(),
                }),
                Ok(StreamEvent::MessageEnd {
                    stop_reason: StopReason::EndTurn,
                }),
            ]
        } else {
            self.rounds
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| vec![text("done"), end()])
        };
        let s = futures_util::stream::iter(events).then(move |e| async move {
            tokio::time::sleep(delay).await;
            e
        });
        Ok(Box::pin(s))
    }
}

struct Source(Arc<dyn Provider>);

impl ProviderSource for Source {
    fn provider(&self, _id: &ProviderId) -> Option<Arc<dyn Provider>> {
        Some(self.0.clone())
    }
}

/// A connector with a read tool, a write tool and one that fails.
struct Fake {
    descriptor: ConnectorDescriptor,
    calls: Mutex<Vec<ToolCallRequest>>,
    /// Released to let `stream` finish; unset means it returns at once.
    hold: Mutex<Option<Arc<tokio::sync::Notify>>>,
}

#[async_trait]
impl Connector for Fake {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }
    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        let mut echo = ToolDef::new(
            "echo",
            "Echoes text",
            serde_json::json!({ "type": "object", "properties": { "text": { "type": "string" } } }),
            RiskTier::Read,
        );
        echo.parallel_safe = true;
        Ok(vec![
            echo,
            ToolDef::new("write", "Writes", serde_json::json!({}), RiskTier::Write),
            ToolDef::new("boom", "Fails", serde_json::json!({}), RiskTier::Read),
            ToolDef::new(
                "crash",
                "Writes, badly",
                serde_json::json!({}),
                RiskTier::Write,
            ),
            ToolDef::new(
                "stream",
                "Prints as it goes",
                serde_json::json!({}),
                RiskTier::Read,
            ),
            ToolDef::new(
                "paint",
                "Makes a picture",
                serde_json::json!({}),
                RiskTier::Read,
            ),
        ])
    }
    async fn call(
        &self,
        req: ToolCallRequest,
        _sink: Arc<dyn ToolEventSink>,
        _cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        self.calls.lock().unwrap().push(req.clone());
        match req.tool.as_str() {
            "echo" => Ok(ToolOutcome::json(
                serde_json::json!({ "echo": req.args["text"] }),
            )),
            "write" => Ok(ToolOutcome::text("written")),
            // What the `media` connector does: a result the model reads, and a part for the
            // answer itself (03 §4).
            "paint" => Ok(ToolOutcome::text("A red square, 1 px.").with_media(vec![
                gantry_core::ContentPart::Image {
                    source: gantry_core::MediaSource::Base64 {
                        data: ONE_PIXEL.to_owned(),
                    },
                    mime: "image/png".to_owned(),
                },
            ])),
            "boom" | "crash" => Err(ConnectorError::Failed("kaboom".into())),
            "stream" => {
                for line in ["compiling gantry-core\n", "compiling gantry-agent\n"] {
                    _sink.output(
                        &req.call_id,
                        gantry_connectors::OutputStream::Stdout,
                        line.as_bytes(),
                    );
                }
                let hold = self.hold.lock().unwrap().clone();
                if let Some(hold) = hold {
                    hold.notified().await;
                }
                Ok(ToolOutcome::text("done"))
            }
            other => Err(ConnectorError::UnknownTool(other.into())),
        }
    }

    /// What `media` does for its model (04 §7): an argument with a small set of equivalent
    /// answers, resolved, for the card to offer.
    async fn choices(&self, req: &ToolCallRequest) -> Vec<gantry_core::ArgChoice> {
        if req.tool != "write" {
            return Vec::new();
        }
        vec![gantry_core::ArgChoice {
            key: "target".to_owned(),
            label: "Target".to_owned(),
            value: Some("draft".to_owned()),
            options: ["draft", "final"]
                .into_iter()
                .map(|value| gantry_core::ChoiceOption {
                    value: value.to_owned(),
                    label: value.to_owned(),
                    detail: None,
                })
                .collect(),
            note: None,
        }]
    }
}

#[derive(Default)]
struct Collect(Mutex<Vec<AgentEventBatch>>);

impl EventSink for Collect {
    fn emit(&self, batch: AgentEventBatch) {
        self.0.lock().unwrap().push(batch);
    }
}

impl Collect {
    fn kinds(&self) -> Vec<AgentEventKind> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .flat_map(|b| b.events.iter().map(|e| e.event.clone()))
            .collect()
    }
    fn names(&self) -> Vec<&'static str> {
        self.kinds().iter().map(AgentEventKind::name).collect()
    }
    fn text(&self) -> String {
        self.kinds()
            .iter()
            .filter_map(|k| match k {
                AgentEventKind::TextDelta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
    fn completed(&self) -> Option<TurnStatus> {
        self.kinds().iter().find_map(|k| match k {
            AgentEventKind::TurnCompleted { status, .. } => Some(*status),
            _ => None,
        })
    }
}

#[derive(Default)]
struct Notes {
    chats: Mutex<Vec<ChatId>>,
    pending: Mutex<Vec<(ChatId, u32)>>,
}

impl ChatNotifier for Notes {
    fn chats_changed(&self, ids: Vec<ChatId>) {
        self.chats.lock().unwrap().extend(ids);
    }
    fn interactions_changed(&self, chat_id: ChatId, pending: u32) {
        self.pending.lock().unwrap().push((chat_id, pending));
    }
}

fn book() -> (tempfile::TempDir, Arc<ChatBook>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());
    (dir, Arc::new(ChatBook::new(store, blobs)))
}

struct Harness {
    _dir: tempfile::TempDir,
    m: Arc<TurnManager>,
    provider: Arc<Scripted>,
    fake: Arc<Fake>,
    notes: Arc<Notes>,
    registry: Arc<ConnectorRegistry>,
    settings: Arc<RwLock<Settings>>,
}

impl std::ops::Deref for Harness {
    type Target = Arc<TurnManager>;
    fn deref(&self) -> &Arc<TurnManager> {
        &self.m
    }
}

impl Harness {
    fn requests(&self) -> Vec<ChatRequest> {
        self.provider.requests.lock().unwrap().clone()
    }

    /// Guarded Auto, which is what the guard of 04 §6 decides in.
    fn guarded(&self, chat: ChatId) {
        self.m
            .update_chat(
                chat,
                ChatPatch {
                    mode: Some(Mode::Auto),
                    guard: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
    }

    fn guard_says(&self, replies: &[&str]) {
        *self.provider.guard.lock().unwrap() = replies.iter().map(|r| (*r).to_owned()).collect();
    }

    fn guard_inputs(&self) -> Vec<String> {
        self.provider.guard_inputs.lock().unwrap().clone()
    }

    /// The tail the snapshot would carry for a turn, or `None` when the turn is over: the
    /// state is gone with it, which is itself the answer a finished call gives.
    fn snapshot_output(&self, turn: gantry_core::TurnId) -> Option<Vec<String>> {
        let sink = Arc::new(Collect::default());
        self.m.subscribe(turn, 0, sink.clone()).ok()?;
        match sink.kinds().first()? {
            AgentEventKind::TurnSnapshot { snapshot } => {
                Some(snapshot.output.values().flatten().cloned().collect())
            }
            _ => None,
        }
    }

    /// A chat with the fake connector installed and attached, which is what a chat looks like
    /// once the user has added a connector to it (03 §11). A chat with nothing attached sees
    /// only the runtime tools, which is what `a_chat_sees_only_what_it_attached` checks.
    fn chat(&self) -> gantry_core::ChatSummary {
        let chat = self.m.create_chat(None).unwrap();
        attach_fake(self.m.chats().store(), chat.id);
        chat
    }
}

/// Installs the fake connector as an instance, without attaching it to anything.
fn install_fake(store: &Arc<gantry_store::Store>) -> gantry_core::InstanceId {
    use gantry_store::repos::connectors::{self, NewInstance};
    let id = gantry_core::InstanceId::new();
    store
        .write_blocking(move |c| {
            if let Some(existing) = connectors::list(c)?.iter().find(|i| i.namespace == "fake") {
                return Ok(existing.id);
            }
            connectors::insert(
                c,
                &NewInstance {
                    id,
                    catalog_id: None,
                    namespace: "fake".into(),
                    display_name: "Fake".into(),
                    config: gantry_core::ConnectorConfig::Native,
                    auth: gantry_core::AuthType::None,
                    auth_state: gantry_core::AuthState::Authorized,
                },
            )?;
            Ok(id)
        })
        .unwrap()
}

/// Installs the fake connector as an instance and attaches it to one chat.
fn attach_fake(store: &Arc<gantry_store::Store>, chat_id: gantry_core::ChatId) {
    use gantry_store::repos::connectors;
    let id = install_fake(store);
    store
        .write_blocking(move |c| connectors::attach(c, chat_id, id, "user"))
        .unwrap();
}

fn manager_with(rounds: Vec<Script>, delay: Duration, settings: Settings) -> Harness {
    manager_windowed(rounds, delay, settings, None)
}

fn manager_windowed(
    rounds: Vec<Script>,
    delay: Duration,
    settings: Settings,
    window: Option<u32>,
) -> Harness {
    let provider = Arc::new(Scripted {
        id: ProviderId::openrouter(),
        rounds: Mutex::new(rounds.into()),
        delay,
        requests: Mutex::new(Vec::new()),
        window,
        guard: Mutex::new(VecDeque::new()),
        guard_inputs: Mutex::new(Vec::new()),
    });
    let fake = Arc::new(Fake {
        descriptor: ConnectorDescriptor {
            id: "fake".into(),
            name: "Fake".into(),
            instance_id: None,
            first_party: true,
        },
        calls: Mutex::new(Vec::new()),
        hold: Mutex::new(None),
    });
    let registry = Arc::new(ConnectorRegistry::new());
    registry.register(fake.clone());
    let (dir, chats) = book();
    // Gantry ships with no default model — the composer asks the user to pick one (11 §1) — so
    // every harness picks for them, or `create_chat(None)` would rightly refuse.
    let mut settings = settings;
    settings.chat.default_model = settings
        .chat
        .default_model
        .or_else(|| Some(ModelRef::new(ProviderId::openrouter(), "test/model")));
    let settings = Arc::new(RwLock::new(settings));
    let kept = settings.clone();
    let m = TurnManager::new(
        chats,
        Arc::new(Source(provider.clone())),
        registry.clone(),
        settings.clone(),
        PromptContext::default(),
        tokio::runtime::Handle::current(),
    );
    // As the app does (startup.rs): the connector tools ask through the turn manager's own
    // interaction registry, so they are registered once it exists.
    registry.register(Arc::new(gantry_agent::RuntimeTools::new().with_connectors(
        gantry_agent::ConnectorAccess::new(
            m.chats().store().clone(),
            registry.clone(),
            m.interactions().clone(),
            settings,
        ),
    )));
    let notes = Arc::new(Notes::default());
    m.set_notifier(notes.clone());
    Harness {
        _dir: dir,
        m,
        provider,
        fake,
        notes,
        registry,
        settings: kept,
    }
}

/// The sub-agent connector, registered and attached to one chat, the way startup and the Code
/// surface do it (18 §1). Its instance row is what `NewChat`'s namespace list and the composer's
/// checkbox both go through, so the test writes one.
fn with_sub_agents(h: &Harness, chat: ChatId) {
    use gantry_store::repos::connectors::{NewInstance, attach, insert};
    // Startup fills the library with whatever built-in is missing (18 §3); without this there
    // is nothing for the model to name.
    gantry_agent::subagents::seed(h.chats().store());
    let id = gantry_core::InstanceId::new();
    h.chats()
        .store()
        .write_blocking(move |c| {
            insert(
                c,
                &NewInstance {
                    id,
                    catalog_id: Some("subagents".to_owned()),
                    namespace: "subagents".to_owned(),
                    display_name: "Sub agents".to_owned(),
                    config: gantry_core::ConnectorConfig::Native,
                    auth: gantry_core::AuthType::None,
                    auth_state: gantry_core::AuthState::Authorized,
                },
            )?;
            attach(c, chat, id, "test")
        })
        .unwrap();
    h.registry
        .register(Arc::new(gantry_agent::subagents::SubAgents::new(
            "subagents".to_owned(),
            id,
            Arc::new(RwLock::new(Arc::downgrade(&h.m))),
            h.chats().store().clone(),
            h.settings.clone(),
        )));
}

fn manager(events: Script, delay: Duration) -> Harness {
    manager_with(vec![events], delay, Settings::default())
}

/// Text lands in block 1; block 0 is where a reasoning model puts its thinking.
fn text(t: &str) -> Result<StreamEvent, ProviderError> {
    Ok(StreamEvent::TextDelta {
        index: 1,
        text: t.into(),
    })
}

fn end() -> Result<StreamEvent, ProviderError> {
    Ok(StreamEvent::MessageEnd {
        stop_reason: StopReason::EndTurn,
    })
}

/// A round in which the model calls `name` with `args` and stops for the result.
fn tool_round(id: &str, name: &str, args: serde_json::Value) -> Script {
    vec![
        text("Let me check. "),
        Ok(StreamEvent::ToolCallStart {
            index: 2,
            id: CallId(id.into()),
            name: name.into(),
        }),
        Ok(StreamEvent::ToolCallArgsDelta {
            index: 2,
            json_fragment: args.to_string(),
        }),
        Ok(StreamEvent::ToolCallEnd { index: 2, args }),
        Ok(StreamEvent::MessageEnd {
            stop_reason: StopReason::ToolUse,
        }),
    ]
}

async fn wait_for<F: Fn() -> bool>(f: F) {
    for _ in 0..300 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not met in time");
}

fn manual(m: &Harness, chat: ChatId) {
    m.update_chat(
        chat,
        ChatPatch {
            mode: Some(Mode::Manual),
            ..Default::default()
        },
    )
    .unwrap();
}

#[tokio::test]
async fn a_text_turn_completes_and_is_recorded() {
    let m = manager(
        vec![
            Ok(StreamEvent::MessageStart {
                provider_message_id: None,
            }),
            Ok(StreamEvent::ThinkingDelta {
                index: 0,
                text: "hmm".into(),
            }),
            text("Hel"),
            text("lo"),
            Ok(StreamEvent::Usage(Usage {
                input: 5,
                output: 2,
                ..Default::default()
            })),
            end(),
        ],
        Duration::ZERO,
    );
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    let turn = m
        .start(
            chat.id,
            "Hi there".into(),
            Vec::new(),
            Vec::new(),
            sink.clone(),
        )
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
    assert_eq!(sink.text(), "Hello");
    // The request declared the registered tools.
    let first = m.requests()[0].clone();
    let names: Vec<String> = first.tools.iter().map(|t| t.name.clone()).collect();
    assert_eq!(
        names,
        [
            "fake__echo",
            "fake__write",
            "fake__boom",
            "fake__crash",
            "fake__stream",
            "fake__paint",
            "gantry__clock",
            "gantry__search_connectors",
            "gantry__request_access",
            "gantry__suggest_connector",
        ]
    );
    // The first exchange names the chat with a second, tiny request.
    wait_for(|| m.requests().len() == 2).await;
    wait_for(|| m.notes.chats.lock().unwrap().len() >= 2).await;
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(detail.title, "A generated title");
    let title_req = m.requests()[1].clone();
    assert_eq!(title_req.model, "deepseek/deepseek-v4-flash");
    assert!(title_req.messages[0].text().contains("Hi there"));
    let persisted = m
        .chats()
        .store()
        .read(|c| gantry_store::repos::events::list_for_turn(c, turn))
        .unwrap();
    let kinds: Vec<&str> = persisted.iter().map(|e| e.event.name()).collect();
    assert_eq!(
        kinds,
        [
            "turn.started",
            "message.started",
            "message.completed",
            "turn.completed"
        ]
    );
    let t = &detail.turns[0];
    assert_eq!(t.id, turn);
    assert_eq!(t.status, TurnStatus::Completed);
    assert_eq!(t.usage.unwrap().output, 2);
    assert_eq!(t.messages.len(), 1);
    let a = &t.messages[0];
    assert!(matches!(&a.parts[0], ContentPart::Thinking { text, .. } if text == "hmm"));
    assert!(matches!(&a.parts[1], ContentPart::Text { text } if text == "Hello"));
    assert_eq!(t.assistant_text(), "Hello");
    assert!(m.list_active().is_empty());
}

/// 11 §1: Gantry proposes no model of its own. A chat created before anybody has picked one is
/// refused rather than quietly opened against a model — and a provider — the user never chose.
#[tokio::test]
async fn a_chat_needs_a_model_somebody_picked() {
    let h = manager_with(Vec::new(), Duration::ZERO, Settings::default());
    h.settings.write().unwrap().chat.default_model = None;
    let refused = h.m.create_chat(None);
    assert!(
        matches!(&refused, Err(e) if e.to_string().contains("no model is selected")),
        "expected a refusal, got {refused:?}"
    );
    // Naming one at the call site is enough; it does not have to be the setting.
    let named = ModelRef::new(ProviderId::openrouter(), "test/model");
    let chat = h.m.create_chat(Some(named.clone())).unwrap();
    assert_eq!(h.m.chats().get(chat.id).unwrap().unwrap().model, named);
}

/// 04 §6: one setting decides which model is called on the user's behalf, and it covers the
/// title generator too — a model nobody chose must not appear in their provider's log just
/// because a chat needed naming.
#[tokio::test]
async fn the_utility_model_setting_names_the_chat() {
    let mut settings = Settings::default();
    settings.guard.judge_model = Some(ModelRef::new(
        ProviderId::new("anthropic"),
        "claude-haiku-4-5",
    ));
    let m = manager_with(vec![vec![text("Hello"), end()]], Duration::ZERO, settings);
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "Hi there".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    wait_for(|| m.requests().len() == 2).await;
    let title_req = m.requests()[1].clone();
    assert!(title_req.system.starts_with("You name conversations"));
    assert_eq!(title_req.model, "claude-haiku-4-5");
}

#[tokio::test]
async fn cancel_keeps_the_partial_text() {
    let events: Script = (0..50)
        .map(|i| text(&format!("w{i} ")))
        .chain([end()])
        .collect();
    let m = manager(events, Duration::from_millis(20));
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    let turn = m
        .start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| !sink.text().is_empty()).await;
    assert!(m.cancel(turn));
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Cancelled));
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(detail.turns[0].status, TurnStatus::Cancelled);
    assert_eq!(detail.turns[0].stop_reason, Some(StopReason::Cancelled));
    let kept = detail.turns[0].assistant_text();
    // Two assertions rather than one `&&`, and both print what they saw: this one fails under a
    // loaded machine about one run in three, and a conjunction that says only "assertion failed"
    // cannot tell "the partial text was lost" from "the stream finished before the cancel".
    assert!(
        kept.starts_with("w0 "),
        "the partial text was kept: {kept:?}"
    );
    assert!(!kept.contains("w49"), "and it is partial: {kept:?}");
    assert!(!m.cancel(turn), "a finished turn cannot be cancelled again");
}

#[tokio::test]
async fn a_mid_stream_error_fails_the_turn_and_keeps_the_text() {
    let m = manager(
        vec![
            text("Part"),
            Err(ProviderError::new(
                ProviderErrorKind::Overloaded,
                "upstream reset",
            )),
        ],
        Duration::ZERO,
    );
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Failed));
    let err = sink.kinds().into_iter().find_map(|k| match k {
        AgentEventKind::Error {
            message, retryable, ..
        } => Some((message, retryable)),
        _ => None,
    });
    assert_eq!(err, Some(("upstream reset".to_owned(), true)));
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(detail.turns[0].error.as_deref(), Some("upstream reset"));
    assert_eq!(detail.turns[0].assistant_text(), "Part");
}

#[tokio::test]
async fn a_late_subscriber_gets_a_snapshot_then_live_events() {
    let events: Script = (0..30)
        .map(|i| text(&format!("{i} ")))
        .chain([end()])
        .collect();
    let m = manager(events, Duration::from_millis(15));
    let chat = m.chat();
    let first = Arc::new(Collect::default());
    let turn = m
        .start(chat.id, "go".into(), Vec::new(), Vec::new(), first.clone())
        .unwrap();
    wait_for(|| first.text().len() > 10).await;

    let late = Arc::new(Collect::default());
    m.subscribe(turn, 0, late.clone()).unwrap();
    wait_for(|| late.completed().is_some()).await;

    let kinds = late.kinds();
    let snapshot = match &kinds[0] {
        AgentEventKind::TurnSnapshot { snapshot } => snapshot.clone(),
        other => panic!("first event must be the snapshot, got {other:?}"),
    };
    assert_eq!(snapshot.status, TurnStatus::Running);
    let snap_text = snapshot.messages[0].text();
    assert!(!snap_text.is_empty());
    // Live deltas with seq > snapshot.seq complete the text exactly once.
    let live: String = late
        .0
        .lock()
        .unwrap()
        .iter()
        .flat_map(|b| b.events.clone())
        .filter(|e| e.seq > snapshot.seq)
        .filter_map(|e| match e.event {
            AgentEventKind::TextDelta { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(snap_text + &live, first.text());
    assert!(
        m.subscribe(turn, 0, late).is_err(),
        "finished turns are not subscribable"
    );
}

#[tokio::test]
async fn without_a_provider_the_turn_fails_cleanly() {
    struct NoSource;
    impl ProviderSource for NoSource {
        fn provider(&self, _id: &ProviderId) -> Option<Arc<dyn Provider>> {
            None
        }
    }
    let (_dir, chats) = book();
    // A model is picked; it is the *provider* that is missing, which is what this tests.
    let mut settings = Settings::default();
    settings.chat.default_model = Some(ModelRef::new(ProviderId::openrouter(), "test/model"));
    let m = TurnManager::new(
        chats,
        Arc::new(NoSource),
        Arc::new(ConnectorRegistry::new()),
        Arc::new(RwLock::new(settings)),
        PromptContext::default(),
        tokio::runtime::Handle::current(),
    );
    let chat = m.create_chat(None).unwrap();
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Failed));
    assert!(
        m.chats().get(chat.id).unwrap().unwrap().turns[0]
            .messages
            .is_empty()
    );
}

/// Tauri commands run on the UI thread, outside every runtime; starting a turn there must work.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_can_be_started_from_a_plain_thread() {
    let m = manager(vec![text("ok"), end()], Duration::ZERO);
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    let (m2, sink2) = (m.clone(), sink.clone());
    std::thread::spawn(move || {
        m2.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink2)
            .unwrap()
    })
    .join()
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
}

/// A one-pixel PNG, base64. Small enough to read in a diff, real enough to be stored.
const ONE_PIXEL: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

// ---- M3: the tool loop ----------------------------------------------------------------

#[tokio::test]
async fn an_allowed_call_runs_and_its_result_goes_back_to_the_model() {
    let m = manager_with(
        vec![
            tool_round("call_1", "fake__echo", serde_json::json!({ "text": "hi" })),
            vec![text("Echoed: hi"), end()],
        ],
        Duration::ZERO,
        Settings::default(), // Auto-edit: reads run without asking
    );
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    let turn = m
        .start(
            chat.id,
            "echo hi".into(),
            Vec::new(),
            Vec::new(),
            sink.clone(),
        )
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));

    let names = sink.names();
    for expected in [
        "tool_call.started",
        "tool_call.args_delta",
        "tool_call.ready",
        "tool_call.executing",
        "tool_call.completed",
    ] {
        assert!(names.contains(&expected), "missing {expected} in {names:?}");
    }
    assert!(!names.contains(&"decision.requested"));
    assert_eq!(
        names.iter().filter(|n| **n == "message.started").count(),
        2,
        "one assistant message per round"
    );

    // The second request replays the call and carries the result.
    let second = m.requests()[1].clone();
    let n = second.messages.len();
    assert_eq!(second.messages[n - 2].role, Role::Assistant);
    assert!(matches!(
        &second.messages[n - 2].parts[1],
        ContentPart::ToolCall { name, .. } if name == "fake__echo"
    ));
    assert_eq!(second.messages[n - 1].role, Role::Tool);
    assert!(matches!(
        &second.messages[n - 1].parts[0],
        ContentPart::ToolResult { call_id, is_error: false, content } if call_id.as_str() == "call_1" && serde_json::to_string(content).unwrap().contains("\"hi\"")
    ));
    assert_eq!(m.fake.calls.lock().unwrap()[0].tool, "echo");

    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let t = &detail.turns[0];
    assert_eq!(t.id, turn);
    assert_eq!(t.messages.len(), 3, "assistant, tool, assistant");
    assert_eq!(t.assistant_text(), "Let me check.\n\nEchoed: hi");
    assert_eq!(t.tool_calls.len(), 1);
    let c = &t.tool_calls[0];
    assert_eq!(c.status, ToolCallStatus::Completed);
    assert_eq!(c.decision_source, Some(DecisionSource::Mode));
    assert_eq!(c.connector, "fake");
    assert_eq!(c.tool, "echo");
    assert_eq!(c.display.summary, "text=hi");
    assert!(
        c.result.is_some(),
        "the result is read back from the transcript"
    );
    assert!(c.result_preview.as_deref().unwrap().contains("hi"));
    assert!(c.duration_ms.is_some());
    let tool_calls = sink.kinds().into_iter().find_map(|k| match k {
        AgentEventKind::TurnCompleted { tool_calls, .. } => Some(tool_calls),
        _ => None,
    });
    assert_eq!(tool_calls, Some(1));
}

#[tokio::test]
async fn manual_mode_asks_and_allow_once_runs_the_call() {
    let m = manager_with(
        vec![
            tool_round("call_1", "gantry__clock", serde_json::json!({})),
            vec![text("It is today."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    manual(&m, chat.id);
    let sink = Arc::new(Collect::default());
    let turn = m
        .start(
            chat.id,
            "what day is it".into(),
            Vec::new(),
            Vec::new(),
            sink.clone(),
        )
        .unwrap();
    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;

    let pending = m.interactions().list_pending(Some(chat.id));
    assert_eq!(pending.len(), 1);
    let gantry_core::InteractionPayload::Permission { request } = &pending[0].payload else {
        panic!("the pending interaction is not a permission prompt");
    };
    assert_eq!(request.model_tool_name, "gantry__clock");
    assert_eq!(request.tier, RiskTier::Read);
    assert_eq!(request.why.as_deref(), Some("Let me check"));
    assert_eq!(m.notes.pending.lock().unwrap().last(), Some(&(chat.id, 1)));
    wait_for(|| sink.names().contains(&"decision.requested")).await;
    assert!(!sink.names().contains(&"tool_call.executing"));

    // A late subscriber sees the pending card and the waiting call in the snapshot.
    let late = Arc::new(Collect::default());
    m.subscribe(turn, 0, late.clone()).unwrap();
    let AgentEventKind::TurnSnapshot { snapshot } = late.kinds()[0].clone() else {
        panic!("snapshot first");
    };
    assert_eq!(snapshot.pending.len(), 1);
    assert_eq!(
        snapshot.tool_calls[0].status,
        ToolCallStatus::AwaitingDecision
    );

    m.resolve_interaction(
        pending[0].id,
        InteractionResolution::Permission {
            decision: PermissionDecision::AllowOnce,
            message: None,
            chosen: Default::default(),
        },
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
    assert!(sink.names().contains(&"decision.resolved"));
    assert!(m.interactions().list_pending(None).is_empty());
    assert_eq!(m.notes.pending.lock().unwrap().last(), Some(&(chat.id, 0)));

    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let c = &detail.turns[0].tool_calls[0];
    assert_eq!(c.status, ToolCallStatus::Completed);
    assert_eq!(c.decision_source, Some(DecisionSource::UserOnce));
    assert!(c.result_preview.as_deref().unwrap().contains("weekday"));
    let rows = m
        .chats()
        .store()
        .read(|conn| gantry_store::repos::interactions::list_for_turn(conn, turn))
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, InteractionStatus::Resolved);
    assert!(
        m.resolve_interaction(pending[0].id, InteractionResolution::Cancelled)
            .is_err(),
        "a resolved interaction is gone"
    );
}

#[tokio::test]
async fn a_denial_with_a_message_reaches_the_model() {
    let m = manager_with(
        vec![
            tool_round("call_1", "fake__echo", serde_json::json!({ "text": "x" })),
            vec![text("Understood."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    manual(&m, chat.id);
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;
    let id = m.interactions().list_pending(Some(chat.id))[0].id;
    m.resolve_interaction(
        id,
        InteractionResolution::Permission {
            decision: PermissionDecision::Deny,
            message: Some("not now, ask me tomorrow".into()),
            chosen: Default::default(),
        },
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
    assert!(
        m.fake.calls.lock().unwrap().is_empty(),
        "denied calls never run"
    );
    let second = m.requests()[1].clone();
    let tool_msg = second.messages.last().unwrap();
    let ContentPart::ToolResult {
        content, is_error, ..
    } = &tool_msg.parts[0]
    else {
        panic!("a tool result follows the call");
    };
    assert!(*is_error);
    assert!(
        serde_json::to_string(content)
            .unwrap()
            .contains("ask me tomorrow")
    );
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(detail.turns[0].tool_calls[0].status, ToolCallStatus::Denied);
}

#[tokio::test]
async fn cancelling_while_a_card_waits_ends_the_turn() {
    let m = manager_with(
        vec![tool_round("call_1", "fake__echo", serde_json::json!({}))],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    manual(&m, chat.id);
    let sink = Arc::new(Collect::default());
    let turn = m
        .start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;
    let id = m.interactions().list_pending(Some(chat.id))[0].id;
    assert!(m.cancel(turn));
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Cancelled));
    assert!(m.interactions().list_pending(None).is_empty());
    let rows = m
        .chats()
        .store()
        .read(|conn| gantry_store::repos::interactions::get(conn, id))
        .unwrap()
        .unwrap();
    assert_eq!(rows.status, InteractionStatus::Cancelled);
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let t = &detail.turns[0];
    assert_eq!(t.tool_calls[0].status, ToolCallStatus::Cancelled);
    // The transcript stays complete: the call has a (synthetic) result.
    assert_eq!(t.messages.len(), 2);
    assert!(matches!(
        &t.messages[1].parts[0],
        ContentPart::ToolResult { is_error: true, .. }
    ));
}

#[tokio::test]
async fn unknown_and_failing_tools_become_error_results() {
    let m = manager_with(
        vec![
            vec![
                Ok(StreamEvent::ToolCallStart {
                    index: 0,
                    id: CallId("c1".into()),
                    name: "nope__x".into(),
                }),
                Ok(StreamEvent::ToolCallEnd {
                    index: 0,
                    args: serde_json::json!({}),
                }),
                Ok(StreamEvent::ToolCallStart {
                    index: 1,
                    id: CallId("c2".into()),
                    name: "fake__boom".into(),
                }),
                Ok(StreamEvent::ToolCallEnd {
                    index: 1,
                    args: serde_json::json!({}),
                }),
                Ok(StreamEvent::MessageEnd {
                    stop_reason: StopReason::ToolUse,
                }),
            ],
            vec![text("Sorry."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let calls = &detail.turns[0].tool_calls;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].status, ToolCallStatus::Failed);
    assert!(
        calls[0]
            .result_preview
            .as_deref()
            .unwrap()
            .contains("Unknown tool")
    );
    assert_eq!(calls[1].status, ToolCallStatus::Failed);
    assert!(
        calls[1]
            .result_preview
            .as_deref()
            .unwrap()
            .contains("kaboom")
    );
    let second = m.requests()[1].clone();
    let tool_msg = second.messages.last().unwrap();
    assert_eq!(tool_msg.parts.len(), 2, "one result per call, in order");
}

#[tokio::test]
async fn the_round_cap_stops_a_looping_model() {
    let mut settings = Settings::default();
    settings.advanced.max_tool_rounds = 1;
    let m = manager_with(
        vec![
            tool_round("c1", "fake__echo", serde_json::json!({ "text": "1" })),
            tool_round("c2", "fake__echo", serde_json::json!({ "text": "2" })),
            tool_round("c3", "fake__echo", serde_json::json!({ "text": "3" })),
        ],
        Duration::ZERO,
        settings,
    );
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "loop".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
    assert_eq!(
        m.fake.calls.lock().unwrap().len(),
        1,
        "only the first round ran"
    );
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(
        detail.turns[0].stop_reason,
        Some(StopReason::Other {
            reason: "max_tool_rounds".into()
        })
    );
    assert!(sink.kinds().iter().any(
        |k| matches!(k, AgentEventKind::ProviderNotice { kind, .. } if kind == "tool_round_cap")
    ));
    let calls = &detail.turns[0].tool_calls;
    assert_eq!(calls[1].status, ToolCallStatus::Cancelled);
    // The transcript is still complete for the next turn.
    assert!(matches!(
        &detail.turns[0].messages.last().unwrap().parts[0],
        ContentPart::ToolResult { .. }
    ));
}

/// The footer's two new numbers (15 §7): a round the provider does not bill is priced from the
/// catalog's list prices and says so, and the time the model spent producing is measured from
/// its first token — not from the request, whose wait is queueing, not speed.
#[tokio::test]
async fn a_round_is_timed_and_priced_from_the_catalog_when_nobody_bills_it() {
    let m = manager(
        vec![
            Ok(StreamEvent::MessageStart {
                provider_message_id: None,
            }),
            text("one "),
            text("two "),
            text("three"),
            Ok(StreamEvent::Usage(Usage {
                input: 1_000_000,
                output: 1_000_000,
                ..Default::default()
            })),
            end(),
        ],
        Duration::from_millis(30),
    );
    // $2 in, $10 out per million, for the harness's own model.
    m.chats()
        .store()
        .write_blocking(|conn| {
            gantry_store::repos::providers::ensure(
                conn,
                "openrouter",
                "openai_chat",
                "OpenRouter",
                None,
            )?;
            gantry_store::repos::models::replace_for(
                conn,
                "openrouter",
                &[gantry_store::repos::models::ModelRecord {
                    provider_id: "openrouter".into(),
                    model_id: "test/model".into(),
                    display_name: "Test".into(),
                    capabilities_json: "{}".into(),
                    context_window: None,
                    max_output: None,
                    pricing_json: Some(
                        r#"{"input_per_mtok":2.0,"output_per_mtok":10.0,"cache_read_per_mtok":null}"#
                            .into(),
                    ),
                    created_at: None,
                    fetched_at: gantry_core::now_ms(),
                }],
            )
        })
        .unwrap();
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    let turn = m
        .start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let t = detail.turns.iter().find(|t| t.id == turn).unwrap();
    let usage = t.usage.unwrap();
    assert_eq!(
        usage.cost_usd,
        Some(12.0),
        "a million in at $2 and a million out at $10"
    );
    assert!(usage.cost_is_estimate, "and it says it is an estimate");
    // Three text events thirty milliseconds apart: the clock starts at the first of them.
    assert!(
        usage.generation_ms >= 50,
        "the time spent producing was measured: {} ms",
        usage.generation_ms
    );
}

/// One assistant message asking for the same call over and over: a small model on 2026-09-22
/// emitted forty-six `code-editor__replace` calls in one reply, none of them answered, and the
/// only thing that stopped it was the user. The first one runs; the rest are refused with a
/// result the model can read, and the row says it did not run.
#[tokio::test]
async fn a_reply_that_repeats_one_call_runs_it_once() {
    let mut round: Script = Vec::new();
    for i in 0..5 {
        round.push(Ok(StreamEvent::ToolCallStart {
            index: i,
            id: CallId(format!("dup_{i}")),
            name: "fake__echo".into(),
        }));
        round.push(Ok(StreamEvent::ToolCallEnd {
            index: i,
            args: serde_json::json!({ "text": "same" }),
        }));
    }
    round.push(Ok(StreamEvent::MessageEnd {
        stop_reason: StopReason::ToolUse,
    }));
    let m = manager_with(
        vec![round, vec![text("Done."), end()]],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    assert_eq!(
        m.fake.calls.lock().unwrap().len(),
        1,
        "the tool ran once, not five times"
    );
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let calls = &detail.turns[0].tool_calls;
    assert_eq!(calls[0].status, ToolCallStatus::Completed);
    for call in &calls[1..] {
        assert_eq!(call.status, ToolCallStatus::Cancelled);
        assert!(
            call.result_preview
                .as_deref()
                .unwrap_or_default()
                .contains("exact duplicate"),
            "the model is told why: {:?}",
            call.result_preview
        );
    }
    // Every call still has a result, or the next request would be malformed.
    let second = m.requests()[1].clone();
    assert_eq!(second.messages.last().unwrap().parts.len(), 5);
    assert!(sink.kinds().iter().any(
        |k| matches!(k, AgentEventKind::ProviderNotice { kind, .. } if kind == "tool_call_cap")
    ));
}

/// The breadth limit, which catches a storm of calls that are not identical.
#[tokio::test]
async fn a_reply_may_not_ask_for_more_calls_than_the_limit() {
    let mut settings = Settings::default();
    settings.advanced.max_calls_per_reply = 2;
    let mut round: Script = Vec::new();
    for i in 0..4 {
        round.push(Ok(StreamEvent::ToolCallStart {
            index: i,
            id: CallId(format!("c{i}")),
            name: "fake__echo".into(),
        }));
        round.push(Ok(StreamEvent::ToolCallEnd {
            index: i,
            args: serde_json::json!({ "text": format!("{i}") }),
        }));
    }
    round.push(Ok(StreamEvent::MessageEnd {
        stop_reason: StopReason::ToolUse,
    }));
    let m = manager_with(
        vec![round, vec![text("Done."), end()]],
        Duration::ZERO,
        settings,
    );
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    assert_eq!(
        m.fake.calls.lock().unwrap().len(),
        2,
        "the first two ran, in the model's own order"
    );
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let calls = &detail.turns[0].tool_calls;
    assert_eq!(calls[0].status, ToolCallStatus::Completed);
    assert_eq!(calls[1].status, ToolCallStatus::Completed);
    assert_eq!(calls[2].status, ToolCallStatus::Cancelled);
    assert!(
        calls[3]
            .result_preview
            .as_deref()
            .unwrap_or_default()
            .contains("at most 2 tool calls per reply"),
        "{:?}",
        calls[3].result_preview
    );
}

#[tokio::test]
async fn plan_mode_offers_only_tools_it_would_allow() {
    let m = manager(vec![text("plan"), end()], Duration::ZERO);
    let chat = m.chat();
    m.update_chat(
        chat.id,
        ChatPatch {
            mode: Some(Mode::Plan),
            ..Default::default()
        },
    )
    .unwrap();
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    let names: Vec<String> = m.requests()[0]
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(
        names,
        [
            "fake__echo",
            "fake__boom",
            "fake__stream",
            "fake__paint",
            "gantry__clock",
            "gantry__search_connectors",
            "gantry__request_access",
            "gantry__suggest_connector",
        ],
        "the connector tools are `app` tier, so Plan mode keeps them"
    );
}

/// A code session is defined by the folder it works in (16 C3, C5): it cannot be created
/// without one, it cannot send a message without one, and it never appears in the chat list.
#[tokio::test]
async fn a_code_session_needs_a_folder_and_keeps_its_own_list() {
    let m = manager(vec![text("hi"), end()], Duration::ZERO);
    let refused = m.create_session(gantry_core::Surface::Code, Vec::new(), None, false, None);
    assert!(
        refused.is_err(),
        "a code session with no folder was created"
    );

    let chat = m.create_chat(None).unwrap();
    let code = m
        .create_session(
            gantry_core::Surface::Code,
            vec!["/home/olav/dev/gantry".into()],
            None,
            false,
            None,
        )
        .unwrap();
    assert_eq!(code.surface, gantry_core::Surface::Code);
    assert_eq!(code.roots, ["/home/olav/dev/gantry"]);

    let chats = m.chats().list(gantry_core::Surface::Chat).unwrap();
    let sessions = m.chats().list(gantry_core::Surface::Code).unwrap();
    assert_eq!(chats.iter().map(|c| c.id).collect::<Vec<_>>(), [chat.id]);
    assert_eq!(sessions.iter().map(|c| c.id).collect::<Vec<_>>(), [code.id]);
    assert_eq!(
        sessions[0].roots,
        ["/home/olav/dev/gantry"],
        "the sidebar names the folder"
    );

    // The folder can be taken away only through the store; the turn then refuses.
    m.chats()
        .remove_root(code.id, "/home/olav/dev/gantry".into())
        .unwrap();
    let sink = Arc::new(Collect::default());
    let err = m
        .start(code.id, "go".into(), Vec::new(), Vec::new(), sink)
        .expect_err("a code session with no folder sent a message");
    assert!(format!("{err:?}").contains("folder"), "{err:?}");
}

/// The model finds a connector it does not have, asks for it, and uses it in the same reply
/// (03 §9, 04 §9). Nothing is attached without the user's answer, and once it is attached the
/// tool set is rebuilt mid-turn so the next round can call it.
#[tokio::test]
async fn an_access_request_attaches_a_connector_and_its_tools_arrive_in_the_same_turn() {
    let m = manager_with(
        vec![
            tool_round(
                "c0",
                "gantry__search_connectors",
                serde_json::json!({ "query": "echo" }),
            ),
            tool_round(
                "c1",
                "gantry__request_access",
                serde_json::json!({
                    "connector": "fake",
                    "tools": ["echo"],
                    "reason": "to echo the text you asked about"
                }),
            ),
            tool_round("c2", "fake__echo", serde_json::json!({ "text": "hi" })),
            vec![text("done"), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    install_fake(m.chats().store());
    let chat = m.create_chat(None).unwrap();
    // Manual mode, to prove the `app` tier never prompts for the asking itself.
    manual(&m, chat.id);
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "echo hi".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();

    // The search answered without a card; the request raised one.
    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;
    let pending = m.interactions().list_pending(Some(chat.id));
    assert_eq!(pending.len(), 1);
    let gantry_core::InteractionPayload::AccessRequest { request } = &pending[0].payload else {
        panic!(
            "the card is not an access request: {:?}",
            pending[0].payload
        );
    };
    assert_eq!(request.connector, "fake");
    assert_eq!(request.tools, ["echo"]);
    assert!(request.reason.contains("echo the text"));
    assert_eq!(
        m.notes.pending.lock().unwrap().last().map(|(_, n)| *n),
        Some(1),
        "the sidebar badge is told about a card a tool raised"
    );

    m.resolve_interaction(
        pending[0].id,
        InteractionResolution::AccessRequest {
            decision: gantry_core::AccessDecision::Attach { allow_tools: true },
            message: None,
        },
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));

    // The round after the answer was given the connector's tools…
    let requests = m.requests();
    let after = &requests[2];
    let names: Vec<&str> = after.tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"fake__echo"), "{names:?}");
    // …and a note saying so, so the model is not guessing.
    let note = after.messages.iter().find(|msg| msg.role == Role::System);
    assert!(
        matches!(
            note.map(|msg| msg.parts.as_slice()),
            Some([ContentPart::ToolSetChange { added, .. }]) if added == &["fake".to_owned()]
        ),
        "no tool-set change reached the model: {note:?}"
    );
    // The call ran without a second prompt, because "attach and allow these tools" granted it.
    assert!(
        m.fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.tool == "echo")
    );
    let grants = m.chats().grants(chat.id).unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].tool_name.as_deref(), Some("echo"));
    assert_eq!(grants[0].source, gantry_core::GrantSource::AccessRequest);
    // The attachment outlives the turn: the chat keeps it.
    assert_eq!(m.chats().attached_connectors(chat.id).unwrap(), ["fake"]);
}

/// The card is the only way in: a refusal leaves the chat exactly as it was.
#[tokio::test]
async fn a_refused_access_request_attaches_nothing() {
    let m = manager_with(
        vec![
            tool_round(
                "c1",
                "gantry__request_access",
                serde_json::json!({ "connector": "fake", "reason": "to echo" }),
            ),
            vec![text("I cannot do that without it."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    install_fake(m.chats().store());
    let chat = m.create_chat(None).unwrap();
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "echo hi".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;
    let pending = m.interactions().list_pending(Some(chat.id));
    m.resolve_interaction(
        pending[0].id,
        InteractionResolution::AccessRequest {
            decision: gantry_core::AccessDecision::Deny,
            message: Some("Not this time".into()),
        },
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert!(m.chats().attached_connectors(chat.id).unwrap().is_empty());
    assert!(m.fake.calls.lock().unwrap().is_empty());
    let requests = m.requests();
    let names: Vec<&str> = requests[1].tools.iter().map(|t| t.name.as_str()).collect();
    assert!(!names.contains(&"fake__echo"), "{names:?}");
}

/// Installing a connector does not give it to every conversation: a chat sees a connector only
/// once it has attached it (03 §11). The runtime tools are the app's own and are always there.
#[tokio::test]
async fn a_chat_sees_only_the_connectors_it_attached() {
    let m = manager(vec![text("hi"), end()], Duration::ZERO);
    install_fake(m.chats().store());
    let bare = m.create_chat(None).unwrap();
    let sink = Arc::new(Collect::default());
    m.start(bare.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    let names: Vec<String> = m.requests()[0]
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(
        names,
        [
            "gantry__clock",
            "gantry__search_connectors",
            "gantry__request_access",
            "gantry__suggest_connector",
        ],
        "the fake connector is installed but not attached, so only the way to ask for it is here"
    );
    // And the model is told, in words, what it may ask for (04 §9).
    let system = &m.requests()[0].system;
    assert!(system.contains("attached to this chat: none"), "{system}");
    assert!(
        system.contains("installed, not attached: fake (0 tools)"),
        "{system}"
    );
}

/// Context management, end to end (02 §6): a chat that grows past three quarters of the model's
/// window gets its older turns summarized, and the next request carries the summary instead of
/// the messages. Nothing is deleted — the chat still shows every one of them.
#[tokio::test]
async fn a_long_chat_is_summarized_and_the_next_request_carries_the_summary() {
    // A small window, so a handful of ordinary turns is enough to cross the threshold, and a
    // usage report the provider makes, which is what the budget prefers to counting characters.
    let heavy = || -> Script {
        vec![
            text("ok"),
            Ok(StreamEvent::Usage(Usage {
                input: 900,
                output: 10,
                ..Default::default()
            })),
            end(),
        ]
    };
    let m = manager_windowed(
        (0..8).map(|_| heavy()).collect(),
        Duration::ZERO,
        Settings::default(),
        Some(1_000),
    );
    let chat = m.chat();

    // Six exchanges. Keep-tail leaves the last three turns alone, so it takes more than three
    // before there is anything old enough — and long enough — to be worth summarizing.
    for i in 0..6 {
        let sink = Arc::new(Collect::default());
        m.start(
            chat.id,
            format!("question {i}"),
            Vec::new(),
            Vec::new(),
            sink.clone(),
        )
        .unwrap();
        wait_for(|| sink.completed() == Some(TurnStatus::Completed)).await;
    }
    wait_for(|| {
        m.requests()
            .iter()
            .any(|r| r.system.starts_with("You are summarizing"))
    })
    .await;

    // The summarizer was asked with the cheap model and saw what happened, not the raw window.
    let summary_req = m
        .requests()
        .into_iter()
        .find(|r| r.system.starts_with("You are summarizing"))
        .expect("the budget asked for a summary");
    assert_eq!(summary_req.model, "deepseek/deepseek-v4-flash");
    assert!(summary_req.messages[0].text().contains("question 0"));

    // The marker is in the transcript, and it is a system message, so the chat still shows
    // every message it stands for.
    wait_for(|| {
        m.chats()
            .get(chat.id)
            .unwrap()
            .unwrap()
            .turns
            .iter()
            .any(|t| {
                t.messages.iter().any(|msg| {
                    msg.parts
                        .iter()
                        .any(|p| matches!(p, ContentPart::Compacted { .. }))
                })
            })
    })
    .await;
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(detail.turns.len(), 6, "every turn is still in the chat");
    let asked: Vec<String> = detail.turns.iter().map(|t| t.user.text()).collect();
    assert_eq!(
        asked,
        (0..6).map(|i| format!("question {i}")).collect::<Vec<_>>(),
        "including the ones the summary stands for: compaction is a marker, not a delete"
    );

    // The next request sends the summary and the kept tail, not the summarized messages.
    let before = m.requests().len();
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "question 4".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    let next = m.requests()[before].clone();
    let sent: String = next
        .messages
        .iter()
        .map(gantry_core::Message::text)
        .collect();
    assert!(
        next.messages.iter().any(|msg| msg
            .parts
            .iter()
            .any(|p| matches!(p, ContentPart::Compacted { .. }))),
        "the summary goes first"
    );
    assert!(
        !sent.contains("question 0"),
        "the summarized messages are not sent again: {sent}"
    );
    assert!(sent.contains("question 4"), "and the new one is: {sent}");
}

/// A tool result too large for the transcript is cut at ingestion, keeping both ends, and the
/// cut is what gets written down (05 §8). The setting is in Settings → Advanced.
#[tokio::test]
async fn a_huge_tool_result_is_cut_to_the_configured_size() {
    let mut settings = Settings::default();
    settings.advanced.max_result_kb = 1;
    let m = manager_with(
        vec![
            vec![
                Ok(StreamEvent::ToolCallStart {
                    index: 0,
                    id: CallId("c1".into()),
                    name: "fake__echo".into(),
                }),
                Ok(StreamEvent::ToolCallEnd {
                    index: 0,
                    args: serde_json::json!({ "text": "x".repeat(20_000) }),
                }),
                end(),
            ],
            vec![text("done"), end()],
        ],
        Duration::ZERO,
        settings,
    );
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "echo a lot".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| sink.completed() == Some(TurnStatus::Completed)).await;

    let second = m.requests()[1].clone();
    let result = second
        .messages
        .iter()
        .flat_map(|msg| msg.parts.iter())
        .find_map(|p| match p {
            ContentPart::ToolResult { content, .. } => {
                Some(gantry_core::result_preview(content, usize::MAX))
            }
            _ => None,
        })
        .expect("the result reached the next request");
    assert!(
        result.len() < 4_000,
        "cut to about a kilobyte: {}",
        result.len()
    );
    assert!(result.contains("characters omitted"), "and says so");

    // What the cut took out is not lost: the whole output is a blob, which is what the row's
    // drawer reads and what `tool_call_output` hands back.
    let call = m
        .chats()
        .tool_call(&CallId("c1".into()))
        .unwrap()
        .expect("the call has a row");
    let hash = call
        .result_blob_hash
        .expect("the whole output was kept somewhere");
    let whole = String::from_utf8(m.chats().blobs().get(&hash).unwrap()).unwrap();
    assert!(whole.len() > 20_000, "all of it: {}", whole.len());
    assert!(!whole.contains("characters omitted"), "and uncut");
}

// ── The guard (04 §6) ────────────────────────────────────────────────────────────────────

/// Guarded Auto, hands off: the guard allows the work and blocks what the user did not ask
/// for, and the user is never interrupted for either.
#[tokio::test]
async fn the_guard_decides_in_auto_mode_without_asking_the_user() {
    let m = manager_with(
        vec![
            tool_round(
                "c1",
                "fake__write",
                serde_json::json!({ "path": "src/lib.rs" }),
            ),
            tool_round(
                "c2",
                "fake__write",
                serde_json::json!({ "path": "/etc/passwd" }),
            ),
            vec![text("Done."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    m.guarded(chat.id);
    m.guard_says(&[
        r#"{"decision":"allow","confidence":0.9,"reason":"Edits the file the task is about","flags":[]}"#,
        r#"{"decision":"deny","confidence":0.95,"reason":"Writes outside the workspace, which the task never mentioned","flags":["outside_task"]}"#,
    ]);
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "Fix the parser".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
    assert!(
        !sink.names().contains(&"decision.requested"),
        "the guard never opens a blocking prompt: {:?}",
        sink.names()
    );
    assert_eq!(
        sink.names()
            .iter()
            .filter(|n| **n == "judge.decision")
            .count(),
        2,
        "every decision is audited, the allow as well as the block"
    );

    let calls = &m.chats().get(chat.id).unwrap().unwrap().turns[0].tool_calls;
    let allowed = calls.iter().find(|c| c.id.as_str() == "c1").unwrap();
    assert_eq!(allowed.status, ToolCallStatus::Completed);
    assert_eq!(allowed.decision_source, Some(DecisionSource::Judge));
    let verdict = allowed.judge.as_ref().expect("the allow is kept too");
    assert!(verdict.allows());
    assert_eq!(verdict.reason, "Edits the file the task is about");
    assert_eq!(verdict.model, "deepseek/deepseek-v4-flash");

    let blocked = calls.iter().find(|c| c.id.as_str() == "c2").unwrap();
    assert_eq!(blocked.status, ToolCallStatus::Denied);
    assert_eq!(blocked.decision_source, Some(DecisionSource::Judge));
    assert!(!blocked.judge.as_ref().unwrap().allows());
    assert_eq!(
        blocked.judge.as_ref().unwrap().flags,
        vec![gantry_core::JudgeFlag::OutsideTask]
    );

    // Only the allowed call reached the connector, and the model was told why the other
    // did not.
    let ran: Vec<String> = m
        .fake
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.args.to_string())
        .collect();
    assert_eq!(ran, [r#"{"path":"src/lib.rs"}"#]);
    let result = m.chats().get(chat.id).unwrap().unwrap().turns[0]
        .messages
        .iter()
        .flat_map(|msg| msg.parts.clone())
        .find_map(|p| match p {
            ContentPart::ToolResult {
                call_id, content, ..
            } if call_id.as_str() == "c2" => Some(content),
            _ => None,
        })
        .expect("the model gets a result for a blocked call too");
    let json = format!("{result:?}");
    assert!(json.contains("blocked_by_guard"), "{json}");
    assert!(json.contains("Writes outside the workspace"), "{json}");

    // What the guard was told: the task in the user's words, the action, and the history.
    let asked = m.guard_inputs();
    assert!(
        asked[0].contains("What the user first asked: Fix the parser"),
        "{}",
        asked[0]
    );
    assert!(asked[0].contains("This is the first tool call of the turn."));
    assert!(asked[1].contains("/etc/passwd"), "{}", asked[1]);
    assert!(
        asked[1].contains("allowed by the guard"),
        "the second decision knows how the first went: {}",
        asked[1]
    );
}

/// Fail closed (04 §1): a guard that cannot answer hands the question to the user, with the
/// reason it could not, rather than guessing either way.
#[tokio::test]
async fn a_guard_that_cannot_decide_asks_the_user() {
    let m = manager_with(
        vec![
            tool_round("c1", "fake__write", serde_json::json!({})),
            vec![text("Fine."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    m.guarded(chat.id);
    m.guard_says(&["Sure, that looks fine to me!"]);
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();

    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;
    let pending = m.interactions().list_pending(Some(chat.id));
    let gantry_core::InteractionPayload::Permission { request } = &pending[0].payload else {
        panic!("a permission card");
    };
    let note = request
        .guard
        .as_deref()
        .expect("the card says why it exists");
    assert!(note.contains("unreadable"), "{note}");
    assert!(note.contains("yours"), "{note}");

    m.resolve_interaction(
        pending[0].id,
        InteractionResolution::Permission {
            decision: PermissionDecision::AllowOnce,
            message: None,
            chosen: Default::default(),
        },
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
    assert_eq!(m.fake.calls.lock().unwrap().len(), 1);
    let names = sink.names();
    assert!(names.contains(&"provider.notice"), "{names:?}");
}

/// Rule 4 of the pipeline: the same call failing over and over is refused without a model
/// being asked, because the answer cannot depend on judgement.
#[tokio::test]
async fn a_call_that_keeps_failing_is_stopped_without_asking_the_guard() {
    let round = || tool_round("c", "fake__crash", serde_json::json!({ "n": 1 }));
    let m = manager_with(
        vec![
            round(),
            round(),
            round(),
            round(),
            vec![text("Giving up."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    m.guarded(chat.id);
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "build it".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    assert_eq!(
        m.guard_inputs().len(),
        3,
        "the fourth attempt costs nothing: the loop detector already knows"
    );
    assert_eq!(
        m.fake.calls.lock().unwrap().len(),
        3,
        "and it never reaches the connector again"
    );
    let verdicts: Vec<_> = sink
        .kinds()
        .iter()
        .filter_map(|k| match k {
            AgentEventKind::JudgeDecision { verdict, .. } => Some((**verdict).clone()),
            _ => None,
        })
        .collect();
    assert_eq!(verdicts.len(), 4);
    let last = verdicts.last().unwrap();
    assert_eq!(last.source, gantry_core::JudgeSource::Loop);
    assert!(
        last.reason.contains("already failed 3 times"),
        "{}",
        last.reason
    );
    assert_eq!(last.flags, vec![gantry_core::JudgeFlag::Loop]);
}

/// The floor is above the guard, not under it: what a guardrail asks about stays the user's
/// question, and what it refuses is refused before any model is asked.
#[tokio::test]
async fn the_floor_outranks_the_guard() {
    let m = manager_with(
        vec![
            tool_round(
                "c1",
                "fake__write",
                serde_json::json!({ "path": "/home/olav/.ssh/id_ed25519" }),
            ),
            vec![text("ok"), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    m.guarded(chat.id);
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "read my key".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();

    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;
    let pending = m.interactions().list_pending(Some(chat.id));
    let gantry_core::InteractionPayload::Permission { request } = &pending[0].payload else {
        panic!("a permission card");
    };
    assert_eq!(request.guardrail.as_ref().unwrap().rule, "ssh");
    assert!(
        m.guard_inputs().is_empty(),
        "no model is asked about a question that is the user's"
    );
    assert!(m.cancel(pending[0].turn_id));
    wait_for(|| sink.completed().is_some()).await;
}

/// **Allow anyway** (04 §6): the block stands in the transcript, marked as overridden, and the
/// work carries on from where it stopped.
#[tokio::test]
async fn allow_anyway_lets_the_blocked_call_through_on_the_next_turn() {
    let blocked = || tool_round("c1", "fake__write", serde_json::json!({ "path": "out" }));
    let m = manager_with(
        vec![
            blocked(),
            vec![text("I was blocked."), end()],
            // The next turn: the model makes the same call again, as the note tells it to.
            tool_round("c2", "fake__write", serde_json::json!({ "path": "out" })),
            vec![text("Written."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    m.guarded(chat.id);
    m.guard_says(&[r#"{"decision":"deny","confidence":0.9,"reason":"Not part of the task"}"#]);
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "write it".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert!(m.fake.calls.lock().unwrap().is_empty(), "the block held");

    let second = Arc::new(Collect::default());
    m.allow_blocked(chat.id, CallId("c1".into()), second.clone())
        .unwrap();
    wait_for(|| second.completed().is_some()).await;

    assert_eq!(
        m.fake.calls.lock().unwrap().len(),
        1,
        "the call the user allowed ran, and it was not put to the guard again"
    );
    assert_eq!(
        m.guard_inputs().len(),
        1,
        "the override answers instead of the guard"
    );
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let first = detail.turns[0]
        .tool_calls
        .iter()
        .find(|c| c.id.as_str() == "c1")
        .unwrap();
    assert_eq!(
        first.status,
        ToolCallStatus::Denied,
        "the block is not rewritten"
    );
    assert!(first.judge.as_ref().unwrap().overridden);
    let allowed = detail.turns[1]
        .tool_calls
        .iter()
        .find(|c| c.id.as_str() == "c2")
        .unwrap();
    assert_eq!(allowed.status, ToolCallStatus::Completed);
    assert_eq!(allowed.decision_source, Some(DecisionSource::UserOnce));
    // The model was told, in a system note, what the user decided.
    let opener: String = detail.turns[1]
        .user
        .parts
        .iter()
        .filter_map(gantry_core::ContentPart::system_text)
        .collect();
    assert!(
        opener.contains("The user has looked at it and allowed it"),
        "{opener}"
    );

    // And the override is spent: a call the guard has not blocked is still the guard's.
    assert!(
        m.allow_blocked(chat.id, CallId("c2".into()), Arc::new(Collect::default()))
            .is_err(),
        "only a call the guard blocked can be allowed anyway"
    );
}

/// 03 §11: a new chat starts with the connectors the user chose for new chats, so the first
/// question does not cost a permission card before anything has happened.
#[tokio::test]
async fn a_new_chat_attaches_the_connectors_chosen_for_new_chats() {
    let mut settings = Settings::default();
    settings.chat.default_connectors = vec!["fake".into(), "not-installed".into()];
    let m = manager_with(vec![vec![text("hi"), end()]], Duration::ZERO, settings);
    install_fake(m.chats().store());

    let chat = m.create_chat(None).unwrap();
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "hello".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    let names: Vec<String> = m.requests()[0]
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert!(
        names.contains(&"fake__echo".to_owned()),
        "the chat can use it from its first message: {names:?}"
    );
    // A namespace that is not installed is skipped rather than failing the chat.
    assert!(m.chats().get(chat.id).unwrap().is_some());
}

/// An incognito chat carries no memory, and nothing that re-freezes its prompt may hand it any
/// (15 A21). Changing the mode from its composer does exactly that re-freeze, which is the way
/// the promise would have been broken quietly: the chat would go on saying it keeps nothing
/// while carrying the user's standing preferences into the provider.
#[tokio::test]
async fn a_mode_change_never_gives_an_incognito_chat_the_memory_core_set() {
    let h = manager(vec![text("ok")], Duration::ZERO);
    let store = h.m.chats().store().clone();
    let memories = gantry_agent::Memories::new(store);
    memories
        .create(
            "Answers should always be in Norwegian",
            gantry_core::MemoryKind::Instruction,
            gantry_core::MemoryScopeKind::Global,
            None,
            gantry_core::MemorySource::User,
            None,
        )
        .unwrap();

    let ordinary =
        h.m.create_session(gantry_core::Surface::Chat, Vec::new(), None, false, None)
            .unwrap();
    let private =
        h.m.create_session(gantry_core::Surface::Chat, Vec::new(), None, true, None)
            .unwrap();
    let carries = |id: ChatId| {
        h.m.chats()
            .system_prompt(id)
            .unwrap()
            .expect("the chat exists")
            .0
            .contains("Answers should always be in Norwegian")
    };
    assert!(
        carries(ordinary.id),
        "an ordinary chat freezes the core set"
    );
    assert!(!carries(private.id));

    // The re-freeze path: a mode change on a chat that has not spoken yet.
    for id in [ordinary.id, private.id] {
        h.m.update_chat(
            id,
            ChatPatch {
                mode: Some(Mode::Plan),
                ..Default::default()
            },
        )
        .unwrap();
    }
    assert!(carries(ordinary.id));
    assert!(
        !carries(private.id),
        "the re-freeze handed an incognito chat the memory it exists to do without"
    );
}

/// A view that reattaches while a command is still running sees what it has printed so far
/// (05 §3). `tool_call.output` is transient — nothing replays it — so before the snapshot
/// carried a tail, opening a chat in the middle of a two-minute build showed a row with no
/// output at all until the command finished.
#[tokio::test]
async fn a_late_subscriber_sees_what_a_running_command_has_printed() {
    let m = manager_with(
        vec![
            tool_round("call_1", "fake__stream", serde_json::json!({})),
            vec![text("built"), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let hold = Arc::new(tokio::sync::Notify::new());
    *m.fake.hold.lock().unwrap() = Some(hold.clone());

    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    let turn = m
        .start(
            chat.id,
            "build it".into(),
            Vec::new(),
            Vec::new(),
            sink.clone(),
        )
        .unwrap();
    wait_for(|| sink.names().contains(&"tool_call.output")).await;

    let late = Arc::new(Collect::default());
    m.subscribe(turn, 0, late.clone()).unwrap();
    let AgentEventKind::TurnSnapshot { snapshot } = late.kinds()[0].clone() else {
        panic!("snapshot first");
    };
    let tail = snapshot
        .output
        .values()
        .next()
        .expect("the running call's output belongs in the snapshot")
        .clone();
    assert!(
        tail.iter().any(|l| l.contains("compiling gantry-agent")),
        "the tail should carry what was printed: {tail:?}"
    );

    hold.notify_waiters();
    wait_for(|| sink.completed().is_some()).await;

    // Once the call has ended its result carries the output, so the tail is dropped rather
    // than sent a second time in a different shape.
    let state = m.snapshot_output(turn);
    assert!(state.is_none() || state.unwrap().is_empty());
}

/// **Stop** while a tool is running (05 §7): the turn ends, the call is recorded as cancelled
/// rather than left running for ever, and the connector is not waited out. This is the guarantee
/// every connector rests on — a connector that cannot interrupt its own work still stops at the
/// next await, and one that never awaits at all has at least not been started.
#[tokio::test]
async fn stopping_a_turn_cancels_the_tool_call_it_was_waiting_on() {
    let m = manager_with(
        vec![
            tool_round("call_1", "fake__stream", serde_json::json!({})),
            vec![text("never sent"), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    // Never released: the call is still in the connector when the user presses Stop.
    *m.fake.hold.lock().unwrap() = Some(Arc::new(tokio::sync::Notify::new()));

    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    let turn = m
        .start(
            chat.id,
            "build it".into(),
            Vec::new(),
            Vec::new(),
            sink.clone(),
        )
        .unwrap();
    wait_for(|| sink.names().contains(&"tool_call.output")).await;

    assert!(m.cancel(turn), "the turn was there to stop");
    wait_for(|| sink.completed() == Some(TurnStatus::Cancelled)).await;

    let completed = sink.kinds().into_iter().find_map(|k| match k {
        AgentEventKind::ToolCallCompleted { status, result, .. } => Some((status, result)),
        _ => None,
    });
    let (status, result) = completed.expect("the call it was waiting on is closed, not left open");
    assert_eq!(status, ToolCallStatus::Cancelled);
    assert!(
        gantry_core::result_preview(&result, usize::MAX).contains("Cancelled"),
        "and the transcript says why: {result:?}"
    );
}

/// Plan mode's read prompt offers **Allow all reads for this chat** (04 §4), and a folder
/// narrower than that when the call names a file. The roadmap carried this as unbuilt work; it
/// was reachable from the tier rule all along, and this is what says so.
#[tokio::test]
async fn a_read_prompt_in_plan_mode_offers_all_reads_and_a_folder() {
    let m = manager_with(
        vec![tool_round(
            "call_1",
            "fake__echo",
            serde_json::json!({ "path": "/home/olav/dev/gantry/Cargo.toml" }),
        )],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    m.update_chat(
        chat.id,
        ChatPatch {
            mode: Some(Mode::Plan),
            ..Default::default()
        },
    )
    .unwrap();
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "read it".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;

    let pending = m.interactions().list_pending(Some(chat.id));
    let gantry_core::InteractionPayload::Permission { request } = &pending[0].payload else {
        panic!("not a permission prompt");
    };
    assert_eq!(
        request.scopes,
        vec![
            gantry_core::GrantScope::PathPrefix {
                prefix: "/home/olav/dev/gantry".into()
            },
            gantry_core::GrantScope::Tool,
            gantry_core::GrantScope::AllReads,
        ],
        "narrowest first: this folder, this tool, every read"
    );

    // Taking the folder grant answers the next read under it without asking again, and leaves
    // a read somewhere else to ask.
    m.resolve_interaction(
        pending[0].id,
        InteractionResolution::Permission {
            decision: PermissionDecision::AllowChat {
                scope: gantry_core::GrantScope::PathPrefix {
                    prefix: "/home/olav/dev/gantry".into(),
                },
            },
            message: None,
            chosen: Default::default(),
        },
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    let grants = m.chats().grants(chat.id).unwrap();
    assert_eq!(grants.len(), 1);
    assert!(grants[0].covers(
        "fake",
        "echo",
        RiskTier::Read,
        &serde_json::json!({ "path": "/home/olav/dev/gantry/src/main.rs" })
    ));
    assert!(!grants[0].covers(
        "fake",
        "echo",
        RiskTier::Read,
        &serde_json::json!({ "path": "/etc/passwd" })
    ));
}

/// 03 §4: a tool may contribute to the answer, not only to its own result. The picture lands in
/// the reply at the point the call happened, its bytes go to the blob store rather than into the
/// transcript, and the live event carries them so it appears at once.
#[tokio::test]
async fn a_tool_can_put_a_picture_in_the_answer() {
    let m = manager_with(
        vec![
            tool_round("call_1", "fake__paint", serde_json::json!({})),
            vec![text("There it is."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "draw something".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));

    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let t = &detail.turns[0];
    assert_eq!(
        t.messages.len(),
        4,
        "assistant, tool, the picture, assistant"
    );
    assert_eq!(t.messages[2].role, Role::Assistant);
    let ContentPart::Image { source, mime } = &t.messages[2].parts[0] else {
        panic!(
            "the third message is the picture: {:?}",
            t.messages[2].parts
        );
    };
    assert_eq!(mime, "image/png");
    let gantry_core::MediaSource::Blob { hash } = source else {
        panic!("what is written down is a hash, not the bytes: {source:?}");
    };
    assert_eq!(hash.len(), 64);

    // The live event is the other half: it carries the bytes, so the picture is on screen
    // before anything has been read back out of the store.
    let live = sink.kinds().into_iter().find_map(|k| match k {
        AgentEventKind::BlockDone {
            part: ContentPart::Image { source, .. },
            ..
        } => Some(source),
        _ => None,
    });
    assert!(
        matches!(live, Some(gantry_core::MediaSource::Base64 { data }) if data == ONE_PIXEL),
        "the live event carries the picture itself"
    );

    // The transcript carries it, because the transcript is what the chat is. What each provider
    // does with an assistant message of media — nothing — is checked where the projections are
    // (`gantry-providers/tests/answer_media.rs`).
    let second = m.requests()[1].clone();
    assert_eq!(
        second.messages.last().map(|msg| msg.role),
        Some(Role::Assistant),
        "the picture is the last thing before the next round"
    );
}

/// A card that offers a choice runs the call the user was looking at (04 §7).
///
/// This is the whole point of the mechanism: before it, a card naming the wrong model could only
/// be denied, which cost a round trip through the chat model to say "use that one instead". The
/// row is updated too — the activity has to be about the call that happened, not about the one
/// the model asked for.
#[tokio::test]
async fn a_card_can_change_an_argument_before_the_call_runs() {
    let m = manager_with(
        vec![
            tool_round(
                "c1",
                "fake__write",
                serde_json::json!({ "target": "draft" }),
            ),
            vec![text("Written."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    manual(&m, chat.id);
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();

    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;
    let pending = m.interactions().list_pending(Some(chat.id));
    let gantry_core::InteractionPayload::Permission { request } = &pending[0].payload else {
        panic!("a permission card");
    };
    assert_eq!(request.choices.len(), 1);
    assert_eq!(request.choices[0].key, "target");
    assert_eq!(request.choices[0].value.as_deref(), Some("draft"));

    m.resolve_interaction(
        pending[0].id,
        InteractionResolution::Permission {
            decision: PermissionDecision::AllowOnce,
            message: None,
            chosen: std::collections::BTreeMap::from([
                ("target".to_owned(), "final".to_owned()),
                // Not offered, so not applied: a card picks between things the connector
                // already called equivalent, and cannot widen the call.
                ("path".to_owned(), "/etc/passwd".to_owned()),
            ]),
        },
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    let calls = m.fake.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].args["target"], "final");
    assert!(calls[0].args.get("path").is_none(), "{:?}", calls[0].args);

    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let row = &detail.turns[0].tool_calls[0];
    assert_eq!(
        row.args["target"], "final",
        "the row is about the call that happened"
    );
}

/// A value the card did not offer changes nothing — including when nothing else does either.
#[tokio::test]
async fn a_choice_the_card_never_offered_is_ignored() {
    let m = manager_with(
        vec![
            tool_round(
                "c1",
                "fake__write",
                serde_json::json!({ "target": "draft" }),
            ),
            vec![text("Written."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    manual(&m, chat.id);
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;
    let pending = m.interactions().list_pending(Some(chat.id));
    m.resolve_interaction(
        pending[0].id,
        InteractionResolution::Permission {
            decision: PermissionDecision::AllowOnce,
            message: None,
            chosen: std::collections::BTreeMap::from([(
                "target".to_owned(),
                "whatever-i-typed".to_owned(),
            )]),
        },
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    let calls = m.fake.calls.lock().unwrap().clone();
    assert_eq!(calls[0].args["target"], "draft");
}

/// One sub agent, end to end (18 A1, A2, A6): the parent calls, waits, and gets one report; the
/// work happens in a chat of its own that no list shows.
#[tokio::test]
async fn a_sub_agent_does_the_work_and_hands_back_one_report() {
    let m = manager_with(
        vec![
            tool_round(
                "call_1",
                "subagents__run",
                serde_json::json!({ "agent": "researcher", "task": "what is a gantry crane" }),
            ),
            // The sub agent's own turn, on the same scripted provider.
            vec![text("A gantry crane rides on legs over a span."), end()],
            vec![text("It rides on legs over a span."), end()],
        ],
        Duration::ZERO,
        Settings::default(),
    );
    let chat = m.chat();
    with_sub_agents(&m, chat.id);
    let sink = Arc::new(Collect::default());
    m.start(
        chat.id,
        "ask a researcher what a gantry crane is".into(),
        Vec::new(),
        Vec::new(),
        sink.clone(),
    )
    .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));

    // What came back to the parent is the sub agent's last message and nothing else.
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    let call = &detail.turns[0].tool_calls[0];
    assert_eq!(call.status, ToolCallStatus::Completed);
    let preview = call.result_preview.clone().unwrap_or_default();
    assert!(preview.contains("rides on legs"), "{preview}");

    // And beside the report, what it cost: the row in the parent's chat and the turn footer
    // read these, and a number kept only in `structured` reaches neither — the runner keeps a
    // call's content and drops the rest.
    let accounting = call
        .result
        .as_ref()
        .expect("the call kept its result")
        .iter()
        .find_map(|p| match p {
            gantry_core::ResultPart::Json { json } => Some(json.clone()),
            _ => None,
        })
        .expect("the result carries what the sub agent cost");
    assert_eq!(accounting["agent"], "Researcher");
    assert_eq!(accounting["status"], "completed");
    assert!(accounting["transcript"].is_string(), "{accounting}");
    assert!(accounting["seconds"].is_number(), "{accounting}");

    // And the sub agent's own steps are not in the parent's transcript: the user reads what
    // their model said, not the forty pages somebody had to read to answer it (18 A6).
    let parent_text = detail.turns[0].assistant_text();
    assert!(
        !parent_text.contains("A gantry crane rides"),
        "{parent_text}"
    );

    // The transcript exists, belongs to the parent's turn, and is in no list (18 A1).
    let store = m.chats().store().clone();
    let hidden: Vec<(String, Option<String>)> = store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT agent_type, parent_turn_id FROM chats WHERE parent_turn_id IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .unwrap();
    assert_eq!(hidden.len(), 1, "one sub agent ran");
    assert_eq!(hidden[0].0, "researcher");
    assert_eq!(
        hidden[0].1.as_deref(),
        Some(detail.turns[0].id.to_string().as_str())
    );
    let listed = m.chats().list(gantry_core::Surface::Chat).unwrap();
    assert_eq!(listed.len(), 1, "the sidebar shows the conversation only");

    // The one list that does show it: the tree the parent's line opens (18 §7). It names the
    // type, the task in the parent's own words, and what the turn cost, because the whole
    // point of the modal is answering "what did it do and what did it cost".
    let tree = m.chats().sub_agents(detail.turns[0].id).unwrap();
    assert_eq!(tree.len(), 1);
    assert_eq!(tree[0].agent, "researcher");
    assert_eq!(tree[0].name, "Researcher");
    assert_eq!(tree[0].task, "what is a gantry crane");
    assert_eq!(tree[0].status, TurnStatus::Completed);
    assert_eq!(tree[0].chat_id, hidden_id(&store));
    assert!(tree[0].ended_at.is_some(), "it finished before the parent");

    // And the transcript itself reads back like any other chat, which is what makes the tree
    // openable: no second projection, the same turn view the chat uses.
    let inside = m.chats().get(tree[0].chat_id).unwrap().unwrap();
    assert!(inside.turns[0].assistant_text().contains("A gantry crane"));
}

/// The id of the one sub-agent chat in the store, for the assertions above.
fn hidden_id(store: &gantry_store::Store) -> gantry_core::ChatId {
    store
        .read(|c| {
            let id: String = c.query_row(
                "SELECT id FROM chats WHERE parent_turn_id IS NOT NULL",
                [],
                |r| r.get(0),
            )?;
            Ok(id)
        })
        .unwrap()
        .parse()
        .unwrap()
}

/// Stopping the parent stops what it started (18 §9). Without this a cancelled turn leaves a
/// model running somewhere, spending money on an answer nobody will read.
#[tokio::test]
async fn stopping_the_parent_stops_the_sub_agent() {
    let mut deltas: Script = vec![];
    for i in 0..50 {
        deltas.push(text(&format!("w{i} ")));
    }
    deltas.push(end());
    let m = manager_with(
        vec![
            tool_round(
                "call_1",
                "subagents__run",
                serde_json::json!({ "agent": "researcher", "task": "read everything" }),
            ),
            deltas,
        ],
        Duration::from_millis(20),
        Settings::default(),
    );
    let chat = m.chat();
    with_sub_agents(&m, chat.id);
    let sink = Arc::new(Collect::default());
    let turn = m
        .start(
            chat.id,
            "delegate it".into(),
            Vec::new(),
            Vec::new(),
            sink.clone(),
        )
        .unwrap();

    // Wait until the sub agent is actually running, so the test is about cancelling it rather
    // than about cancelling before it started.
    let store = m.chats().store().clone();
    let running = || {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT count(*) FROM chats WHERE parent_turn_id IS NOT NULL",
                    [],
                    |r| r.get::<_, i64>(0),
                )?)
            })
            .unwrap()
            > 0
    };
    wait_for(running).await;
    assert!(m.cancel(turn), "the parent turn was running");
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Cancelled));

    // Cancellation travels, it does not teleport: the sub agent's own runner has to unwind and
    // write its partial message before its turn is final.
    let statuses = || {
        store
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT t.status FROM turns t JOIN chats ch ON ch.id = t.chat_id
                     WHERE ch.parent_turn_id IS NOT NULL",
                )?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row?);
                }
                Ok(out)
            })
            .unwrap()
    };
    wait_for(|| statuses() == vec!["cancelled".to_owned()]).await;
    assert_eq!(
        statuses(),
        vec!["cancelled".to_owned()],
        "the tree was cancelled"
    );
}

/// A sub-agent type that may not change anything is shown no tool that could (18 §3).
///
/// A filter rather than a refusal, for the reason Plan mode hides the tools it would deny: a
/// tool that is not in the list cannot be reached for, and the model does not spend a round
/// finding that out.
#[tokio::test]
async fn a_read_only_sub_agent_is_not_shown_the_tools_that_write() {
    let m = manager(vec![end()], Duration::ZERO);
    let attached = vec!["fake".to_owned()];
    let full = gantry_agent::ToolSet::assemble(&m.registry, Mode::Auto, &attached, true, false)
        .await
        .specs();
    let read_only = gantry_agent::ToolSet::assemble(&m.registry, Mode::Auto, &attached, true, true)
        .await
        .specs();
    let names =
        |set: &[gantry_providers::ToolSpec]| set.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
    assert!(names(&full).contains(&"fake__write".to_owned()));
    assert!(
        !names(&read_only).contains(&"fake__write".to_owned()),
        "{:?}",
        names(&read_only)
    );
    assert!(names(&read_only).contains(&"fake__echo".to_owned()));
}
