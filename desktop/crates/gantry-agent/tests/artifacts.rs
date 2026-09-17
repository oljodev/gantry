//! The artifact service and its runtime tools (docs/plan/13): versions, edits, reads across
//! chats, the render handshake, and the tool surface the model sees.

use std::{sync::Arc, time::Duration};

use gantry_agent::{
    Artifacts, ChatBook, NewChat, RuntimeTools,
    artifacts::{CreateRequest, Edit, Origin},
};
use gantry_connectors::{
    ChatScope, Connector, NoopToolEvents, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::{
    AgentEventKind, CallId, ChatId, Mode, ModelRef, ReasoningEffort, RenderReport, RenderStatus,
    Surface, TurnId, VersionSource,
};
use gantry_store::{BlobStore, Store};
use tokio_util::sync::CancellationToken;

fn setup() -> (tempfile::TempDir, Arc<ChatBook>, Arc<Artifacts>) {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());
    let book = Arc::new(ChatBook::new(store.clone(), blobs.clone()));
    let artifacts =
        Arc::new(Artifacts::new(store, blobs).with_render_timeout(Duration::from_millis(50)));
    (dir, book, artifacts)
}

fn chat(book: &ChatBook) -> ChatId {
    book.create(NewChat {
        surface: Surface::Chat,
        roots: Vec::new(),
        model: ModelRef::default_model(),
        mode: Mode::AutoEdit,
        guard: true,
        effort: ReasoningEffort::Off,
        system_snapshot: String::new(),
        system_snapshot_version: 1,
        connectors: Vec::new(),
        incognito: false,
        project: None,
        grants: Vec::new(),
        parent: None,
    })
    .unwrap()
    .id
}

