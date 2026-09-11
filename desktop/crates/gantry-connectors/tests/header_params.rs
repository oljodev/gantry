//! Arguments that travel as headers (SEP-2243), which is how GitHub's server takes `owner` and
//! `repo` (docs/plan/03 §6).
//!
//! A server may annotate a tool's arguments with `x-mcp-header`, and then refuse a call whose
//! arguments arrived only in the body: "header mismatch: missing Mcp-Param-owner header". The
//! client learns which arguments to promote from a `tools/list` it has seen *on that
//! connection*, so a session that opens and calls straight away fails every annotated tool —
//! which is exactly what happened live, after two unannotated tools had worked.

use std::sync::{Arc, Mutex};

use gantry_connectors::{
    Connector, NoopToolEvents, ToolCallRequest,
    mcp::{Endpoint, McpConnector},
};
use gantry_core::{CallId, ChatId, ConnectorKind, InstanceId, Mode, TurnId};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;

/// A server that speaks the 2026-07-28 revision, offers one tool whose `owner` argument is
/// annotated as a header, and refuses a call that arrives without it.
async fn serve(seen: Arc<Mutex<Vec<String>>>) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("a port");
    let url = format!("http://{}/mcp", listener.local_addr().expect("an address"));
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = vec![0_u8; 32 * 1024];
                let read = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..read]).to_string();
                let header = |name: &str| {
                    request
                        .lines()
                        .skip(1)
                        .filter_map(|l| l.split_once(':'))
                        .find(|(n, _)| n.trim().eq_ignore_ascii_case(name))
                        .map(|(_, v)| v.trim().to_owned())
                };
                let body = request.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
                if body.contains("\"tools/call\"") {
                    seen.lock()
                        .expect("the record")
                        .push(header("Mcp-Param-owner").unwrap_or_default());
                }
                let _ = socket
                    .write_all(answer(body, header("Mcp-Param-owner")).as_bytes())
                    .await;
                let _ = socket.shutdown().await;
            });
        }
    });
    url
}

fn answer(body: &str, owner_header: Option<String>) -> String {
    let request: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let Some(id) = request.get("id") else {
        return "HTTP/1.1 202 Accepted\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            .to_owned();
    };
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let result = match method {
        "initialize" => serde_json::json!({
            "protocolVersion": "2026-07-28",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "annotator", "version": "1.0.0" },
        }),
        "tools/list" => serde_json::json!({
            "tools": [{
                "name": "get_file",
                "description": "Reads a file from a repository.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        // The annotation is the suffix; the client sends it as `Mcp-Param-owner`.
                        "owner": { "type": "string", "x-mcp-header": "owner" },
                        "path": { "type": "string" },
                    },
                    "required": ["owner", "path"],
                },
                "annotations": { "readOnlyHint": true },
            }],
        }),
        "tools/call" => {
            // What GitHub does when the argument came only in the body.
            let Some(owner) = owner_header.filter(|v| !v.is_empty()) else {
                let payload = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32020,
                        "message": "header mismatch: missing Mcp-Param-owner header for parameter \"owner\"",
                    },
                })
                .to_string();
                return http(&payload);
            };
            serde_json::json!({ "content": [{ "type": "text", "text": format!("read for {owner}") }] })
        }
        _ => {
            let payload = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "Method not found" },
            })
            .to_string();
            return http(&payload);
        }
    };
    http(&serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
}

fn http(payload: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
        payload.len()
    )
}

fn request(tool: &str, args: serde_json::Value) -> ToolCallRequest {
    ToolCallRequest {
        call_id: CallId::new(),
        tool: tool.to_owned(),
        args,
        scope: gantry_connectors::ChatScope {
            chat_id: ChatId::new(),
            turn_id: TurnId::new(),
            mode: Mode::AutoEdit,
            attach_decided: false,
        },
    }
}

#[tokio::test]
async fn an_annotated_argument_is_sent_as_a_header_on_a_freshly_opened_session() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let url = serve(seen.clone()).await;
    // The tools are cached, as they are after an install, so nothing here would list them.
    let connector = McpConnector::new(
        "annotator".into(),
        "Annotator".into(),
        InstanceId::new(),
        ConnectorKind::McpRemote,
        Endpoint::Http {
            url,
            headers: Vec::new(),
            bearer: None,
        },
        Some(Vec::new()),
    );

    let outcome = connector
        .call(
            request(
                "get_file",
                serde_json::json!({ "owner": "oljodev", "path": "README.md" }),
            ),
            Arc::new(NoopToolEvents),
            CancellationToken::new(),
        )
        .await
        .expect("the call");
    let gantry_connectors::ToolOutcome::Complete {
        content, is_error, ..
    } = outcome;
    assert!(!is_error, "the server refused the call: {content:?}");
    assert_eq!(
        seen.lock().expect("the record").as_slice(),
        ["oljodev"],
        "the annotated argument did not travel as a header"
    );
    connector.stop().await;
}
