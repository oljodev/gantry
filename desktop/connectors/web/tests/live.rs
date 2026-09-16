//! The connector against the real internet. `cargo test` never runs any of it.
//!
//! Every other test in this crate is offline and that property is worth keeping absolute, so
//! these are all `#[ignore]`d. Run them by hand when an API might have moved, or after touching
//! anything that builds a request:
//!
//! ```text
//! cargo test -p gantry-connector-web --test live -- --ignored --nocapture
//! ```
//!
//! They need no key. That is the point of the design, and it is also what makes this file
//! runnable by anyone who checks the repository out.
//!
//! These exist because recorded fixtures cannot catch a whole class of bug. A fixture proves a
//! parser reads a reply; it cannot tell you the *request* was wrong. Both registry defects —
//! searching for the word that routed the query, and running whole sentences through a package
//! index — were found here and by nothing else.

use std::sync::Arc;

use gantry_connector_web::{Source, Web};
use gantry_connectors::{ChatScope, Connector, NoopToolEvents, ToolCallRequest, ToolOutcome};
use gantry_core::{CallId, ChatId, InstanceId, Mode, TurnId};
use tokio_util::sync::CancellationToken;

fn web() -> Web {
    Web::new("web".to_owned(), InstanceId::new(), None)
}

/// One tool call, as the turn loop would make it.
async fn call(web: &Web, tool: &str, args: serde_json::Value) -> serde_json::Value {
    let outcome = web
        .call(
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
        .expect("the call itself should not fail");
    let ToolOutcome::Complete {
        structured,
        is_error,
        ..
    } = &outcome;
    assert!(!is_error, "{tool} failed: {outcome:?}");
    structured.clone().expect("a structured result")
}

#[tokio::test]
#[ignore = "reaches the live search APIs; run by hand"]
async fn a_live_search_returns_results_from_every_index() {
    let web = web();
    for (query, expected) in [
        ("who is magnus carlsen", Source::Wikipedia),
        ("serde crate", Source::CratesIo),
        ("npm react", Source::Npm),
        (
            "cannot borrow as mutable more than once rust",
            Source::StackOverflow,
        ),
    ] {
        let answer = web
            .search_for(query, 6, None)
            .await
            .unwrap_or_else(|err| panic!("{query}: {err}"));
        let hits = answer.hits;
        if !answer.unavailable.is_empty() {
            println!("  (unavailable: {})", answer.unavailable.join("; "));
        }
        println!("\n{query} -> {} hits", hits.len());
        for hit in &hits {
            println!("  [{}] {} — {}", hit.source.label(), hit.title, hit.url);
        }
        assert!(
            hits.iter().any(|hit| hit.source == expected),
            "{query} returned nothing from {}",
            expected.label()
        );
    }
}

#[tokio::test]
#[ignore = "reaches crates.io; run by hand"]
async fn a_named_package_leads_even_when_the_registry_search_buries_it() {
    // The defect a model found in real use, and the reason searching is paired with a by-name
    // lookup: crates.io's own search does not return `tokio` for this in thirty results,
    // because tokio's description never says "runtime".
    let web = web();
    for (query, wanted) in [
        ("tokio async runtime", "tokio"),
        ("serde json serialization", "serde"),
    ] {
        let hits = web
            .search_for(query, 6, Some(Source::CratesIo))
            .await
            .unwrap_or_else(|err| panic!("{query}: {err}"))
            .hits;
        println!("\ncrates.io: {query}");
        for hit in &hits {
            println!("  {} — {}", hit.title, hit.url);
        }
        assert_eq!(
            hits.first()
                .map(|hit| hit.title.split(' ').next().unwrap_or_default()),
            Some(wanted),
            "{query} should lead with {wanted}"
        );
        // And the incidental namesakes — `runtime` at 0.0.0, `serialization` at seventeen
        // thousand downloads — should not have taken a slot.
        for junk in ["runtime ", "serialization "] {
            assert!(
                !hits.iter().any(|hit| hit.title.starts_with(junk)),
                "{query} wasted a slot on {junk}"
            );
        }
    }
}

#[tokio::test]
#[ignore = "fetches a real page; run by hand"]
async fn a_long_page_is_located_first_and_then_read_at_the_right_place() {
    // The whole chain the two tools exist to make possible, against a page big enough for it to
    // matter: index it, pick a section, read exactly that. This is the sequence a model runs,
    // and every offset in it crosses a tool boundary.
    let web = web();
    let url = "https://en.wikipedia.org/wiki/James_Webb_Space_Telescope";

    let outline = call(&web, "find_in_page", serde_json::json!({ "url": url })).await;
    let headings = outline["headings"].as_array().expect("headings");
    let total = outline["total_chars"].as_u64().expect("a length");
    println!(
        "\n{url}\n  {} characters, {} headings",
        total,
        headings.len()
    );
    assert!(total > 100_000, "the fixture page should be long: {total}");
    assert!(headings.len() > 5, "and structured: {}", headings.len());

    // A heading from the far end of the page: the part sequential paging reaches last.
    let late = headings.last().expect("a heading");
    let offset = late["offset"].as_u64().expect("an offset");
    println!("  jumping to {:?} at {offset}", late["title"]);
    assert!(
        offset > total / 2,
        "the last heading should be in the back half"
    );

    let window = call(
        &web,
        "fetch_url",
        serde_json::json!({ "url": url, "offset": offset, "max_chars": 400 }),
    )
    .await;
    // Read from exactly the offset the index gave, and the heading is the first thing there.
    let content = window["content"].as_str().expect("content");
    println!("  read: {:?}", content.chars().take(60).collect::<String>());
    assert!(
        content.contains(late["title"].as_str().expect("a title")),
        "reading from {offset} did not land on {:?}: {content:?}",
        late["title"]
    );
    assert_eq!(window["first_char"].as_u64(), Some(offset));
    assert_eq!(window["total_chars"].as_u64(), Some(total));
    // The second tool call reused the first one's download.
    assert_eq!(window["cached"].as_bool(), Some(true));

    // And a pattern finds its way to a section by name.
    let found = call(
        &web,
        "find_in_page",
        serde_json::json!({ "url": url, "pattern": "infrared" }),
    )
    .await;
    let matches = found["matches"].as_array().expect("matches");
    println!("  `infrared` occurs on {} lines", matches.len());
    assert!(
        !matches.is_empty(),
        "the article is about an infrared telescope"
    );
    assert_eq!(found["cached"].as_bool(), Some(true));
}

#[tokio::test]
#[ignore = "spends a real general-search query; run by hand and sparingly"]
async fn the_open_web_answers_what_the_curated_indexes_do_not_hold() {
    // Documentation, release notes, a blog post — the model's complaint about the four-index
    // version, and what the general tier exists for. Run sparingly: DuckDuckGo blocks after
    // five or six queries in two minutes, which is the whole reason this tier is rationed.
    let web = web();
    let answer = web
        .search_for("MDN fetch API abort signal", 8, None)
        .await
        .expect("something should answer");
    let hits = answer.hits;
    if !answer.unavailable.is_empty() {
        println!("  (unavailable: {})", answer.unavailable.join("; "));
    }
    println!("\nMDN fetch API abort signal -> {} hits", hits.len());
    for hit in &hits {
        println!("  [{}] {} — {}", hit.source.label(), hit.title, hit.url);
    }
    assert!(
        hits.iter()
            .any(|hit| hit.source == Source::DuckDuckGo || hit.source == Source::Mwmbl),
        "no general result at all"
    );

    // The ration is real: a second general query straight away must not reach the engine, and
    // must not fail either — the independent index answers instead.
    let again = web
        .search_for(
            "postgresql generated columns documentation",
            8,
            Some(Source::DuckDuckGo),
        )
        .await
        .expect("the fallback should answer rather than failing")
        .hits;
    println!("\nimmediately again -> {} hits", again.len());
    for hit in &again {
        println!("  [{}] {}", hit.source.label(), hit.title);
    }
    // The claim is that the *engine* was not asked, which is what the ration is for. It is
    // stated that way round on purpose: mwmbl is a much thinner index and a plausible answer
    // for it is nothing at all, so asserting it returned something would be asserting a
    // property of somebody else's crawl.
    assert!(
        !again.iter().any(|hit| hit.source == Source::DuckDuckGo),
        "the second query inside the gap reached the engine anyway"
    );
    if again.is_empty() {
        println!("  (mwmbl had nothing for it — thin index, no recency, per §6.5)");
    }
}
