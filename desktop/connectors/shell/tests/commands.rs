//! The connector against real processes (`docs/connectors/shell.md` §13). Everything here uses
//! `sh` and shell built-ins, so no test depends on a tool being installed.

#![cfg(unix)]

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use gantry_connector_shell::{Shell, ShellEnv};
use gantry_connectors::{
    ChatScope, Connector, OutputStream, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::{
    CallId, ChatId, InstanceId, Mode, ModelRef, ReasoningEffort, ResultPart, Surface, TurnId,
    now_ms,
};
use gantry_store::{BlobStore, Store};
use gantry_workspace::Workspace;
use tokio_util::sync::CancellationToken;

struct Fixture {
    _dir: tempfile::TempDir,
    work: tempfile::TempDir,
    shell: Shell,
    chat: ChatId,
    turn: TurnId,
}

/// Collects what the feed would have shown, so the streaming can be asserted.
#[derive(Default)]
struct Collect {
    lines: Mutex<Vec<(OutputStream, String)>>,
}

impl ToolEventSink for Collect {
    fn output(&self, _call_id: &CallId, stream: OutputStream, chunk: &[u8]) {
        self.lines
            .lock()
            .unwrap()
            .push((stream, String::from_utf8_lossy(chunk).into_owned()));
    }
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());

    let now = now_ms();
    let chat = ChatId::new();
    let record = gantry_store::repos::chats::ChatRecord {
        surface: Surface::Code,
        id: chat,
        project_id: None,
        title: "commands".into(),
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
    };
    let root = work.path().display().to_string();
    store
        .write_blocking(move |conn| {
            gantry_store::repos::chats::insert(conn, &record)?;
            gantry_store::repos::chats::add_root(conn, chat, &root)
        })
        .unwrap();

    let workspace = Arc::new(Workspace::new(store, blobs, dir.path().join("app-data")));
    Fixture {
        shell: Shell::new(
            "shell".into(),
            InstanceId::new(),
            workspace,
            // The inherited environment, never the developer's login shell: a test that reads
            // `~/.zshrc` passes or fails by whose machine it runs on.
            Arc::new(ShellEnv::inherited()),
        ),
        _dir: dir,
        work,
        chat,
        turn: TurnId::new(),
    }
}

impl Fixture {
    async fn run(&self, args: serde_json::Value) -> serde_json::Value {
        self.call("run_command", args, Arc::new(Collect::default()), None)
            .await
    }

