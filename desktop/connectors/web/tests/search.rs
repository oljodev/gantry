//! The search backends, against responses the real APIs actually sent.
//!
//! Every fixture in this file was captured from the live API on 2026-09-13 and saved verbatim,
//! not written from memory. That distinction is the whole point of the file: the connector's
//! previous search was built against hand-written fixtures and never once met a real response,
//! so its parsers were fiction that compiled. These are the bytes Wikipedia, Stack Exchange,
//! crates.io and the npm registry returned.
//!
//! Nothing here reaches the network. The requests are separate from the parsing for exactly
//! that reason: what can go wrong in a parser is the shape of a reply, and a recorded reply
//! tests it without a socket.
//!
//! One licensing note, since these are other people's words: `stackexchange.json` holds three
//! question bodies under CC BY-SA 4.0. The fixture is unedited, so each carries its own
//! `link`, `owner` and `content_license` — the attribution the licence asks for is in the file.
//! Trim the bodies if that ever needs to be tighter; the parsers only read the first paragraph.

use gantry_connector_web::{
    Hit, Source, parse_crate, parse_crates, parse_npm, parse_package, parse_stack, parse_wikipedia,
};

fn json(name: &str) -> serde_json::Value {
    let raw = std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|err| panic!("fixture {name}: {err}"));
    serde_json::from_str(&raw).unwrap_or_else(|err| panic!("fixture {name} is not JSON: {err}"))
}

fn one(hits: &[Hit], source: Source) -> &Hit {
    assert!(!hits.is_empty(), "no hits at all");
    assert!(
        hits.iter().all(|hit| hit.source == source),
        "a hit is labelled with the wrong index"
    );
    assert!(
        hits.iter().all(|hit| !hit.title.is_empty()),
        "a hit with no title is not a result anybody can use"
    );
    assert!(
        hits.iter().all(|hit| hit.url.starts_with("https://")),
        "every hit has to be followable with fetch_url: {:?}",
        hits.iter().map(|h| &h.url).collect::<Vec<_>>()
    );
    &hits[0]
}

#[test]
fn a_wikipedia_search_becomes_followable_hits() {
    let hits = parse_wikipedia(&json("wikipedia.json"));
    assert_eq!(hits.len(), 3);
    let first = one(&hits, Source::Wikipedia);
    assert_eq!(first.title, "Magnus Carlsen");
    // The API returns a page id and no URL, so the connector builds one — and `?curid=` is
    // followable where a title with a slash or an apostrophe in it would not be.
    assert_eq!(first.url, "https://en.wikipedia.org/?curid=442682");
    // The snippet arrives as highlighted HTML; none of that markup reaches the model.
    assert!(!first.snippet.contains('<'), "{}", first.snippet);
    assert!(first.snippet.contains("Magnus"), "{}", first.snippet);
}

#[test]
fn a_wikipedia_search_that_matched_nothing_is_no_hits_and_not_a_panic() {
    // Captured from the live API for a query with no matches: `search` is present and empty.
    assert!(parse_wikipedia(&json("wikipedia-empty.json")).is_empty());
}

#[test]
fn a_stack_overflow_question_carries_whether_it_was_answered() {
    let hits = parse_stack(&json("stackexchange.json"));
    assert_eq!(hits.len(), 3);
    let first = one(&hits, Source::StackOverflow);
    assert_eq!(
        first.title,
        "How do Rust async runtimes (e.g. tokio) handle channels?"
    );
    assert!(
        first
            .url
            .starts_with("https://stackoverflow.com/questions/")
    );
    // Which result to open is the decision this tool exists to inform, and "has an accepted
    // answer" is the single most useful fact for making it.
    assert!(
        first.snippet.starts_with("[accepted answer]"),
        "{}",
        first.snippet
    );
    assert!(!first.snippet.contains("<p>"), "{}", first.snippet);
}

#[test]
fn a_crate_hit_names_the_version_cargo_would_install() {
    let hits = parse_crates(&json("crates.json"));
    assert_eq!(hits.len(), 3);
    let first = one(&hits, Source::CratesIo);
    // `max_stable_version`, not `max_version`, which can be a pre-release nobody wants.
    assert_eq!(first.title, "tokio 1.53.1");
    assert_eq!(first.url, "https://crates.io/crates/tokio");
    assert!(
        first
            .snippet
            .starts_with("An event-driven, non-blocking I/O")
    );
    // The registry's description has the author's own line breaks in it; a result list is one
    // line per result.
    assert!(!first.snippet.contains('\n'), "{}", first.snippet);
}

#[test]
fn an_npm_hit_uses_the_registry_s_own_link() {
    let hits = parse_npm(&json("npm.json"));
    assert_eq!(hits.len(), 3);
    let first = one(&hits, Source::Npm);
    assert!(first.title.starts_with("react "), "{}", first.title);
    assert_eq!(first.url, "https://www.npmjs.com/package/react");
    assert!(
        first.snippet.contains("JavaScript library"),
        "{}",
        first.snippet
    );
}

