//! The connector as the turn loop sees it: which tools it offers, and what the refusals say.
//!
//! Every test here is offline, and most of them prove it: a URL that fails the scheme or address
//! check is refused *before* a socket is opened, so these run with no network at all and would
//! run the same on a machine that has none.

use std::sync::Arc;

use gantry_connector_web::{Web, definitions};
use gantry_connectors::{
    ChatScope, Connector, ConnectorError, NoopToolEvents, ToolCallRequest, ToolOutcome,
};
use gantry_core::{CallId, ChatId, InstanceId, Mode, ResultPart, TurnId};
use tokio_util::sync::CancellationToken;

fn web() -> Web {
    Web::new("web".to_owned(), InstanceId::new(), None)
}

async fn call(
    web: &Web,
    tool: &str,
    args: serde_json::Value,
) -> Result<ToolOutcome, ConnectorError> {
    web.call(
        ToolCallRequest {
            call_id: CallId::new(),
            tool: tool.to_owned(),
            args,
            scope: ChatScope {
                chat_id: ChatId::new(),
                turn_id: TurnId::new(),
                mode: Mode::Manual,
                attach_decided: false,
            },
        },
        Arc::new(NoopToolEvents),
        CancellationToken::new(),
    )
    .await
}

/// The error text a refusal put in front of the model.
fn refusal(outcome: &ToolOutcome) -> String {
    let ToolOutcome::Complete {
        content, is_error, ..
    } = outcome;
    assert!(is_error, "this outcome is not an error");
    content
        .iter()
        .map(|part| match part {
            ResultPart::Text { text } => text.clone(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[tokio::test]
async fn the_connector_offers_reading_locating_and_searching() {
    let names: Vec<String> = web()
        .tools()
        .await
        .unwrap()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, ["fetch_url", "find_in_page", "search"]);
}

#[tokio::test]
async fn an_empty_query_is_refused_before_anything_is_asked() {
    // Offline, and provably: with nothing to search for there is no request to make. What is
    // being checked is that the refusal is a sentence rather than an empty list — `[]` reads to
    // a model as proof the thing does not exist, and it will answer from memory and cite
    // nothing (`docs/connectors/web.md` §6.8).
    let outcome = call(
        &web(),
        "search",
        serde_json::json!({"query": "  ", "max_results": 5}),
    )
    .await
    .unwrap();
    let message = refusal(&outcome);
    assert!(message.contains("nothing to search for"), "{message}");
}

#[tokio::test]
async fn an_index_this_connector_does_not_have_names_the_ones_it_does() {
    let outcome = call(
        &web(),
        "search",
        serde_json::json!({"query": "anything", "source": "google"}),
    )
    .await
    .unwrap();
    let message = refusal(&outcome);
    assert!(message.contains("wikipedia"), "{message}");
    assert!(message.contains("crates.io"), "{message}");
}

#[tokio::test]
async fn a_tool_this_connector_does_not_have_is_an_unknown_tool() {
    let err = call(&web(), "crawl_site", serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::UnknownTool(name) if name == "crawl_site"));
}

#[tokio::test]
async fn a_missing_argument_is_an_invalid_argument() {
    let err = call(&web(), "fetch_url", serde_json::json!({}))
        .await
        .unwrap_err();
    assert!(matches!(err, ConnectorError::InvalidArgs(m) if m.contains("url")));
}

#[tokio::test]
async fn a_scheme_this_tool_does_not_fetch_is_refused_without_a_request() {
    for url in [
        "file:///etc/passwd",
        "ftp://files.example.com/x",
        "data:text/html,<b>hi</b>",
        "javascript:alert(1)",
    ] {
        let outcome = call(&web(), "fetch_url", serde_json::json!({"url": url}))
            .await
            .unwrap();
        let message = refusal(&outcome);
        assert!(
            message.contains("only fetches http and https"),
            "{url}: {message}"
        );
    }
}

#[tokio::test]
async fn this_machine_and_this_network_are_refused_without_a_request() {
    // The whole point of the address check, and the reason it is worth a test that could not
    // pass by accident: none of these opens a socket.
    for url in [
        "http://127.0.0.1:8080/admin",
        "http://[::1]:9200/",
        "http://169.254.169.254/latest/meta-data/iam/security-credentials/",
        "http://10.0.0.1/router",
        "http://192.168.1.1/",
        "http://[::ffff:127.0.0.1]/",
    ] {
        let outcome = call(&web(), "fetch_url", serde_json::json!({"url": url}))
            .await
            .unwrap();
        let message = refusal(&outcome);
        assert!(
            message.contains("this machine or this private network"),
            "{url}: {message}"
        );
    }
}

#[tokio::test]
async fn localhost_by_name_is_refused_too() {
    // Separate from the address literals because this one goes through the resolver, and a
    // machine whose hosts file lacks `localhost` would refuse it for the other reason. Either
    // refusal is correct; what must never happen is the fetch going through.
    let outcome = call(
        &web(),
        "fetch_url",
        serde_json::json!({"url": "http://localhost:5432/"}),
    )
    .await
    .unwrap();
    let message = refusal(&outcome);
    assert!(
        message.contains("this machine or this private network")
            || message.contains("could not be resolved"),
        "{message}"
    );
}

#[tokio::test]
async fn something_that_is_not_a_url_says_so() {
    let outcome = call(
        &web(),
        "fetch_url",
        serde_json::json!({"url": "example.com/page"}),
    )
    .await
    .unwrap();
    let message = refusal(&outcome);
    assert!(message.contains("is not a URL"), "{message}");
    assert!(
        message.contains("https://"),
        "it should show the shape it wants: {message}"
    );
}

#[tokio::test]
async fn an_unknown_format_names_the_three_that_exist() {
    let outcome = call(
        &web(),
        "fetch_url",
        serde_json::json!({"url": "https://example.com", "format": "pdf"}),
    )
    .await
    .unwrap();
    let message = refusal(&outcome);
    assert!(message.contains("markdown, text or html"), "{message}");
}

#[tokio::test]
async fn a_cancelled_call_stops_rather_than_waiting_out_the_timeout() {
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let outcome = web()
        .call(
            ToolCallRequest {
                call_id: CallId::new(),
                tool: "fetch_url".to_owned(),
                // A real address, never reached: the token is already cancelled.
                args: serde_json::json!({"url": "https://example.com/"}),
                scope: ChatScope {
                    chat_id: ChatId::new(),
                    turn_id: TurnId::new(),
                    mode: Mode::Manual,
                    attach_decided: false,
                },
            },
            Arc::new(NoopToolEvents),
            cancelled,
        )
        .await
        .unwrap();
    assert!(refusal(&outcome).contains("cancelled"));
    // Deterministic, and offline: `select!` is `biased`, so an already-cancelled token wins
    // before the fetch branch is polled. Without that this test resolved a real hostname in
    // roughly half its runs, which is the one thing the suite must never do.
}

#[test]
fn the_declared_schemas_agree_with_what_the_code_accepts() {
    for def in definitions() {
        let schema = &def.input_schema;
        assert_eq!(schema["type"], "object", "{}", def.name);
        // `additionalProperties: false` is what stops a model inventing an argument and getting
        // silence back for it.
        assert_eq!(schema["additionalProperties"], false, "{}", def.name);
        let required = schema["required"].as_array().expect("required");
        assert!(!required.is_empty(), "{}", def.name);
        for key in required {
            let key = key.as_str().unwrap();
            assert!(
                schema["properties"].get(key).is_some(),
                "{} requires `{key}` but does not describe it",
                def.name
            );
        }
    }
}

/// D15, held to the source: **Gantry identifies itself honestly and never impersonates a
/// browser.** It was not held — the general search engine was asked with a Chrome 131 user-agent
/// and a `Referer` naming a page that had never been loaded. §7.3 had already measured that the
/// honest header and the lie produce byte-identical outcomes on every blocking site, so the
/// impersonation bought nothing and cost the one posture this connector is built on.
///
/// A source scan rather than a live request, because what needs guarding is the next header
/// table somebody adds, and the network is not where that would be caught.
#[test]
fn nothing_this_connector_sends_claims_to_be_a_browser() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect(&src, &mut files);
    assert!(files.len() > 5, "the source was not found: {src:?}");
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        for lie in ["Mozilla/", "AppleWebKit/", "Chrome/", "Safari/"] {
            assert!(
                !text.contains(lie),
                "{} sends `{lie}`, which is a browser this is not (D15)",
                path.display()
            );
        }
    }
}

/// And what it does send names the product and where to read about it, so a site owner who
/// wants to refuse Gantry can.
#[test]
fn the_user_agent_says_who_it_is_and_where_to_ask() {
    assert!(
        gantry_connector_web::USER_AGENT.starts_with("GantryBot/"),
        "{}",
        gantry_connector_web::USER_AGENT
    );
    assert!(
        gantry_connector_web::USER_AGENT.contains("https://"),
        "{}",
        gantry_connector_web::USER_AGENT
    );
}

fn collect(dir: &std::path::Path, into: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, into);
        } else if path.extension().is_some_and(|e| e == "rs") {
            into.push(path);
        }
    }
}
