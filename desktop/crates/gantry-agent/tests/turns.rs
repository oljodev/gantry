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
    InteractionResolution, InteractionStatus, Mode, PermissionDecision, ProviderErrorKind,
    ProviderId, ProviderKind, RiskTier, Role, Settings, StopReason, ToolCallStatus, ToolDef,
    TurnStatus, Usage,
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
    async fn check_key(&self) -> Result<KeyInfo, ProviderError> {
        Ok(KeyInfo::default())
    }
    async fn stream(&self, req: ChatRequest) -> Result<ChatStream, ProviderError> {
        let title_request = req.system.starts_with("You name conversations");
        self.requests.lock().unwrap().push(req);
        let delay = self.delay;
        let events = if title_request {
            vec![
                Ok(StreamEvent::TextDelta {
                    index: 0,
                    text: "\"A generated title.\"".into(),
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
            "boom" => Err(ConnectorError::Failed("kaboom".into())),
            other => Err(ConnectorError::UnknownTool(other.into())),
        }
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

    /// A chat with the fake connector installed and attached, which is what a chat looks like
    /// once the user has added a connector to it (03 §11). A chat with nothing attached sees
    /// only the runtime tools, which is what `a_chat_sees_only_what_it_attached` checks.
    fn chat(&self) -> gantry_core::ChatSummary {
        let chat = self.m.create_chat(None).unwrap();
        attach_fake(self.m.chats().store(), chat.id);
        chat
    }
}

/// Installs the fake connector as an instance and attaches it to one chat.
fn attach_fake(store: &Arc<gantry_store::Store>, chat_id: gantry_core::ChatId) {
    use gantry_store::repos::connectors::{self, NewInstance};
    let id = gantry_core::InstanceId::new();
    store
        .write_blocking(move |c| {
            if connectors::get(c, id)?.is_none()
                && !connectors::namespaces(c)?.iter().any(|n| n == "fake")
            {
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
                connectors::attach(c, chat_id, id, "user")?;
                return Ok(());
            }
            let existing = connectors::list(c)?;
            let instance = existing
                .iter()
                .find(|i| i.namespace == "fake")
                .expect("the fake instance");
            connectors::attach(c, chat_id, instance.id, "user")
        })
        .unwrap();
}

fn manager_with(rounds: Vec<Script>, delay: Duration, settings: Settings) -> Harness {
    let provider = Arc::new(Scripted {
        id: ProviderId::openrouter(),
        rounds: Mutex::new(rounds.into()),
        delay,
        requests: Mutex::new(Vec::new()),
    });
    let fake = Arc::new(Fake {
        descriptor: ConnectorDescriptor {
            id: "fake".into(),
            name: "Fake".into(),
            instance_id: None,
            first_party: true,
        },
        calls: Mutex::new(Vec::new()),
    });
    let registry = Arc::new(ConnectorRegistry::new());
    registry.register(fake.clone());
    registry.register(Arc::new(gantry_agent::RuntimeTools::new()));
    let (dir, chats) = book();
    let m = TurnManager::new(
        chats,
        Arc::new(Source(provider.clone())),
        registry,
        Arc::new(RwLock::new(settings)),
        PromptContext::default(),
        tokio::runtime::Handle::current(),
    );
    let notes = Arc::new(Notes::default());
    m.set_notifier(notes.clone());
    Harness {
        _dir: dir,
        m,
        provider,
        fake,
        notes,
    }
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
        .start(chat.id, "Hi there".into(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
    assert_eq!(sink.text(), "Hello");
    // The request declared the registered tools.
    let first = m.requests()[0].clone();
    let names: Vec<String> = first.tools.iter().map(|t| t.name.clone()).collect();
    assert_eq!(
        names,
        ["fake__echo", "fake__write", "fake__boom", "gantry__clock"]
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
        .start(chat.id, "go".into(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| !sink.text().is_empty()).await;
    assert!(m.cancel(turn));
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Cancelled));
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(detail.turns[0].status, TurnStatus::Cancelled);
    assert_eq!(detail.turns[0].stop_reason, Some(StopReason::Cancelled));
    let kept = detail.turns[0].assistant_text();
    assert!(kept.starts_with("w0 ") && !kept.contains("w49"));
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
    m.start(chat.id, "go".into(), Vec::new(), sink.clone())
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
        .start(chat.id, "go".into(), Vec::new(), first.clone())
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
    let m = TurnManager::new(
        chats,
        Arc::new(NoSource),
        Arc::new(ConnectorRegistry::new()),
        Arc::new(RwLock::new(Settings::default())),
        PromptContext::default(),
        tokio::runtime::Handle::current(),
    );
    let chat = m.create_chat(None).unwrap();
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), Vec::new(), sink.clone())
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
    std::thread::spawn(move || m2.start(chat.id, "go".into(), Vec::new(), sink2).unwrap())
        .join()
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
}

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
        .start(chat.id, "echo hi".into(), Vec::new(), sink.clone())
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
        .start(chat.id, "what day is it".into(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;

    let pending = m.interactions().list_pending(Some(chat.id));
    assert_eq!(pending.len(), 1);
    let gantry_core::InteractionPayload::Permission { request } = &pending[0].payload;
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
    m.start(chat.id, "go".into(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| !m.interactions().list_pending(Some(chat.id)).is_empty()).await;
    let id = m.interactions().list_pending(Some(chat.id))[0].id;
    m.resolve_interaction(
        id,
        InteractionResolution::Permission {
            decision: PermissionDecision::Deny,
            message: Some("not now, ask me tomorrow".into()),
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
        .start(chat.id, "go".into(), Vec::new(), sink.clone())
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
    m.start(chat.id, "go".into(), Vec::new(), sink.clone())
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
    m.start(chat.id, "loop".into(), Vec::new(), sink.clone())
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
    m.start(chat.id, "go".into(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    let names: Vec<String> = m.requests()[0]
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(names, ["fake__echo", "fake__boom", "gantry__clock"]);
}

/// Installing a connector does not give it to every conversation: a chat sees a connector only
/// once it has attached it (03 §11). The runtime tools are the app's own and are always there.
#[tokio::test]
async fn a_chat_sees_only_the_connectors_it_attached() {
    let m = manager(vec![text("hi"), end()], Duration::ZERO);
    let bare = m.create_chat(None).unwrap();
    let sink = Arc::new(Collect::default());
    m.start(bare.id, "go".into(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    let names: Vec<String> = m.requests()[0]
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(
        names,
        ["gantry__clock"],
        "the fake connector is installed but not attached"
    );
}