    async fn call(
        &self,
        tool: &str,
        args: serde_json::Value,
        sink: Arc<dyn ToolEventSink>,
        cancel: Option<CancellationToken>,
    ) -> serde_json::Value {
        let outcome = self
            .shell
            .call(
                ToolCallRequest {
                    call_id: CallId::new(),
                    tool: tool.to_owned(),
                    args,
                    scope: ChatScope {
                        chat_id: self.chat,
                        turn_id: self.turn,
                        mode: Mode::AutoEdit,
                    },
                },
                sink,
                cancel.unwrap_or_default(),
            )
            .await
            .expect("a refusal is a result, not an error");
        let ToolOutcome::Complete {
            content,
            structured,
            is_error,
        } = outcome;
        if let Some(json) = structured {
            return json;
        }
        let text = content
            .iter()
            .map(|p| match p {
                ResultPart::Text { text } => text.clone(),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        serde_json::json!({ "error": is_error, "text": text })
    }
}

#[tokio::test]
async fn a_command_runs_and_reports_both_streams_and_its_exit_code() {
    let f = fixture();
    let result = f
        .run(serde_json::json!({ "command": "echo out; echo err >&2; exit 3" }))
        .await;
    assert_eq!(result["exit_code"], 3);
    assert!(result["stdout"].as_str().unwrap().contains("out"));
    assert!(result["stderr"].as_str().unwrap().contains("err"));
    assert_eq!(result["killed"], false);
    // A non-zero exit is a result, not an error (D5).
    assert!(result["note"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn output_reaches_the_feed_while_the_command_runs() {
    let f = fixture();
    let sink = Arc::new(Collect::default());
    f.call(
        "run_command",
        serde_json::json!({ "command": "echo one; echo two >&2" }),
        sink.clone(),
        None,
    )
    .await;
    let lines = sink.lines.lock().unwrap();
    assert!(
        lines
            .iter()
            .any(|(s, t)| *s == OutputStream::Stdout && t.contains("one"))
    );
    assert!(
        lines
            .iter()
            .any(|(s, t)| *s == OutputStream::Stderr && t.contains("two"))
    );
}

#[tokio::test]
async fn the_command_runs_in_the_attached_folder_by_default() {
    let f = fixture();
    std::fs::write(f.work.path().join("marker.txt"), "x").unwrap();
    let result = f.run(serde_json::json!({ "command": "ls" })).await;
    assert!(result["stdout"].as_str().unwrap().contains("marker.txt"));
    assert!(
        result["cwd"]
            .as_str()
            .unwrap()
            .contains(f.work.path().file_name().unwrap().to_str().unwrap())
    );
}

#[tokio::test]
async fn a_working_directory_outside_the_attached_folders_is_refused() {
    let f = fixture();
    let result = f
        .run(serde_json::json!({ "command": "ls", "cwd": "/etc" }))
        .await;
    let text = result["text"].as_str().unwrap_or_default();
    assert_eq!(result["error"], true, "{result}");
    assert!(
        text.contains("attached"),
        "the refusal says what would fix it: {text}"
    );
}

#[tokio::test]
async fn a_chat_with_no_folder_is_told_so_rather_than_running_anywhere() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
    let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());
    let workspace = Arc::new(Workspace::new(store, blobs, dir.path().join("app-data")));
    let shell = Shell::new(
        "shell".into(),
        InstanceId::new(),
        workspace,
        Arc::new(ShellEnv::inherited()),
    );
    let outcome = shell
        .call(
            ToolCallRequest {
                call_id: CallId::new(),
                tool: "run_command".into(),
                args: serde_json::json!({ "command": "ls" }),
                scope: ChatScope {
                    chat_id: ChatId::new(),
                    turn_id: TurnId::new(),
                    mode: Mode::AutoEdit,
                },
            },
            Arc::new(gantry_connectors::NoopToolEvents),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let ToolOutcome::Complete {
        content, is_error, ..
    } = outcome;
    assert!(is_error);
    let ResultPart::Text { text } = &content[0] else {
        panic!("expected text")
    };
    assert!(text.contains("no folder attached"), "{text}");
    assert!(text.contains("nowhere to run"), "{text}");
}

#[tokio::test]
async fn a_command_that_outlives_its_deadline_is_killed_with_its_children() {
    let f = fixture();
    // The child ignores termination and outlives the shell that started it. If only the shell
    // were killed, this file would be written a second after the call returns — which is the
    // exact mistake D4 exists to prevent.
    let marker = f.work.path().join("child-survived.txt");
    let command = format!(
        "trap '' TERM; (sleep 1; echo alive > {}) & sleep 30",
        marker.display()
    );
    let result = f
        .run(serde_json::json!({ "command": command, "timeout_ms": 1000 }))
        .await;
    assert_eq!(result["timed_out"], true, "{result}");
    assert_eq!(result["killed"], true);
    assert!(
        result["note"]
            .as_str()
            .unwrap()
            .contains("long-running processes"),
        "the message points at the real limit rather than saying it failed"
    );
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        !marker.exists(),
        "the child outlived the kill: the process group was not signalled"
    );
}

#[tokio::test]
async fn cancelling_the_turn_stops_the_command() {
    let f = fixture();
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        token.cancel();
    });
    let result = f
        .call(
            "run_command",
            serde_json::json!({ "command": "sleep 30" }),
            Arc::new(Collect::default()),
            Some(cancel),
        )
        .await;
    assert_eq!(result["killed"], true, "{result}");
    assert_eq!(result["timed_out"], false);
    assert!(result["duration_ms"].as_u64().unwrap() < 5_000);
}

#[tokio::test]
async fn far_more_output_than_the_model_gets_keeps_the_head_and_the_tail() {
    let f = fixture();
    let result = f
        .run(serde_json::json!({
            "command": "i=0; while [ $i -lt 4000 ]; do echo \"line $i padding padding padding\"; i=$((i+1)); done"
        }))
        .await;
    let stdout = result["stdout"].as_str().unwrap();
    assert_eq!(result["truncated"], true);
    assert!(stdout.contains("line 0"), "the head survives");
    assert!(stdout.contains("line 3999"), "the tail survives");
    assert!(
        stdout.contains("elided"),
        "the loss is counted, never silent"
    );
}

#[tokio::test]
async fn the_classifier_verdict_travels_with_the_result() {
    let f = fixture();
    let reads = f.run(serde_json::json!({ "command": "ls" })).await;
    assert_eq!(reads["checked_read_only"], true);
    let writes = f
        .run(serde_json::json!({ "command": "touch made-a-file" }))
        .await;
    assert_eq!(writes["checked_read_only"], false);
}

#[tokio::test]
async fn extra_environment_reaches_the_command_and_costs_it_the_read_only_verdict() {
    let f = fixture();
    let result = f
        .run(serde_json::json!({
            "command": "echo $GANTRY_TEST_VAR",
            "env": { "GANTRY_TEST_VAR": "visible" }
        }))
        .await;
    assert!(result["stdout"].as_str().unwrap().contains("visible"));
    // An environment decides which program the command's words resolve to — PATH picks the `ls`,
    // BASH_ENV sources a file first, an exported function replaces it outright — so a call that
    // sets one is never reported as proven read-only, whatever the command says.
    assert_eq!(result["checked_read_only"], false, "{result}");
}

#[tokio::test]
async fn output_that_is_not_ascii_survives_the_read_boundaries() {
    let f = fixture();
    // Far more than one 8 KB read, all multi-byte: any read that ends mid-character would show
    // as replacement marks if the reader converted each read on its own.
    let result = f
        .run(serde_json::json!({
            "command": "i=0; while [ $i -lt 400 ]; do printf 'héllo wörld ✓ →\n'; i=$((i+1)); done"
        }))
        .await;
    let stdout = result["stdout"].as_str().unwrap();
    assert!(
        !stdout.contains('\u{FFFD}'),
        "a character was split across two reads"
    );
    assert!(stdout.contains("héllo wörld ✓ →"));
}

#[tokio::test]
async fn standard_input_is_closed_so_a_command_that_asks_ends() {
    let f = fixture();
    let result = f
        .run(serde_json::json!({ "command": "read -r answer; echo \"got:$answer\"", "timeout_ms": 5000 }))
        .await;
    assert_eq!(result["timed_out"], false, "it ended rather than hanging");
    assert!(result["stdout"].as_str().unwrap().contains("got:"));
}

#[tokio::test]
async fn killing_an_unknown_call_says_so_rather_than_pretending() {
    let f = fixture();
    let result = f
        .call(
            "kill_command",
            serde_json::json!({ "call_id": "nothing-here" }),
            Arc::new(Collect::default()),
            None,
        )
        .await;
    assert_eq!(result["error"], true);
    assert!(result["text"].as_str().unwrap().contains("No command"));
}
