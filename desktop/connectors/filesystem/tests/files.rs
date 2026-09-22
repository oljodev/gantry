//! The connector end to end, against real files: the boundary of `filesystem.md` §4, the
//! reading rules of §5, the ignore rules of §7, and the failure texts of §11, which are as much
//! part of the interface as the successes are.

use std::sync::Arc;

use gantry_connector_filesystem::Filesystem;
use gantry_connectors::{ChatScope, Connector, NoopToolEvents, ToolCallRequest, ToolOutcome};
use gantry_core::{
    CallId, ChatId, InstanceId, Mode, ModelRef, ProviderId, ReasoningEffort, ResultPart, Surface,
    TurnId, now_ms,
};
use gantry_store::{BlobStore, Store, repos::chats};
use gantry_workspace::Workspace;
use tokio_util::sync::CancellationToken;

struct Fixture {
    _dir: tempfile::TempDir,
    work: tempfile::TempDir,
    fs: Filesystem,
    workspace: Arc<Workspace>,
    chat: ChatId,
    turn: TurnId,
}

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
        title: "files".into(),
        title_source: "auto".into(),
        pinned: false,
        mode: Mode::AutoEdit,
        guard: true,
        model: ModelRef::new(ProviderId::openrouter(), "test/model"),
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
        parent_turn_id: None,
        agent_type: None,
    };
    let root = work.path().display().to_string();
    store
        .write_blocking(move |conn| {
            chats::insert(conn, &record)?;
            chats::add_root(conn, chat, &root)
        })
        .unwrap();

    let w = work.path();
    std::fs::create_dir_all(w.join("src")).unwrap();
    std::fs::create_dir_all(w.join("node_modules/left-pad")).unwrap();
    std::fs::write(w.join(".gitignore"), "build/\n").unwrap();
    std::fs::create_dir_all(w.join("build")).unwrap();
    std::fs::write(w.join("build/out.js"), "needle\n").unwrap();
    std::fs::write(w.join("node_modules/left-pad/index.js"), "needle\n").unwrap();
    std::fs::write(w.join("src/main.rs"), "fn main() {\n    // needle\n}\n").unwrap();
    std::fs::write(w.join("README.md"), "# Title\n").unwrap();
    std::fs::write(w.join(".env"), "TOKEN=abc\n").unwrap();

    let workspace =
        Arc::new(Workspace::new(store, blobs, dir.path().join("app-data")).without_trash());
    Fixture {
        fs: Filesystem::new("filesystem".into(), InstanceId::new(), workspace.clone()),
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
            .fs
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

    async fn ok(&self, tool: &str, args: serde_json::Value) -> serde_json::Value {
        let (text, is_error) = self.call(tool, args).await;
        assert!(!is_error, "{tool}: {text}");
        serde_json::from_str(&text).unwrap()
    }

    async fn refused(&self, tool: &str, args: serde_json::Value) -> String {
        let (text, is_error) = self.call(tool, args).await;
        assert!(is_error, "{tool} should have refused, got: {text}");
        text
    }
}

#[tokio::test]
async fn reading_reports_position_and_spelling_without_putting_them_in_the_text() {
    let f = fixture();
    let result = f
        .ok(
            "read_file",
            serde_json::json!({ "path": f.path("src/main.rs") }),
        )
        .await;
    assert_eq!(result["content"], "fn main() {\n    // needle\n}");
    assert_eq!(result["first_line"], 1);
    assert_eq!(result["total_lines"], 3);
    assert_eq!(result["more"], false);
    assert_eq!(result["encoding"], "UTF-8");
    assert_eq!(result["line_ending"], "LF");

    // The window reaches the end exactly once, with no gap and no overlap.
    let first = f
        .ok(
            "read_file",
            serde_json::json!({ "path": f.path("src/main.rs"), "limit": 2 }),
        )
        .await;
    assert_eq!(first["content"], "fn main() {\n    // needle");
    assert_eq!(first["more"], true);
    let rest = f
        .ok(
            "read_file",
            serde_json::json!({ "path": f.path("src/main.rs"), "offset": 2, "limit": 2 }),
        )
        .await;
    assert_eq!(rest["content"], "}");
    assert_eq!(rest["more"], false);
}

#[tokio::test]
async fn reading_says_what_to_use_instead_when_the_path_is_a_folder_or_binary() {
    let f = fixture();
    let message = f
        .refused("read_file", serde_json::json!({ "path": f.path("src") }))
        .await;
    assert!(message.contains("list_directory"), "{message}");

    std::fs::write(f.work.path().join("logo.png"), b"\x89PNG\r\n\x1a\n\0\0").unwrap();
    let message = f
        .refused(
            "read_file",
            serde_json::json!({ "path": f.path("logo.png") }),
        )
        .await;
    assert!(message.contains("a PNG image"), "{message}");
}