#[test]
fn a_crate_looked_up_by_name_is_the_package_everybody_meant() {
    // The endpoint that fixes the defect this pairing exists for: searching crates.io for
    // "tokio async runtime" does not return `tokio` in thirty results, because its description
    // never says "runtime". Asking for the name directly always does.
    //
    // This fixture is the live reply with its `versions` array dropped — 195 entries and 440 KB
    // of publish history the parser never reads. The `crate` object it does read is verbatim.
    let hit = parse_crate(&json("crates-exact.json")).expect("tokio is a crate");
    assert_eq!(hit.source, Source::CratesIo);
    assert_eq!(hit.title, "tokio 1.53.1");
    assert_eq!(hit.url, "https://crates.io/crates/tokio");
    assert!(
        hit.snippet.starts_with("An event-driven"),
        "{}",
        hit.snippet
    );
    // The two endpoints have different envelopes and must not be read by the wrong parser.
    assert!(
        parse_crate(&json("crates.json")).is_none(),
        "that is the search shape"
    );
}

#[test]
fn a_package_looked_up_by_name_reads_the_bare_manifest() {
    // npm's by-name endpoint returns the manifest with no envelope at all, unlike its search.
    let hit = parse_package(&json("npm-exact.json")).expect("react is a package");
    assert_eq!(hit.source, Source::Npm);
    assert!(hit.title.starts_with("react 19"), "{}", hit.title);
    assert_eq!(hit.url, "https://www.npmjs.com/package/react");
    assert!(
        hit.snippet.contains("JavaScript library"),
        "{}",
        hit.snippet
    );
}

#[test]
fn a_body_in_the_wrong_shape_is_no_results_rather_than_a_panic() {
    // An API that changes, an error document, a proxy's login page. Every parser answers the
    // same way: nothing found, which `run` turns into a sentence.
    for body in [
        serde_json::json!({}),
        serde_json::json!([]),
        serde_json::json!({"query": {"search": "not an array"}}),
        serde_json::json!({"items": [{"no": "title"}]}),
        serde_json::json!({"crates": [{}]}),
        serde_json::json!({"objects": [{"package": {}}]}),
        serde_json::json!(null),
    ] {
        assert!(parse_wikipedia(&body).is_empty(), "{body}");
        assert!(parse_stack(&body).is_empty(), "{body}");
        assert!(parse_crates(&body).is_empty(), "{body}");
        assert!(parse_npm(&body).is_empty(), "{body}");
        assert!(parse_crate(&body).is_none(), "{body}");
        assert!(parse_package(&body).is_none(), "{body}");
    }
}

#[test]
fn the_shapes_do_not_bleed_into_each_other() {
    // Each parser reads its own envelope and nothing else, so a reply routed to the wrong
    // backend yields nothing instead of nonsense.
    let wiki = json("wikipedia.json");
    assert!(parse_stack(&wiki).is_empty());
    assert!(parse_crates(&wiki).is_empty());
    assert!(parse_npm(&wiki).is_empty());
    let crates = json("crates.json");
    assert!(parse_wikipedia(&crates).is_empty());
    assert!(parse_stack(&crates).is_empty());
}

/// The whole tool against the live indexes, which `cargo test` never runs.
///
/// `#[ignore]`d because every other test in this crate is offline and that property is worth
/// keeping absolute. Run by hand when a backend's API might have moved:
///
/// ```text
/// cargo test -p gantry-connector-web --test search -- --ignored --nocapture
/// ```
///
/// It needs no key, which is the point of the whole design: there is nothing to configure
/// before this passes.
#[tokio::test]
#[ignore = "reaches the live search APIs; run by hand"]
async fn a_live_search_returns_results_from_every_index() {
    use gantry_connector_web::Web;
    use gantry_core::InstanceId;

    let web = Web::new("web".to_owned(), InstanceId::new());
    for (query, expected) in [
        ("who is magnus carlsen", Source::Wikipedia),
        ("serde crate", Source::CratesIo),
        ("npm react", Source::Npm),
        (
            "cannot borrow as mutable more than once rust",
            Source::StackOverflow,
        ),
    ] {
        let hits = web.search_for(query, 6, None).await.unwrap_or_else(|err| {
            panic!("{query}: {err}");
        });
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

    // The case that motivated pairing search with a by-name lookup, and the one a model
    // actually hit: asked for crates.io directly with a descriptive phrase, crates.io's own
    // search does not return `tokio` in thirty results, because its description never says
    // "runtime". The lookup puts it first anyway.
    for (query, wanted) in [
        ("tokio async runtime", "tokio"),
        ("serde json serialization", "serde"),
    ] {
        let hits = web
            .search_for(query, 6, Some(Source::CratesIo))
            .await
            .unwrap_or_else(|err| panic!("{query}: {err}"));
        println!("\ncrates.io forced: {query}");
        for hit in &hits {
            println!("  {} — {}", hit.title, hit.url);
        }
        assert_eq!(
            hits.first()
                .map(|hit| hit.title.split(' ').next().unwrap_or_default()),
            Some(wanted),
            "{query} should lead with {wanted}"
        );
    }
}
