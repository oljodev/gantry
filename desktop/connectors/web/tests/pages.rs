//! What `fetch_url` makes of a page, driven by recorded HTML.
//!
//! Offline on purpose, and not as a compromise: the network is not the part worth testing here.
//! What breaks a reader is a site whose furniture is shaped differently from the last one's, and
//! a fixture reproduces that exactly and for ever, where a live fetch reproduces whatever the
//! site looks like today. `cargo test` never opens a socket.

use gantry_connector_web::{Format, article};

const ARTICLE: &str = include_str!("fixtures/article.html");
const BARE: &str = include_str!("fixtures/bare.html");
const NO_TITLE: &str = include_str!("fixtures/no-title.html");

#[test]
fn the_article_survives_and_the_furniture_does_not() {
    let page = article(ARTICLE, Format::Markdown);

    // The prose is all there.
    assert!(page.body.contains("A fetcher without limits"));
    assert!(page.body.contains("Read the stream and stop counting"));
    assert!(page.body.contains("one byte per"));
    assert!(page.body.contains("Cap the bytes read"));

    // And the page around it is not.
    for furniture in [
        "We value your privacy", // cookie banner
        "Pricing",               // primary navigation
        "we are hiring",         // navigation, long enough to outweigh a paragraph
        "Related posts",         // sidebar
        "Subscribe to our",      // newsletter prompt
        "42 comments",           // comment thread
        "First!",                // comment body
        "All rights reserved",   // footer
        "Share on social media", // share rail
        "window.analytics",      // script
        "font-family",           // style
    ] {
        assert!(
            !page.body.contains(furniture),
            "{furniture:?} should have been stripped\n---\n{}\n---",
            page.body
        );
    }
}

#[test]
fn the_title_is_the_one_written_for_people() {
    // og:title over <title>, which here carries the site name welded on after a dash.
    let page = article(ARTICLE, Format::Markdown);
    assert_eq!(page.title.as_deref(), Some("Bounded fetching"));
}

#[test]
fn a_page_with_no_title_element_falls_back_to_its_heading() {
    let page = article(NO_TITLE, Format::Markdown);
    // Collapsed, too: the heading in the fixture is written with runs of spaces in it.
    assert_eq!(page.title.as_deref(), Some("The heading stands in"));
}

#[test]
fn markdown_keeps_the_structure_and_text_does_not() {
    let markdown = article(ARTICLE, Format::Markdown).body;
    let text = article(ARTICLE, Format::Text).body;

    assert!(
        markdown.contains("# Bounded fetching") || markdown.contains("Bounded fetching\n="),
        "the heading should still be a heading:\n{markdown}"
    );
    assert!(
        markdown.contains("- Cap the bytes read") || markdown.contains("* Cap the bytes read"),
        "the list should still be a list:\n{markdown}"
    );

    // Same prose, no markers.
    assert!(text.contains("Cap the bytes read"));
    assert!(!text.contains("# Bounded fetching"));
    assert!(!text.contains("- Cap the bytes read"));
}

#[test]
fn html_comes_back_as_the_extracted_subtree_and_not_the_whole_page() {
    let html = article(ARTICLE, Format::Html).body;
    assert!(html.contains("<h1>Bounded fetching</h1>"));
    assert!(html.contains("<li>"));
    // Still the extraction, not the original document.
    assert!(!html.contains("primary-nav"));
    assert!(!html.contains("<script"));
}

#[test]
fn a_page_that_is_only_a_div_still_returns_its_sentences() {
    // Nothing scores as an article container here. Returning an empty body would be the
    // technically-correct answer and a useless one.
    let page = article(BARE, Format::Markdown);
    assert!(
        page.body.contains("written by hand in 2003"),
        "{}",
        page.body
    );
    assert_eq!(page.title.as_deref(), Some("Notes"));
}

#[test]
fn blank_lines_do_not_pile_up() {
    let page = article(ARTICLE, Format::Markdown);
    assert!(
        !page.body.contains("\n\n\n"),
        "reader output is mostly whitespace before it is tidied:\n{}",
        page.body
    );
    assert_eq!(page.body.trim(), page.body, "no leading or trailing blank");
}

#[test]
fn an_empty_document_is_not_a_panic() {
    for input in [
        "",
        "   ",
        "<html></html>",
        "<!DOCTYPE html>",
        "not html at all",
    ] {
        let page = article(input, Format::Markdown);
        // Whatever it decides, it decides it without unwrapping on nothing.
        let _ = page.body;
    }
}

#[test]
fn a_page_cut_to_a_budget_says_that_it_was_cut() {
    use gantry_connector_web::clamp;

    let body = article(ARTICLE, Format::Markdown).body;
    assert!(
        body.chars().count() > 200,
        "the fixture should be long enough to cut"
    );

    // Under the budget: returned whole, and reported as whole.
    let (whole, cut) = clamp(&body, 100_000);
    assert_eq!(whole, body);
    assert!(!cut);

    // Over it: shortened, and reported as shortened. Nothing is ever silently dropped (D9).
    let (short, cut) = clamp(&body, 200);
    assert!(cut);
    assert!(short.chars().count() <= 200);
    assert!(short.starts_with("# Bounded fetching"));
    // Cut on a line boundary where there is one late enough to keep most of the budget, so the
    // model is not handed half a word.
    assert_eq!(short.trim_end(), short);

    // The degenerate budgets do not panic or produce something longer than they asked for.
    for budget in [1, 2, 3] {
        let (tiny, cut) = clamp(&body, budget);
        assert!(cut);
        assert!(tiny.chars().count() <= budget);
    }
}

const STRAY: &str = include_str!("fixtures/stray.html");
const LISTING: &str = include_str!("fixtures/listing.html");

#[test]
fn an_article_element_is_not_outscored_by_the_page_that_contains_it() {
    // `body` contains the `<article>`, so `body` can never score lower — and here a promo rail
    // whose class matches none of the furniture words is enough to tip it. An author who wrote
    // `<article>` has said where the content is; that is not a guess to be outvoted.
    let page = article(STRAY, Format::Text);
    assert!(
        page.body
            .contains("This is the piece somebody asked to read")
    );
    assert!(
        !page.body.contains("promotional rail"),
        "the rail outside the article came back with it:\n{}",
        page.body
    );
    assert!(
        !page.body.contains("Elsewhere on the site"),
        "{}",
        page.body
    );
}

#[test]
fn a_short_article_is_still_the_article() {
    // The obvious fix for the case above — trust `<article>` only past some length — quietly
    // breaks every short page, so length is not the test.
    let page = article(STRAY, Format::Text);
    assert!(
        page.body.chars().count() < 200,
        "the fixture is short: {}",
        page.body
    );
    assert!(!page.body.contains("promotional rail"));
}

#[test]
fn an_index_of_teasers_returns_all_of_them() {
    // Several `<article>` elements are a list, where no single one is the page. Picking the
    // best-scoring teaser would answer "what is on this page" with a third of it.
    let page = article(LISTING, Format::Text);
    for post in ["The first post", "The second post", "The third post"] {
        assert!(
            page.body.contains(post),
            "{post} is missing:\n{}",
            page.body
        );
    }
}
