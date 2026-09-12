//! What actually goes on the wire to an authenticated server (docs/plan/03 §6).
//!
//! This exists because of a bug that cost two rounds of live testing: rmcp's `auth_header` takes
//! the *token* and writes the scheme itself, so passing a whole `Authorization` value sent
//! `Bearer Bearer …` and every authenticated server answered 401 — while the one connector in
//! the catalog that needs no account kept working, so nothing caught it. A server that records
//! its headers catches it in a tenth of a second.

use std::sync::{Arc, Mutex};

use gantry_connectors::mcp::{Endpoint, McpSession};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// A server that speaks just enough MCP to be connected to, and remembers what it was sent.
#[derive(Default)]
struct Seen {
    authorization: Vec<String>,
    other: Vec<(String, String)>,
}

async fn serve(seen: Arc<Mutex<Seen>>) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.expect("a port");
    let url = format!("http://{}/mcp", listener.local_addr().expect("an address"));
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let seen = seen.clone();
            tokio::spawn(async move {
                let mut buf = vec![0_u8; 16 * 1024];
                let read = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..read]).to_string();
                {
                    let mut seen = seen.lock().expect("the record");
                    for line in request.lines().skip(1) {
                        let Some((name, value)) = line.split_once(':') else {
                            continue;
                        };
                        let (name, value) = (name.trim().to_lowercase(), value.trim().to_owned());
                        if name == "authorization" {
                            seen.authorization.push(value);
                        } else if !name.is_empty() {
                            seen.other.push((name, value));
                        }
                    }
                }
                let body = request.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
                let _ = socket.write_all(answer(body).as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    url
}

/// The three answers a client's opening moves need. The request's own id is echoed: a client
/// that asked as 0 will not accept an answer addressed to 1.
fn answer(body: &str) -> String {
    let request: serde_json::Value = serde_json::from_str(body).unwrap_or(serde_json::Value::Null);
    let Some(id) = request.get("id") else {
        // A notification carries no id and wants no body back.
        return "HTTP/1.1 202 Accepted\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
            .to_owned();
    };
    let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let payload = if method == "initialize" {
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "serverInfo": { "name": "recorder", "version": "1.0.0" },
            },
        })
        .to_string()
    } else {
        // Anything newer than this server knows, `server/discover` included, is refused so the
        // client falls back to the handshake — which is what real servers do today.
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": "Method not found" },
        })
        .to_string()
    };
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
        payload.len()
    )
}

#[tokio::test]
async fn a_bearer_credential_arrives_once_with_one_scheme() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let url = serve(seen.clone()).await;

    let session = McpSession::connect(
        &Endpoint::Http {
            url,
            headers: Vec::new(),
            bearer: Some("Bearer secret-token".to_owned()),
        },
        Default::default(),
    )
    .await
    .expect("connecting to the recorder");
    session.close().await;

    let authorization = seen.lock().expect("the record").authorization.clone();
    assert!(!authorization.is_empty(), "the credential was not sent");
    for value in &authorization {
        assert_eq!(
            value, "Bearer secret-token",
            "the scheme was written twice, or the token was mangled"
        );
    }
}

#[tokio::test]
async fn a_credential_with_another_scheme_survives_intact() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let url = serve(seen.clone()).await;

    let session = McpSession::connect(
        &Endpoint::Http {
            url,
            headers: Vec::new(),
            bearer: Some("token ghp_classic".to_owned()),
        },
        Default::default(),
    )
    .await
    .expect("connecting to the recorder");
    session.close().await;

    let authorization = seen.lock().expect("the record").authorization.clone();
    assert!(
        authorization.iter().all(|v| v == "token ghp_classic"),
        "a non-Bearer credential must not be rewritten: {authorization:?}"
    );
}

#[tokio::test]
async fn a_configured_header_reaches_the_server() {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let url = serve(seen.clone()).await;

    let session = McpSession::connect(
        &Endpoint::Http {
            url,
            headers: vec![("X-Gantry-Test".to_owned(), "1".to_owned())],
            bearer: None,
        },
        Default::default(),
    )
    .await
    .expect("connecting to the recorder");
    session.close().await;

    let other = seen.lock().expect("the record").other.clone();
    assert!(
        other
            .iter()
            .any(|(name, value)| name == "x-gantry-test" && value == "1"),
        "the header was dropped: {other:?}"
    );
}
