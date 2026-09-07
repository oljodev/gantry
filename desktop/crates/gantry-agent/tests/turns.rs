//! The turn loop on a scripted provider: happy path, cancel, mid-stream error, snapshot.

use std::{
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use gantry_agent::{ChatBook, EventSink, PromptContext, ProviderSource, TurnManager};
use gantry_core::{
    AgentEventBatch, AgentEventKind, ContentPart, ProviderErrorKind, ProviderId, ProviderKind,
    Settings, StopReason, TurnStatus, Usage,
};
use gantry_providers::{
    ChatRequest, ChatStream, KeyInfo, ModelInfo, Provider, ProviderError, StreamEvent,
};

struct Scripted {
    id: ProviderId,
    events: Vec<Result<StreamEvent, ProviderError>>,
    delay: Duration,
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
    async fn stream(&self, _req: ChatRequest) -> Result<ChatStream, ProviderError> {
        let delay = self.delay;
        let events = self.events.clone();
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

fn manager(events: Vec<Result<StreamEvent, ProviderError>>, delay: Duration) -> Arc<TurnManager> {
    let provider = Arc::new(Scripted {
        id: ProviderId::openrouter(),
        events,
        delay,
    });
    TurnManager::new(
        Arc::new(ChatBook::new()),
        Arc::new(Source(provider)),
        Arc::new(RwLock::new(Settings::default())),
        PromptContext::default(),
    )
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
    let chat = m.create_chat(None);
    let sink = Arc::new(Collect::default());
    let turn = m.start(chat.id, "Hi there".into(), sink.clone()).unwrap();
    wait_for(|| sink.completed().is_some()).await;

    assert_eq!(sink.completed(), Some(TurnStatus::Completed));
    assert_eq!(sink.text(), "Hello");
    let detail = m.chats().get(chat.id).unwrap();
    assert_eq!(detail.title, "Hi there");
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
    let chat = m.create_chat(None);
    let sink = Arc::new(Collect::default());
    let turn = m.start(chat.id, "go".into(), sink.clone()).unwrap();
    wait_for(|| !sink.text().is_empty()).await;
    assert!(m.cancel(turn));
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Cancelled));
    let detail = m.chats().get(chat.id).unwrap();
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
    let chat = m.create_chat(None);
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), sink.clone()).unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Failed));
    let err = sink.kinds().into_iter().find_map(|k| match k {
        AgentEventKind::Error {
            message, retryable, ..
        } => Some((message, retryable)),
        _ => None,
    });
    assert_eq!(err, Some(("upstream reset".to_owned(), true)));
    let detail = m.chats().get(chat.id).unwrap();
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
    let chat = m.create_chat(None);
    let first = Arc::new(Collect::default());
    let turn = m.start(chat.id, "go".into(), first.clone()).unwrap();
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
    let m = TurnManager::new(
        Arc::new(ChatBook::new()),
        Arc::new(NoSource),
        Arc::new(RwLock::new(Settings::default())),
        PromptContext::default(),
    );
    let chat = m.create_chat(None);
    let sink = Arc::new(Collect::default());
    m.start(chat.id, "go".into(), sink.clone()).unwrap();
    wait_for(|| sink.completed().is_some()).await;
    assert_eq!(sink.completed(), Some(TurnStatus::Failed));
    assert!(m.chats().get(chat.id).unwrap().turns[0].assistant.is_none());
}
