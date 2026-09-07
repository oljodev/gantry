//! Connects to a real MCP server. Ignored by default: it needs the network, and a run of the
//! test suite must not depend on somebody else's uptime.
//!
//! ```sh
//! cargo test -p gantry-connectors --test live -- --ignored --nocapture
//! ```
//!
//! The server is Cloudflare's documentation server, chosen because it needs no account
//! (docs/plan/03 §7, "not every server needs auth"), so this proves the whole path — handshake,
//! version negotiation, tool listing, a call and its content — without a credential.

use gantry_connectors::mcp::{Endpoint, McpSession};

const DOCS: &str = "https://docs.mcp.cloudflare.com/mcp";

#[tokio::test]
#[ignore = "needs the network"]
async fn the_cloudflare_documentation_server_answers() {
    let endpoint = Endpoint::Http {
        url: DOCS.to_owned(),
        headers: Vec::new(),
        bearer: None,
    };
    let session = McpSession::connect(&endpoint)
        .await
        .expect("connecting to the documentation server");

    let server = session.server().clone();
    println!(
        "connected: {} {} over MCP {}",
        server.name, server.version, server.protocol
    );
    assert!(!server.protocol.is_empty(), "a version was negotiated");

    let tools = session.tools(true).await.expect("listing tools");
    println!("{} tools", tools.len());
    for tool in &tools {
        println!("  {} · {:?}", tool.name, tool.tier);
    }
    assert!(!tools.is_empty(), "the server offers tools");

    // A search tool exists under some name on this server; call the first one that takes a
    // query and check that something comes back as text.
    let searcher = tools
        .iter()
        .find(|t| t.name.contains("search") || t.name.contains("docs"))
        .expect("a documentation tool");
    let args = serde_json::json!({ "query": "workers kv", "q": "workers kv" });
    let (content, _structured, is_error) = session
        .call(&searcher.name, &args)
        .await
        .expect("calling the tool");
    println!(
        "{} → {} part(s), error: {is_error}",
        searcher.name,
        content.len()
    );
    assert!(!content.is_empty(), "the call returned content");

    session.close().await;
}