/// A real PDF, built rather than checked in: `gantry_documents::sample` says why.
fn note() -> Vec<u8> {
    gantry_documents::sample::pdf(&["A note about gantries.\nSecond line of it."])
}

#[tokio::test]
async fn a_document_is_read_as_its_text_with_pages_instead_of_line_endings() {
    let f = fixture();
    std::fs::write(f.work.path().join("note.pdf"), note()).unwrap();
    let result = f
        .ok(
            "read_file",
            serde_json::json!({ "path": f.path("note.pdf") }),
        )
        .await;
    let content = result["content"].as_str().unwrap();
    assert!(content.contains("A note about gantries."), "{content}");
    assert!(content.contains("Second line of it."), "{content}");
    assert_eq!(result["document"], "PDF document");
    assert_eq!(result["pages"], 1);
    assert_eq!(result["first_page"], 1);
    assert_eq!(result["last_page"], 1);
    assert_eq!(result["more"], false);
    // Nothing about the bytes on disk is reported, because they are not the text: a document has
    // no line ending to preserve and nothing here may be written back.
    assert!(result.get("line_ending").is_none(), "{result}");
    assert!(result.get("encoding").is_none(), "{result}");

    // And the reverse: a file whose name claims to be a document but holds text is read as the
    // text it is.
    std::fs::write(f.work.path().join("notes.pdf"), "not really a pdf\n").unwrap();
    let result = f
        .ok(
            "read_file",
            serde_json::json!({ "path": f.path("notes.pdf") }),
        )
        .await;
    assert_eq!(result["content"], "not really a pdf");
    assert_eq!(result["line_ending"], "LF");
}

#[tokio::test]
async fn a_document_that_was_read_is_still_not_something_write_file_may_replace() {
    let f = fixture();
    let path = f.work.path().join("note.pdf");
    std::fs::write(&path, note()).unwrap();
    f.ok(
        "read_file",
        serde_json::json!({ "path": f.path("note.pdf") }),
    )
    .await;
    // Reading it does not make it writable. Having read a PDF is exactly when a model is most
    // likely to write to it, and the write would replace the document with its own text.
    let message = f
        .refused(
            "write_file",
            serde_json::json!({ "path": f.path("note.pdf"), "content": "rewritten" }),
        )
        .await;
    assert!(message.contains("another path"), "{message}");
    assert_eq!(std::fs::read(&path).unwrap(), note());
}

#[tokio::test]
async fn a_missing_file_names_the_nearest_folder_that_does_exist() {
    let f = fixture();
    let message = f
        .refused(
            "read_file",
            serde_json::json!({ "path": f.path("src/nope/deeper.rs") }),
        )
        .await;
    assert!(message.contains("does not exist"), "{message}");
    assert!(message.contains(&f.path("src")), "{message}");
}

#[tokio::test]
async fn a_path_outside_the_folders_says_which_folders_there_are() {
    let f = fixture();
    let message = f
        .refused("read_file", serde_json::json!({ "path": "/etc/passwd" }))
        .await;
    assert!(message.contains("not inside any folder"), "{message}");
    assert!(
        message.contains(&f.path("")[..f.path("").len() - 1]),
        "{message}"
    );
    assert!(message.contains("Ask the user to attach"), "{message}");
}

