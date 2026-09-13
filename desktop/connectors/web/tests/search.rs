//! What each search service answers, and what this connector makes of it.
//!
//! Recorded bodies, parsed offline. No key is read and no request is made: `parse_hits` is the
//! half of `search` that can be wrong in an interesting way, and it is pure. The half that
//! cannot be tested without an account is the request itself, and that is the `#[ignore]` smoke
//! test at the bottom — run by hand, with a real key, by someone who has one.

use gantry_connector_web::{Hit, Provider, parse_hits};

fn json(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw).expect("a fixture that is valid JSON")
}

#[test]
fn brave_results_become_hits() {
    let hits = parse_hits(Provider::Brave, &json(include_str!("fixtures/brave.json")));
    assert_eq!(hits.len(), 3);
    assert_eq!(
        hits[0],
        Hit {
            title: "Bounded fetching — Example Engineering Blog".into(),
            url: "https://example.com/blog/bounded-fetching".into(),
            // The <strong> Brave wraps the matched terms in is gone.
            snippet: "A fetcher without limits is a denial of service you wrote yourself.".into(),
        }
    );
    // Whitespace in a snippet is collapsed, newlines included: it is one line in a result list.
    assert_eq!(
        hits[1].snippet,
        "The header is a claim. A server can send a small one and then a very large body."
    );
    // A result with no title falls back to its URL rather than being dropped or left blank.
    assert_eq!(hits[2].title, "https://example.net/untitled");
}

#[test]
fn tavily_results_become_hits() {
    let hits = parse_hits(
        Provider::Tavily,
        &json(include_str!("fixtures/tavily.json")),
    );
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].url, "https://example.com/blog/bounded-fetching");
    assert_eq!(
        hits[0].snippet,
        "A fetcher without limits is a denial of service you wrote yourself."
    );
    assert_eq!(hits[1].snippet, "Read the stream and stop counting.");
}

#[test]
fn exa_results_become_hits_from_text_or_summary() {
    let hits = parse_hits(Provider::Exa, &json(include_str!("fixtures/exa.json")));
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].title, "Bounded fetching");
    assert!(hits[0].snippet.starts_with("A fetcher without limits"));
    // Exa returns `text` or `summary` depending on what was asked for; either is the snippet.
    assert_eq!(
        hits[1].snippet,
        "One byte per second, forever, stays under every size cap there is."
    );
}

#[test]
fn a_body_in_the_wrong_shape_is_no_results_rather_than_a_panic() {
    // An error body, an empty object, the wrong provider's shape, a null — every one of these
    // can arrive on a 200 from a service having a bad day.
    for (provider, body) in [
        (Provider::Brave, include_str!("fixtures/brave-error.json")),
        (Provider::Brave, "{}"),
        (Provider::Tavily, r#"{"results": null}"#),
        (Provider::Tavily, r#"{"results": [{"no_url": true}]}"#),
        (Provider::Exa, r#"{"results": []}"#),
        (Provider::Exa, "null"),
    ] {
        assert!(parse_hits(provider, &json(body)).is_empty(), "{body}");
    }
}

#[test]
fn the_shapes_do_not_bleed_into_each_other() {
    // Brave nests its results under `web`; reading a Brave body as Tavily must find nothing
    // rather than half-parsing it.
    let brave = json(include_str!("fixtures/brave.json"));
    assert!(parse_hits(Provider::Tavily, &brave).is_empty());
    let tavily = json(include_str!("fixtures/tavily.json"));
    assert!(parse_hits(Provider::Brave, &tavily).is_empty());
}

/// A real call to a real service, run by hand:
///
/// ```text
/// GANTRY_SEARCH_PROVIDER=brave GANTRY_SEARCH_KEY=… \
///   cargo test -p gantry-connector-web --test search -- --ignored --nocapture
/// ```
///
/// `#[ignore]` because it spends the key-holder's quota and needs a network. `cargo test` on its
/// own never runs it, which is the point: no key is read, and nothing is charged to anybody, by
/// the test suite this repository runs on every commit.
#[tokio::test]
#[ignore = "needs a real search key and the network; run by hand"]
async fn a_live_search_returns_results() {
    use gantry_connector_web::Search;
    use secrecy::SecretString;

    let provider = std::env::var("GANTRY_SEARCH_PROVIDER").unwrap_or_else(|_| "brave".into());
    let key = std::env::var("GANTRY_SEARCH_KEY")
        .expect("set GANTRY_SEARCH_KEY to the key you want to spend");
    let search = Search::from_config(Some(&provider), Some(SecretString::from(key)))
        .expect("GANTRY_SEARCH_PROVIDER must be brave, tavily or exa");

    let http = reqwest::Client::builder()
        .user_agent(gantry_connector_web::USER_AGENT)
        .build()
        .unwrap();
    let hits = search
        .run(&http, "gantry connector system", 3)
        .await
        .expect("the search should succeed");
    for hit in &hits {
        println!("{}\n  {}\n  {}\n", hit.title, hit.url, hit.snippet);
    }
    assert!(!hits.is_empty(), "a live search returned nothing");
    assert!(hits.iter().all(|h| h.url.starts_with("http")));
}