#[test]
fn create_update_edit_read_and_restore_make_versions() {
    let (_d, book, artifacts) = setup();
    let chat_id = chat(&book);
    let a = artifacts
        .create(
            chat_id,
            CreateRequest {
                artifact_type: "markdown".into(),
                title: "Plan".into(),
                language: None,
                content: "# Plan\n\nStep one.\n".into(),
                summary: Some("A plan".into()),
            },
            Origin::default(),
        )
        .unwrap();
    assert_eq!(a.current_version, 1);
    let (_, v2) = artifacts
        .edit(
            a.id,
            &[Edit {
                old_string: "Step one.".into(),
                new_string: "Step one, then two.".into(),
                replace_all: false,
            }],
            None,
            Origin::default(),
        )
        .unwrap();
    assert_eq!(v2, 2);
    let (_, v3) = artifacts
        .update(
            a.id,
            "# Plan v3\n".into(),
            Some("Plan v3".into()),
            None,
            Origin::default(),
        )
        .unwrap();
    assert_eq!(v3, 3);
    let (_, v4) = artifacts
        .save_user_version(a.id, "# Mine\n".into())
        .unwrap();
    let (_, v5) = artifacts.restore(a.id, 2).unwrap();
    assert_eq!((v4, v5), (4, 5));
    let current = artifacts.read(a.id, None).unwrap();
    assert_eq!(current.version, 5);
    assert_eq!(current.content, "# Plan\n\nStep one, then two.\n");
    assert_eq!(current.artifact.title, "Plan v3");
    let sources: Vec<VersionSource> = current.versions.iter().map(|v| v.source).collect();
    assert_eq!(
        sources,
        [
            VersionSource::ModelCreate,
            VersionSource::ModelEdit,
            VersionSource::ModelUpdate,
            VersionSource::UserEdit,
            VersionSource::UserRestore,
        ]
    );
    let old = artifacts.read(a.id, Some(3)).unwrap();
    assert_eq!(old.content, "# Plan v3\n");
    let err = artifacts
        .edit(
            a.id,
            &[Edit {
                old_string: "nope".into(),
                new_string: "x".into(),
                replace_all: false,
            }],
            None,
            Origin::default(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("not found"));
    assert!(
        artifacts
            .create(
                chat_id,
                CreateRequest {
                    artifact_type: "table".into(),
                    title: "T".into(),
                    language: None,
                    content: String::new(),
                    summary: None,
                },
                Origin::default(),
            )
            .is_err(),
        "unknown types are refused"
    );
}

#[test]
fn another_chat_may_not_write_and_deleting_the_chat_removes_the_artifact() {
    let (_d, book, artifacts) = setup();
    let owner = chat(&book);
    let other = chat(&book);
    let a = artifacts
        .create(
            owner,
            CreateRequest {
                artifact_type: "code".into(),
                title: "main.rs".into(),
                language: Some("rust".into()),
                content: "fn main() {}".into(),
                summary: None,
            },
            Origin::default(),
        )
        .unwrap();
    assert!(artifacts.writable(a.id, owner).is_ok());
    assert!(artifacts.writable(a.id, other).is_err());
    assert!(
        artifacts.readable(a.id, other).is_err(),
        "no project in common"
    );
    assert!(book.delete(owner).unwrap());
    assert!(artifacts.get(a.id).is_err());
}

#[tokio::test]
async fn the_render_handshake_completes_or_times_out() {
    let (_d, book, artifacts) = setup();
    let chat_id = chat(&book);
    let a = artifacts
        .create(
            chat_id,
            CreateRequest {
                artifact_type: "react".into(),
                title: "Dash".into(),
                language: None,
                content: "export default () => null".into(),
                summary: None,
            },
            Origin::default(),
        )
        .unwrap();
    // Nobody reports: pending after the (shortened) cap.
    let report = artifacts.await_render(a.id, 1, "react").await;
    assert_eq!(report.status, RenderStatus::Pending);
    // A report while waiting completes the wait.
    let waiter = {
        let artifacts = artifacts.clone();
        let id = a.id;
        tokio::spawn(async move { artifacts.await_render(id, 1, "react").await })
    };
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert!(artifacts.report_render(a.id, 1, RenderReport::ok()));
    assert_eq!(waiter.await.unwrap().status, RenderStatus::Ok);
    assert!(
        !artifacts.report_render(a.id, 1, RenderReport::ok()),
        "nobody waits any more"
    );
    // Parent-rendered types are ok at once.
    assert_eq!(
        artifacts.await_render(a.id, 1, "markdown").await.status,
        RenderStatus::Ok
    );
}

struct Capture(std::sync::Mutex<Vec<AgentEventKind>>);

impl ToolEventSink for Capture {
    fn event(&self, event: AgentEventKind) {
        self.0.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn the_runtime_tools_create_read_and_edit_with_events() {
    let (_d, book, artifacts) = setup();
    let chat_id = chat(&book);
    let tools = RuntimeTools::with_artifacts(artifacts.clone());
    let names: Vec<String> = tools
        .tools()
        .await
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(
        names,
        [
            "clock",
            "create_artifact",
            "update_artifact",
            "edit_artifact",
            "read_artifact"
        ]
    );
    let sink = Arc::new(Capture(Default::default()));
    let call = |tool: &str, args: serde_json::Value| ToolCallRequest {
        call_id: CallId::new(),
        tool: tool.into(),
        args,
        scope: ChatScope {
            chat_id,
            turn_id: TurnId::new(),
            mode: Mode::AutoEdit,
            attach_decided: false,
        },
    };
    let created = tools
        .call(
            call(
                "create_artifact",
                serde_json::json!({ "type": "markdown", "title": "Notes", "content": "# Notes\n" }),
            ),
            sink.clone(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let ToolOutcome::Complete {
        structured: Some(v),
        is_error: false,
        ..
    } = created
    else {
        panic!("{created:?}");
    };
    assert_eq!(v["version"], 1);
    assert_eq!(v["render"]["status"], "ok");
    let id = v["artifact_id"].as_str().unwrap().to_owned();
    assert!(matches!(
        sink.0.lock().unwrap().as_slice(),
        [AgentEventKind::ArtifactCreated { title, .. }] if title == "Notes"
    ));

    let edited = tools
        .call(
            call(
                "edit_artifact",
                serde_json::json!({ "artifact_id": id, "edits": [{ "old_string": "# Notes", "new_string": "# Notes!" }] }),
            ),
            sink.clone(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let ToolOutcome::Complete {
        structured: Some(v),
        ..
    } = edited
    else {
        panic!()
    };
    assert_eq!(v["version"], 2);

    let read = tools
        .call(
            call("read_artifact", serde_json::json!({ "artifact_id": id })),
            Arc::new(NoopToolEvents),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let ToolOutcome::Complete {
        structured: Some(v),
        ..
    } = read
    else {
        panic!()
    };
    assert_eq!(v["content"], "# Notes!\n");
    assert_eq!(v["versions"].as_array().unwrap().len(), 2);

    let bad = tools
        .call(
            call(
                "read_artifact",
                serde_json::json!({ "artifact_id": "nope" }),
            ),
            Arc::new(NoopToolEvents),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert!(matches!(bad, ToolOutcome::Complete { is_error: true, .. }));
}