#[tokio::test]
async fn listing_and_searching_apply_the_folders_own_ignore_rules() {
    let f = fixture();
    let listed = f
        .ok("list_directory", serde_json::json!({ "path": f.path("") }))
        .await;
    let names: Vec<String> = listed["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_owned())
        .collect();
    assert!(names.contains(&"src".to_owned()), "{names:?}");
    assert!(!names.contains(&".env".to_owned()), "{names:?}");
    assert!(!names.contains(&"build".to_owned()), "{names:?}");

    // A search covers every attached folder when none is named, and finds the user's code only.
    let found = f
        .ok("grep", serde_json::json!({ "pattern": "needle" }))
        .await;
    assert_eq!(found["files_with_matches"], 1, "{found}");
    let matches = found["result"]["matches"].as_array().unwrap();
    assert_eq!(matches[0]["line"], 2);
    assert!(matches[0]["path"].as_str().unwrap().ends_with("main.rs"));

    // The cheaper modes answer the same question with fewer tokens.
    let files = f
        .ok(
            "grep",
            serde_json::json!({ "pattern": "needle", "mode": "files" }),
        )
        .await;
    assert_eq!(files["result"]["files"].as_array().unwrap().len(), 1);

    let globbed = f
        .ok("glob", serde_json::json!({ "pattern": "**/*.rs" }))
        .await;
    assert_eq!(globbed["paths"].as_array().unwrap().len(), 1, "{globbed}");
}

#[tokio::test]
async fn an_ignored_file_is_hidden_from_search_but_read_by_name() {
    let f = fixture();
    let read = f
        .ok(
            "read_file",
            serde_json::json!({ "path": f.path("build/out.js") }),
        )
        .await;
    assert_eq!(read["content"], "needle");
}

#[tokio::test]
async fn writing_creates_a_file_journals_it_and_keeps_the_spelling_of_an_existing_one() {
    let f = fixture();
    let wrote = f
        .ok(
            "write_file",
            serde_json::json!({ "path": f.path("src/new.rs"), "content": "fn new() {}\n" }),
        )
        .await;
    assert_eq!(wrote["created"], true);
    assert_eq!(wrote["added"], 1);
    assert_eq!(
        std::fs::read_to_string(f.work.path().join("src/new.rs")).unwrap(),
        "fn new() {}\n"
    );

    // A CRLF file stays a CRLF file, which is the difference between a one-line diff and one
    // that touches every line.
    std::fs::write(f.work.path().join("crlf.txt"), "one\r\ntwo\r\n").unwrap();
    f.ok(
        "write_file",
        serde_json::json!({ "path": f.path("crlf.txt"), "content": "one\nthree\n" }),
    )
    .await;
    assert_eq!(
        std::fs::read(f.work.path().join("crlf.txt")).unwrap(),
        b"one\r\nthree\r\n"
    );

    let edits = f.workspace.journal().for_chat(f.chat).unwrap();
    assert_eq!(edits.len(), 2);
    assert_eq!(edits[0].op, gantry_core::EditOp::Create);
    assert_eq!(edits[1].op, gantry_core::EditOp::Modify);
    assert!(
        edits[1].before_blob_hash.is_some(),
        "the old bytes are kept"
    );
}

#[tokio::test]
async fn writing_refuses_to_overwrite_what_something_else_changed_since_it_was_read() {
    let f = fixture();
    f.ok(
        "read_file",
        serde_json::json!({ "path": f.path("README.md") }),
    )
    .await;
    std::fs::write(f.work.path().join("README.md"), "# Someone else\n").unwrap();
    let message = f
        .refused(
            "write_file",
            serde_json::json!({ "path": f.path("README.md"), "content": "# Mine\n" }),
        )
        .await;
    assert!(message.contains("changed on disk"), "{message}");
    assert_eq!(
        std::fs::read_to_string(f.work.path().join("README.md")).unwrap(),
        "# Someone else\n"
    );
}

#[tokio::test]
async fn moving_and_copying_record_both_ends() {
    let f = fixture();
    f.ok(
        "create_directory",
        serde_json::json!({ "path": f.path("docs/deep") }),
    )
    .await;
    assert!(f.work.path().join("docs/deep").is_dir());

    let moved = f
        .ok(
            "move_path",
            serde_json::json!({ "from": f.path("README.md"), "to": f.path("docs/README.md") }),
        )
        .await;
    assert_eq!(moved["copied"], false);
    assert!(!f.work.path().join("README.md").exists());
    assert!(f.work.path().join("docs/README.md").exists());

    let copied = f
        .ok(
            "copy_path",
            serde_json::json!({ "from": f.path("docs/README.md"), "to": f.path("docs/copy.md") }),
        )
        .await;
    assert_eq!(copied["copied"], true);

    let edits = f.workspace.journal().for_chat(f.chat).unwrap();
    let rename = edits
        .iter()
        .find(|e| e.op == gantry_core::EditOp::Rename)
        .expect("the move is journaled");
    assert_eq!(
        rename.from_path.as_deref(),
        Some(f.path("README.md").as_str())
    );

    // A destination that is already there is not silently replaced.
    let message = f
        .refused(
            "copy_path",
            serde_json::json!({ "from": f.path("docs/README.md"), "to": f.path("docs/copy.md") }),
        )
        .await;
    assert!(message.contains("already exists"), "{message}");

    // Neither end may leave the attached folders.
    let elsewhere = tempfile::tempdir().unwrap();
    let message = f
        .refused(
            "move_path",
            serde_json::json!({
                "from": f.path("docs/README.md"),
                "to": elsewhere.path().join("stolen.md").display().to_string()
            }),
        )
        .await;
    assert!(message.contains("not inside any folder"), "{message}");
    assert!(f.work.path().join("docs/README.md").exists());
}

#[tokio::test]
async fn deleting_keeps_the_file_in_the_journal_and_says_which_promise_it_kept() {
    let f = fixture();
    let deleted = f
        .ok(
            "delete_path",
            serde_json::json!({ "path": f.path("README.md") }),
        )
        .await;
    assert!(!f.work.path().join("README.md").exists());
    // The trash is off in tests, so the result must say so rather than imply a recovery that
    // did not happen.
    assert_eq!(deleted["trashed"], false);
    assert_eq!(deleted["revertible"], true, "the bytes are in the journal");

    let edits = f.workspace.journal().for_chat(f.chat).unwrap();
    assert_eq!(edits[0].op, gantry_core::EditOp::Delete);
    let kept = f
        .workspace
        .journal()
        .content(edits[0].before_blob_hash.as_deref().unwrap())
        .unwrap();
    assert_eq!(kept, b"# Title\n");

    // A folder needs the flag, and the refusal says so.
    let message = f
        .refused("delete_path", serde_json::json!({ "path": f.path("src") }))
        .await;
    assert!(message.contains("recursive"), "{message}");
    assert!(f.work.path().join("src").is_dir());

    let deleted = f
        .ok(
            "delete_path",
            serde_json::json!({ "path": f.path("src"), "recursive": true }),
        )
        .await;
    assert!(!f.work.path().join("src").exists());
    assert_eq!(
        deleted["revertible"], false,
        "a whole tree cannot be put back from the journal, and the row must not pretend it can"
    );
}

/// A credential file is a question the guardrail floor asks before the call ever reaches this
/// connector, in every mode and past any standing grant (04 §5, `gantry-core::guardrail`).
/// Refusing here as well would mean the user answers the card and the connector overrules them,
/// so the connector does what it was told, and the decision stays in one place.
#[tokio::test]
async fn a_credential_file_is_handled_once_the_decision_has_been_made() {
    let f = fixture();
    let read = f
        .ok("read_file", serde_json::json!({ "path": f.path(".env") }))
        .await;
    assert_eq!(read["content"], "TOKEN=abc");

    f.ok(
        "write_file",
        serde_json::json!({ "path": f.path(".env"), "content": "TOKEN=xyz\n" }),
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(f.work.path().join(".env")).unwrap(),
        "TOKEN=xyz\n"
    );
}

#[tokio::test]
async fn gantrys_own_data_is_never_written_even_from_inside_a_root() {
    let f = fixture();
    // A chat whose root is the folder holding Gantry's own data still cannot write into it.
    let data = f._dir.path().join("app-data");
    std::fs::create_dir_all(&data).unwrap();
    let store_path = data.join("gantry.db");
    std::fs::write(&store_path, b"pretend").unwrap();
    let root = f._dir.path().display().to_string();
    let chat = f.chat;
    // Attach the parent of the data directory, which is the shape of the mistake this guards.
    f.workspace
        .journal()
        .for_chat(chat)
        .expect("the journal is reachable");
    let roots = gantry_workspace::Roots::open(&[root], std::slice::from_ref(&data));
    let err = roots
        .resolve(&store_path.display().to_string())
        .expect_err("Gantry's own data is refused");
    assert!(err.to_string().contains("never writable"), "{err}");
}

/// Stop means the calls behind the one in flight do not happen (03 §4). A file operation here is
/// a single step, so the token is read once, before anything touches the disk, and never again:
/// a write abandoned halfway would leave a file nobody wrote.
#[tokio::test]
async fn a_cancelled_call_writes_nothing_and_says_so() {
    let f = fixture();
    let cancelled = CancellationToken::new();
    cancelled.cancel();

    let (text, is_error) = f
        .call_with(
            "write_file",
            serde_json::json!({ "path": f.path("src/new.rs"), "content": "fn main() {}" }),
            cancelled.clone(),
        )
        .await;
    assert!(is_error, "a call that did not run is not a success");
    assert!(text.contains("Cancelled"), "{text}");
    assert!(
        !f.work.path().join("src/new.rs").exists(),
        "nothing reached the disk"
    );

    // And a read is refused the same way rather than quietly answering.
    let (text, is_error) = f
        .call_with(
            "read_file",
            serde_json::json!({ "path": f.path("README.md") }),
            cancelled,
        )
        .await;
    assert!(is_error && text.contains("Cancelled"), "{text}");
}
