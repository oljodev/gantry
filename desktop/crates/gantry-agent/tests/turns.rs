//! The turn loop on a scripted provider: happy path, cancel, mid-stream error, snapshot.

use std::{
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use gantry_agent::{ChatBook, ChatNotifier, EventSink, PromptContext, ProviderSource, TurnManager};
use gantry_core::{
    AgentEventBatch, AgentEventKind, ChatId, ContentPart, ProviderErrorKind, ProviderId,
    ProviderKind, Settings, StopReason, TurnStatus, Usage,
};
use gantry_providers::{
    ChatRequest, ChatStream, KeyInfo, ModelInfo, Provider, ProviderError, StreamEvent,
};
use gantry_store::{BlobStore, Store};

struct Scripted {
    id: ProviderId,
    events: Vec<Result<StreamEvent, ProviderError>>,
    delay: Duration,
    /// Requests seen, newest last (the title generator sends a second one).
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
            self.events.clone()
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
struct Notes(Mutex<Vec<ChatId>>);

impl ChatNotifier for Notes {
    fn chats_changed(&self, ids: Vec<ChatId>) {
        self.0.lock().unwrap().extend(ids);
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
    notes: Arc<Notes>,
}

impl std::ops::Deref for Harness {
    type Target = Arc<TurnManager>;
    fn deref(&self) -> &Arc<TurnManager> {
        &self.m
    }
}

fn manager(events: Vec<Result<StreamEvent, ProviderError>>, delay: Duration) -> Harness {
    let provider = Arc::new(Scripted {
        id: ProviderId::openrouter(),
        events,
        delay,
        requests: Mutex::new(Vec::new()),
    });
    let (dir, chats) = book();
    let m = TurnManager::new(
        chats,
        Arc::new(Source(provider.clone())),
        Arc::new(RwLock::new(Settings::default())),
        PromptContext::default(),
        tokio::runtime::Handle::current(),
    );
    let notes = Arc::new(Notes::default());
    m.set_notifier(notes.clone());
    Harness {
        _dir: dir,
        m,
        provider,
        notes,
    }
}

/// Text lands in block 1; block 0 is where a reasoning model puts its thinking.
fn text(t: &str) -> Result<StreamEvent, ProviderError> {
    Ok(StreamEvent::TextDelta {
        index: 1,
        text: t.into(),
    })
}

async fn wait_for<F: Fn() -> bool>(f: F) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition not met in time");
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
            Ok(StreamEvent::MessageEnd {
                stop_reason: StopReason::EndTurn,
            }),
        ],
        Duration::ZERO,
    );
    let chat = m.create_chat(None).unwrap();
    let sink = Arc::new(Collect::default());
    let turn = m
        .start(chat.id, "Hi there".into(), Vec::new(), sink.clone())
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;

    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
    assert_eq!(sink.text(), "Hello");
    // The first exchange names the chat with a second, tiny request.
    wait_for(|| m.provider.requests.lock().unwrap().len() == 2).await;
    wait_for(|| m.notes.0.lock().unwrap().len() >= 2).await;
    let detail = m.chats().get(chat.id).unwrap().unwrap();
    assert_eq!(detail.title, "A generated title");
    let title_req = m.provider.requests.lock().unwrap()[1].clone();
    assert_eq!(title_req.model, "deepseek/deepseek-v4-flash");
    assert!(title_req.messages[0].text().contains("Hi there"));
    let persisted = m
        .chats()
        .store()
        .read(|c| gantry_store::repos::events::list_for_turn(c, turn))
        .unwrap();
    let kinds: Vec<String> = persisted
        .iter()
        .map(|e| {
            serde_json::to_value(&e.event).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
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
    let a = t.assistant.as_ref().unwrap();
    assert!(matches!(&a.parts[0], ContentPart::Thinking { text, .. } if text == "hmm"));
    assert!(matches!(&a.parts[1], ContentPart::Text { text } if text == "Hello"));
    assert!(m.list_active().is_empty());
    assert!(sink.kinds().iter().any(|k| matches!(
        k,
        AgentEventKind::MessageCompleted {
            stop_reason: StopReason::EndTurn,
            ..
        }
    )));
}

#[tokio::test]
async fn cancel_keeps_the_partial_text() {
    let events: Vec<_> = (0..50)
        .map(|i| text(&format!("w{i} ")))
        .chain([Ok(StreamEvent::MessageEnd {
            stop_reason: StopReason::EndTurn,
        })])
        .collect();
    let m = manager(events, Duration::from_millis(20));
    let chat = m.create_chat(None).unwrap();
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
    let kept = detail.turns[0].assistant.as_ref().unwrap().text();
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
    let chat = m.create_chat(None).unwrap();
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
    assert_eq!(detail.turns[0].assistant.as_ref().unwrap().text(), "Part");
}

#[tokio::test]
async fn a_late_subscriber_gets_a_snapshot_then_live_events() {
    let events: Vec<_> = (0..30)
        .map(|i| text(&format!("{i} ")))
        .chain([Ok(StreamEvent::MessageEnd {
            stop_reason: StopReason::EndTurn,
        })])
        .collect();
    let m = manager(events, Duration::from_millis(15));
    let chat = m.create_chat(None).unwrap();
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
    let snap_text = match &snapshot.parts[0] {
        ContentPart::Text { text } => text.clone(),
        _ => panic!(),
    };
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
            .assistant
            .is_none()
    );
}

/// Tauri commands run on the UI thread, outside every runtime; starting a turn there must work.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_can_be_started_from_a_plain_thread() {
    let m = manager(
        vec![
            text("ok"),
            Ok(StreamEvent::MessageEnd {
                stop_reason: StopReason::EndTurn,
            }),
        ],
        Duration::ZERO,
    );
    let chat = m.create_chat(None).unwrap();
    let sink = Arc::new(Collect::default());
    let (m2, sink2) = (m.clone(), sink.clone());
    std::thread::spawn(move || m2.start(chat.id, "go".into(), Vec::new(), sink2).unwrap())
        .join()
        .unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
}
