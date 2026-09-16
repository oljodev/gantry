//! The connector end to end: a real store, real files on disk, and the rules of
//! `docs/connectors/code-editor.md` §5 and §6 as a model would meet them.

use std::sync::Arc;

use gantry_connector_code_editor::CodeEditor;
use gantry_connectors::{ChatScope, Connector, NoopToolEvents, ToolCallRequest, ToolOutcome};
use gantry_core::{
    CallId, ChatId, InstanceId, Mode, ModelRef, ReasoningEffort, ResultPart, Surface, TurnId,
    now_ms,
};
use gantry_store::{BlobStore, Store, repos::chats};
use gantry_workspace::Workspace;
use tokio_util::sync::CancellationToken;

struct Fixture {
    _dir: tempfile::TempDir,
    work: tempfile::TempDir,
    editor: CodeEditor,
    workspace: Arc<Workspace>,
    chat: ChatId,
    turn: TurnId,
}

const FILE: &str = "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n";

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());

    let now = now_ms();
    let chat = ChatId::new();
    let record = chats::ChatRecord {
        surface: Surface::Code,
        id: chat,
        project_id: None,
        title: "edits".into(),
        title_source: "auto".into(),
        pinned: false,
        mode: Mode::AutoEdit,
        guard: true,
        model: ModelRef::default_model(),
        effort: ReasoningEffort::Medium,
        web_search: false,
        instructions: String::new(),
        system_snapshot: "sys".into(),
        system_snapshot_version: 1,
        created_at: now,
        updated_at: now,
        last_message_at: now,
        archived_at: None,
        incognito: false,
    };
    let root = work.path().display().to_string();
    store
        .write_blocking(move |conn| {
            chats::insert(conn, &record)?;
            chats::add_root(conn, chat, &root)
        })
        .unwrap();

    std::fs::write(work.path().join("main.rs"), FILE).unwrap();
    let workspace = Arc::new(Workspace::new(store, blobs, dir.path().join("app-data")));
    Fixture {
        editor: CodeEditor::new("code-editor".into(), InstanceId::new(), workspace.clone()),
        _dir: dir,
        work,
        workspace,
        chat,
        turn: TurnId::new(),
    }
}

impl Fixture {
    fn path(&self, name: &str) -> String {
        self.work.path().join(name).display().to_string()
    }

    /// What `filesystem__read_file` does: read the file and remember it.
    async fn read(&self, name: &str) {
        let roots = self.workspace.roots(self.chat).unwrap();
        let scoped = roots.resolve(&self.path(name)).unwrap();
        self.workspace.read(self.chat, &scoped).await.unwrap();
    }

    async fn call(&self, tool: &str, args: serde_json::Value) -> (String, bool) {
        self.call_with(tool, args, CancellationToken::new()).await
    }

    async fn call_with(
        &self,
        tool: &str,
        args: serde_json::Value,
        cancel: CancellationToken,
    ) -> (String, bool) {
        let outcome = self
            .editor
            .call(
                ToolCallRequest {
                    call_id: CallId::new(),
                    tool: tool.to_owned(),
                    args,
                    scope: ChatScope {
                        chat_id: self.chat,
                        turn_id: self.turn,
                        mode: Mode::AutoEdit,
                        attach_decided: false,
                    },
                },
                Arc::new(NoopToolEvents),
                cancel,
            )
            .await
            .expect("a refusal is a result, not an error");
        let ToolOutcome::Complete {
            content, is_error, ..
        } = outcome;
        let text = content
            .iter()
            .map(|part| match part {
                ResultPart::Text { text } => text.clone(),
                ResultPart::Json { json } => json.to_string(),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        (text, is_error)
    }

    fn on_disk(&self, name: &str) -> String {
        std::fs::read_to_string(self.work.path().join(name)).unwrap()
    }
}

#[tokio::test]
async fn a_file_must_be_read_before_it_is_edited() {
    let f = fixture();
    let (message, is_error) = f
        .call(
            "replace",
            serde_json::json!({ "path": f.path("main.rs"), "old": "let x = 1;", "new": "let x = 2;" }),
        )
        .await;
    assert!(is_error, "{message}");
    assert!(message.contains("has not been read"), "{message}");
    assert!(message.contains("filesystem__read_file"), "{message}");
    assert_eq!(f.on_disk("main.rs"), FILE, "nothing was written");
}

#[tokio::test]
async fn a_replace_changes_the_file_and_reports_its_hunks() {
    let f = fixture();
    f.read("main.rs").await;
    let (message, is_error) = f
        .call(
            "replace",
            serde_json::json!({ "path": f.path("main.rs"), "old": "let x = 1;", "new": "let x = 2;" }),
        )
        .await;
    assert!(!is_error, "{message}");
    let result: serde_json::Value = serde_json::from_str(&message).unwrap();
    assert_eq!(result["added"], 1);
    assert_eq!(result["removed"], 1);
    assert!(
        result["hunks"][0]["text"]
            .as_str()
            .unwrap()
            .contains("+    let x = 2;")
    );
    assert!(f.on_disk("main.rs").contains("let x = 2;"));

    // One journal row, with both versions kept.
    let edits = f.workspace.journal().for_chat(f.chat).unwrap();
    assert_eq!(edits.len(), 1);
    assert!(edits[0].before_blob_hash.is_some() && edits[0].after_blob_hash.is_some());
    assert_eq!(edits[0].stats_json, "{\"added\":1,\"removed\":1}");
}

#[tokio::test]
async fn a_near_miss_names_the_indentation_and_writes_nothing() {
    let f = fixture();
    f.read("main.rs").await;
    let (message, is_error) = f
        .call(
            "replace",
            serde_json::json!({
                "path": f.path("main.rs"),
                "old": "let x = 1;\nprintln!(\"{x}\");",
                "new": "let x = 2;"
            }),
        )
        .await;
    assert!(is_error, "{message}");
    assert!(message.contains("different indentation"), "{message}");
    assert_eq!(f.on_disk("main.rs"), FILE);
    assert!(f.workspace.journal().for_chat(f.chat).unwrap().is_empty());
}

#[tokio::test]
async fn a_file_changed_underneath_is_refused_with_what_to_do() {
    let f = fixture();
    f.read("main.rs").await;
    std::fs::write(
        f.work.path().join("main.rs"),
        "fn main() {\n    let y = 7;\n}\n",
    )
    .unwrap();
    let (message, is_error) = f
        .call(
            "replace",
            serde_json::json!({ "path": f.path("main.rs"), "old": "let x = 1;", "new": "let x = 2;" }),
        )
        .await;
    assert!(is_error, "{message}");
    assert!(message.contains("changed on disk"), "{message}");
    assert!(message.contains("Read the file again"), "{message}");
}

#[tokio::test]
async fn an_edit_elsewhere_in_a_changed_file_still_applies_and_says_so() {
    let f = fixture();
    f.read("main.rs").await;
    // The user's own editor adds a line above; the passage is untouched.
    std::fs::write(
        f.work.path().join("main.rs"),
        format!("// a comment\n{FILE}"),
    )
    .unwrap();
    let (message, is_error) = f
        .call(
            "replace",
            serde_json::json!({ "path": f.path("main.rs"), "old": "let x = 1;", "new": "let x = 2;" }),
        )
        .await;
    assert!(!is_error, "{message}");
    assert!(message.contains("had changed on disk"), "{message}");
    assert!(f.on_disk("main.rs").starts_with("// a comment"));
    assert!(f.on_disk("main.rs").contains("let x = 2;"));
}

#[tokio::test]
async fn insert_and_patch_and_undo_walk_the_file_back() {
    let f = fixture();
    f.read("main.rs").await;
    let (_, is_error) = f
        .call(
            "insert",
            serde_json::json!({
                "path": f.path("main.rs"),
                "text": "    let y = 2;",
                "after": "let x = 1;"
            }),
        )
        .await;
    assert!(!is_error);
    assert!(f.on_disk("main.rs").contains("let y = 2;"));

    let patch = "--- a\n+++ b\n@@ -1,2 +1,2 @@\n fn main() {\n-    let x = 1;\n+    let x = 42;\n";
    let (message, is_error) = f
        .call(
            "apply_patch",
            serde_json::json!({ "path": f.path("main.rs"), "patch": patch }),
        )
        .await;
    assert!(!is_error, "{message}");
    assert!(f.on_disk("main.rs").contains("let x = 42;"));

    // Undo the patch, then both edits.
    let (message, is_error) = f
        .call("undo", serde_json::json!({ "path": f.path("main.rs") }))
        .await;
    assert!(!is_error, "{message}");
    assert!(f.on_disk("main.rs").contains("let x = 1;"));
    assert!(f.on_disk("main.rs").contains("let y = 2;"));

    let (message, is_error) = f
        .call(
            "undo",
            serde_json::json!({ "path": f.path("main.rs"), "steps": 2 }),
        )
        .await;
    assert!(!is_error, "{message}");
    assert_eq!(f.on_disk("main.rs"), FILE, "back to where it started");

    // Nothing was deleted from the journal: every edit is still there, marked.
    let edits = f.workspace.journal().for_chat(f.chat).unwrap();
    assert_eq!(edits.len(), 4, "two edits and two undos");
    assert_eq!(edits.iter().filter(|e| e.reverted_at.is_some()).count(), 3);
}

#[tokio::test]
async fn undo_refuses_when_someone_else_wrote_the_file() {
    let f = fixture();
    f.read("main.rs").await;
    f.call(
        "replace",
        serde_json::json!({ "path": f.path("main.rs"), "old": "let x = 1;", "new": "let x = 2;" }),
    )
    .await;
    std::fs::write(f.work.path().join("main.rs"), "fn main() {}\n").unwrap();
    let (message, is_error) = f
        .call("undo", serde_json::json!({ "path": f.path("main.rs") }))
        .await;
    assert!(is_error, "{message}");
    assert!(message.contains("would discard"), "{message}");
    assert_eq!(f.on_disk("main.rs"), "fn main() {}\n");
}

#[tokio::test]
async fn a_path_outside_the_folder_asks_for_the_folder_instead_of_writing() {
    let f = fixture();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("other.rs"), FILE).unwrap();
    let (message, is_error) = f
        .call(
            "replace",
            serde_json::json!({
                "path": elsewhere.path().join("other.rs").display().to_string(),
                "old": "let x = 1;",
                "new": "let x = 2;"
            }),
        )
        .await;
    assert!(is_error, "{message}");
    assert!(message.contains("not inside any folder"), "{message}");
    assert_eq!(
        std::fs::read_to_string(elsewhere.path().join("other.rs")).unwrap(),
        FILE
    );
}

/// A credential file is decided *before* the call, by the guardrail floor, which asks in every
/// mode and cannot be answered by a standing grant (04 §5). Until M7 there was nowhere to raise
/// that question from, so these tools refused outright; now that there is, a refusal here would
/// mean the user says yes on the card and the connector says no anyway. The rules themselves are
/// tested in `gantry-core::guardrail`; what this asserts is that the connector has stopped
/// second-guessing an answer it did not hear.
#[tokio::test]
async fn a_credential_file_is_edited_only_once_the_user_has_said_so() {
    let f = fixture();
    std::fs::write(f.work.path().join(".env"), "TOKEN=abc\n").unwrap();
    f.read(".env").await;
    let (message, is_error) = f
        .call(
            "replace",
            serde_json::json!({ "path": f.path(".env"), "old": "TOKEN=abc", "new": "TOKEN=xyz" }),
        )
        .await;
    assert!(!is_error, "{message}");
    assert_eq!(f.on_disk(".env"), "TOKEN=xyz\n");
}

#[tokio::test]
async fn line_endings_survive_an_edit() {
    let f = fixture();
    std::fs::write(f.work.path().join("crlf.txt"), "one\r\ntwo\r\n").unwrap();
    f.read("crlf.txt").await;
    let (message, is_error) = f
        .call(
            "replace",
            serde_json::json!({ "path": f.path("crlf.txt"), "old": "two", "new": "three" }),
        )
        .await;
    assert!(!is_error, "{message}");
    assert_eq!(
        std::fs::read(f.work.path().join("crlf.txt")).unwrap(),
        b"one\r\nthree\r\n"
    );
}

/// Stop stops the calls that have not started (03 §4). An edit is one file operation, so the
/// token is read before the file is opened and not again: a replace abandoned between reading
/// and writing would leave the journal saying one thing and the disk another.
#[tokio::test]
async fn a_cancelled_edit_leaves_the_file_alone() {
    let f = fixture();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let (text, is_error) = f
        .call_with(
            "replace",
            serde_json::json!({ "path": f.path("main.rs"), "old": "let x = 1;", "new": "let x = 2;" }),
            cancelled,
        )
        .await;
    assert!(is_error, "{text}");
    assert!(text.contains("Cancelled"), "{text}");
    assert_eq!(f.on_disk("main.rs"), FILE, "nothing was written");
}
