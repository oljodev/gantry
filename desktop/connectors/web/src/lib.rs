//! First-party connector: Web (`docs/plan/03-connector-system.md` §5).
//!
//! Two tools: read a page, and search. Unlike the other three first-party connectors this one
//! touches nothing on disk, so it does not sit on `gantry-workspace` and has no roots to enforce. Its
//! boundary is the other way round: what it may *reach*. `guard` holds that — http and https
//! only, and nothing that resolves inside this machine or this network, rechecked on every
//! redirect.
//!
//! A page longer than the budget comes back a window at a time rather than cut off, and `cache`
//! holds the document in between so turning the page is neither a second download nor a second
//! chance for the offsets to have gone stale.
//!
//! **Search is keyless by rule.** This connector is free and local: no API key field, no
//! account, no quota to buy, nothing that turns a search into a bill. `search` asks the indexes
//! that need no account and are better than a general engine at their own subject — Wikipedia,
//! Stack Overflow, crates.io, npm — and says so when a question falls outside all of them
//! rather than answering it badly. `docs/connectors/web.md` §6 has the rest of the road: the
//! user's own SearXNG, a rationed general engine, independent indexes. An earlier build took a
//! Brave, Tavily or Exa key; it was removed because a key field is the one thing this connector
//! may not have.

#![forbid(unsafe_code)]

mod cache;
mod extract;
mod fetch;
mod guard;
mod search;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::{InstanceId, RiskTier, ToolDef};
use tokio_util::sync::CancellationToken;

pub use cache::{Cache, MAX_CHARS as CACHE_MAX_CHARS, MAX_PAGES, Page, TTL as CACHE_TTL};
pub use extract::{Article, Format, Window, article, nests_too_deep, window};
pub use fetch::{MAX_BYTES, MAX_REDIRECTS, TIMEOUT, USER_AGENT};
pub use search::{
    DEFAULT_RESULTS, Hit, MAX_RESULTS, SearchError, Source, lookups, parse_crate, parse_crates,
    parse_npm, parse_package, parse_stack, parse_wikipedia, registry_query, route, sources_of,
};

/// The connector manifest, embedded at build time (`docs/plan/03-connector-system.md` §3).
pub const MANIFEST: &str = include_str!("../manifest.json");

/// The connector id; equals the folder name and the tool namespace prefix.
pub const ID: &str = "web";

/// Characters of page text returned in one window when the model does not say. A long article
/// is 20–40 000; this fits most of them whole while keeping a single fetch from eating a
/// context window. Anything past it is a page turn away, not lost.
pub const DEFAULT_MAX_CHARS: usize = 40_000;

/// The ceiling on `max_chars`. Past this the answer is not a page to read but a file to search,
/// and 5 MB of text through a prompt helps nobody. A model that wants more asks for the next
/// window.
pub const MAX_MAX_CHARS: usize = 200_000;

pub struct Web {
    descriptor: ConnectorDescriptor,
    http: reqwest::Client,
    /// What has been read recently, so a page turn is free. See `cache`.
    pages: Cache,
}

impl Web {
    #[must_use]
    pub fn new(namespace: String, instance_id: InstanceId) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: namespace,
                name: "Web".to_owned(),
                instance_id: Some(instance_id),
                first_party: true,
            },
            http: fetch::client(),
            pages: Cache::new(),
        }
    }
}

#[async_trait]
impl Connector for Web {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        Ok(definitions())
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        _sink: Arc<dyn ToolEventSink>,
        cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        match req.tool.as_str() {
            "fetch_url" => {
                // Cancelling a turn should stop a 20-second fetch with it, rather than leaving
                // the user waiting on a result nobody will read.
                // `biased` so an already-cancelled token wins deterministically rather than by
                // a coin flip. Without it a cancelled call still had a chance to open the
                // connection before noticing, which is both a wasted request and a test that
                // reaches the network in half of its runs.
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => Ok(ToolOutcome::error("the fetch was cancelled.")),
                    outcome = self.fetch_url(&req.args) => outcome,
                }
            }
            "search" => {
                tokio::select! {
                    biased;
                    () = cancel.cancelled() => Ok(ToolOutcome::error("the search was cancelled.")),
                    outcome = self.search(&req.args) => outcome,
                }
            }
            other => Err(ConnectorError::UnknownTool(other.to_owned())),
        }
    }
}

impl Web {
    async fn fetch_url(&self, args: &serde_json::Value) -> Result<ToolOutcome, ConnectorError> {
        let url = required(args, "url")?;
        let format = match args.get("format").and_then(serde_json::Value::as_str) {
            None => Format::default(),
            Some(name) => match Format::parse(name) {
                Some(format) => format,
                None => {
                    return Ok(ToolOutcome::error(format!(
                        "`{name}` is not a format. Use markdown, text or html."
                    )));
                }
            },
        };
        let offset = number(args, "offset").unwrap_or(0) as usize;
        let max_chars = number(args, "max_chars")
            .map_or(DEFAULT_MAX_CHARS, |n| (n as usize).clamp(1, MAX_MAX_CHARS));

        // The second window of a document, and the second read of the same page, both land
        // here and neither touches the network.
        if let Some((page, age)) = self.pages.get(&url, format) {
            return Ok(render(&page, format, offset, max_chars, Some(age)));
        }

        let fetched = match fetch::get(&self.http, &url).await {
            Ok(fetched) => fetched,
            Err(err) => return Ok(ToolOutcome::error(err.to_string())),
        };

        let mime = fetch::mime(fetched.content_type.as_deref());
        if !fetch::is_readable(&mime) {
            return Ok(ToolOutcome::error(format!(
                "{} is {mime}, which is not text this tool can read. It is {} bytes{}.",
                fetched.final_url,
                fetched.body.len(),
                if fetched.truncated { " or more" } else { "" }
            )));
        }

        // Lossy, and deliberately: a page whose bytes are not UTF-8 is still mostly readable as
        // UTF-8, and a connector that refuses it outright is less useful than one that returns
        // the text with a few replacement characters in it.
        let text = String::from_utf8_lossy(&fetched.body).into_owned();
        let html = fetch::is_html(&mime) || looks_like_html(&text);
        if html && extract::nests_too_deep(&text) {
            return Ok(ToolOutcome::error(format!(
                "{} nests HTML too deeply to read. Parsing it would cost more time than any page \
                 is worth; this is a property of the page, not of the address.",
                fetched.final_url
            )));
        }

        // Reading a page is CPU work — parsing up to 5 MB of HTML and walking the tree — and it
        // does not belong on an async worker: nothing in it awaits, so the `select!` above could
        // not interrupt it and the runtime thread would be held for the whole of it.
        let (title, body) = if html {
            let owned = text;
            tokio::task::spawn_blocking(move || {
                let article = extract::article(&owned, format);
                (article.title, article.body)
            })
            .await
            .map_err(|err| ConnectorError::Failed(format!("reading the page failed: {err}")))?
        } else {
            (None, text.trim().to_owned())
        };

        let page = Arc::new(Page::new(
            fetched.final_url.to_string(),
            fetched.status,
            mime,
            title,
            fetched.redirects,
            fetched.truncated,
            body,
        ));
        self.pages.put(&url, format, Arc::clone(&page));
        Ok(render(&page, format, offset, max_chars, None))
    }

    /// The search behind the tool, without the JSON envelope around it.
    ///
    /// Public so the live test can exercise the real backends through the real client. Nothing
    /// in the app calls it: the tool is how a model reaches this.
    ///
    /// # Errors
    /// When no index covers the query, none had anything, or none could be reached.
    pub async fn search_for(
        &self,
        query: &str,
        limit: usize,
        source: Option<Source>,
    ) -> Result<Vec<Hit>, SearchError> {
        search::run(&self.http, query, limit, source).await
    }

    async fn search(&self, args: &serde_json::Value) -> Result<ToolOutcome, ConnectorError> {
        let query = required(args, "query")?;
        let limit = number(args, "max_results").map_or(DEFAULT_RESULTS, |n| n as usize);
        let only = match args.get("source").and_then(serde_json::Value::as_str) {
            None | Some("auto") => None,
            Some(name) => match Source::parse(name) {
                Some(source) => Some(source),
                None => {
                    return Ok(ToolOutcome::error(format!(
                        "`{name}` is not one of the indexes this connector can search. Use \
                         wikipedia, stackoverflow, crates.io, npm, or leave it out to let the \
                         query decide."
                    )));
                }
            },
        };

        let hits = match self.search_for(&query, limit, only).await {
            Ok(hits) => hits,
            // Every one of these is a sentence saying what to do next, not an empty list: a
            // model handed `[]` concludes the thing does not exist (`search` §6.8).
            Err(err) => return Ok(ToolOutcome::error(err.to_string())),
        };

        Ok(ToolOutcome::json(serde_json::json!({
            "query": query,
            // Which index answered, per result and in summary. A model should know it is
            // quoting Stack Overflow rather than an encyclopedia before it does.
            "searched": search::sources_of(&hits),
            "results": hits.iter().map(|hit| serde_json::json!({
                "title": hit.title,
                "url": hit.url,
                "snippet": hit.snippet,
                "source": hit.source.label(),
            })).collect::<Vec<_>>(),
            "count": hits.len(),
        })))
    }
}

/// One window of a page, as the model sees it.
///
/// Separate from the fetch because everything here is a pure function of a document that has
/// already been read — which is what lets the shape of the result, page turns included, be
/// tested without reaching the network.
fn render(
    page: &Page,
    format: Format,
    offset: usize,
    max_chars: usize,
    age: Option<Duration>,
) -> ToolOutcome {
    let view = extract::window(&page.body, offset, max_chars);
    let chars = view.text.chars().count();
    let mut out = serde_json::json!({
        "url": page.final_url,
        "status": page.status,
        "content_type": page.content_type,
        "format": match format {
            Format::Markdown => "markdown",
            Format::Text => "text",
            Format::Html => "html",
        },
        "content": view.text,
        "chars": chars,
        // Where this window sits in the document, so a model can say where a quotation came
        // from and can ask for the next part without guessing (D9: nothing is silently cut).
        "first_char": view.first,
        "total_chars": view.total,
        "more": view.more(),
        // A different loss, and one a page turn cannot fix: the response itself was cut at the
        // 5 MB cap, so the document is short of the real page.
        "response_truncated": page.response_truncated,
    });
    let fields = out.as_object_mut().expect("a json object");
    if view.more() {
        // Named rather than left as arithmetic on `first_char` + `chars`, which is off by
        // however much the window backed up to end on a whole line.
        fields.insert("next_offset".into(), view.last.into());
    }
    if let Some(title) = &page.title {
        fields.insert("title".into(), title.clone().into());
    }
    if !page.redirects.is_empty() {
        fields.insert("redirects".into(), page.redirects.clone().into());
    }
    if let Some(age) = age {
        // Said plainly, because it is the one thing about this result that is not what the
        // network would say right now. A model asked to check whether a page has changed can
        // see that this copy would not show it.
        fields.insert("cached".into(), true.into());
        fields.insert("cached_seconds_ago".into(), age.as_secs().into());
    }
    if chars == 0 && view.total > 0 {
        fields.insert(
            "note".into(),
            format!(
                "offset {offset} is past the end of this page, which is {} characters.",
                view.total
            )
            .into(),
        );
    }

    // An HTTP error still has a body, and a 404 page's text is often the useful part of the
    // answer ("moved to /docs/new"). It comes back as an error the model can read, with the
    // status named, rather than as a success that pretends nothing happened.
    if !(200..300).contains(&page.status) {
        // `is_error` so the turn loop and the feed both show it as a failure, with the page
        // body still attached for the model to read.
        return ToolOutcome::Complete {
            content: vec![gantry_core::ResultPart::Json { json: out.clone() }],
            structured: Some(out),
            is_error: true,
        };
    }
    ToolOutcome::json(out)
}

/// A body served as `text/plain` that is plainly HTML, which servers do more often than they
/// should. Cheap enough to be worth trying before handing markup to the model as prose.
fn looks_like_html(text: &str) -> bool {
    // Counted in characters, not bytes. `&head[..1024]` panics when the 1024th byte lands inside
    // a multi-byte character, which any page with an em dash or a non-Latin script early in it
    // can arrange — and this runs on whatever the network returned.
    let head: String = text
        .trim_start()
        .chars()
        .take(1024)
        .collect::<String>()
        .to_lowercase();
    head.starts_with("<!doctype html") || head.starts_with("<html") || head.contains("<body")
}

fn required(args: &serde_json::Value, key: &str) -> Result<String, ConnectorError> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| ConnectorError::InvalidArgs(format!("`{key}` is required")))
}

/// A whole number from the arguments, however the model spelled it.
///
/// `as_u64` alone answers `None` for `40000.0` and for `"40000"`, and a `None` here is silently
/// the default — so a model that asked for a smaller page got the full one and no indication
/// that its argument had been ignored. Both spellings are common enough from real models to be
/// worth reading.
fn number(args: &serde_json::Value, key: &str) -> Option<u64> {
    let value = args.get(key)?;
    value
        .as_u64()
        .or_else(|| value.as_f64().filter(|n| *n >= 0.0).map(|n| n as u64))
        .or_else(|| value.as_str()?.trim().parse().ok())
}

/// The tools of §5. `read` tier, reaching the internet, `parallel_safe` — fetching three pages
/// at once is the normal way to use this — and not hidden in Plan mode, which is where reading
/// around a problem belongs.
///
/// Both are `read`: searching and reading are the same kind of act on somebody else's public
/// page, and neither writes anything anywhere.
#[must_use]
pub fn definitions() -> Vec<ToolDef> {
    let mut defs = vec![ToolDef::new(
        "fetch_url",
        "Fetch a web page and read it as text. HTML comes back as the article — navigation, \
         ads and comments removed — with the page title and the final URL after any redirects. \
         Use this to read a page the user linked, to check documentation, or to follow a search \
         result. A page longer than the budget is not cut off: the result carries `more` and \
         `next_offset`, and calling again with that `offset` continues from there at no extra \
         cost. http and https only, and only addresses on the public internet.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string",
                         "description": "The full URL, including https://." },
                "format": { "type": "string", "enum": ["markdown", "text", "html"],
                            "default": "markdown",
                            "description": "markdown keeps headings, links and lists; text is prose only; html is the extracted markup." },
                "offset": { "type": "integer", "minimum": 0, "default": 0,
                            "description": "First character to return, counting from 0. Pass the previous result's next_offset to read on." },
                "max_chars": { "type": "integer", "minimum": 1, "default": DEFAULT_MAX_CHARS,
                               "maximum": MAX_MAX_CHARS,
                               "description": "Characters of content to return in this window. The result says how long the whole page is and where this window stopped." }
            },
            "required": ["url"],
            "additionalProperties": false
        }),
        RiskTier::Read,
    )];

    defs.push(ToolDef::new(
        "search",
        "Search the web for pages to read. Covers Wikipedia for facts and people, Stack \
         Overflow for programming questions and errors, and crates.io and npm for packages — \
         the query decides which. Every result says which index it came from. Follow a \
         promising one with fetch_url to read the page itself; snippets are short and often cut \
         mid-sentence. There is no general web search here, so a question outside those indexes \
         comes back saying so rather than guessing.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": { "type": "string",
                           "description": "What to search for. A question or an error message works; so does a package name." },
                "source": { "type": "string",
                            "enum": ["auto", "wikipedia", "stackoverflow", "crates.io", "npm"],
                            "default": "auto",
                            "description": "Which index to ask. Leave out unless the query alone would route it wrongly." },
                "max_results": { "type": "integer", "minimum": 1, "maximum": MAX_RESULTS,
                                 "default": DEFAULT_RESULTS }
            },
            "required": ["query"],
            "additionalProperties": false
        }),
        RiskTier::Read,
    ));

    for def in &mut defs {
        // Reading pages is the one thing a model should be doing several of at once.
        def.parallel_safe = true;
    }
    defs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid_json_with_the_right_id() {
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(manifest["manifest_version"], "1");
        assert_eq!(manifest["id"], ID);
        assert_eq!(manifest["runtime"]["kind"], "native");
        assert_eq!(manifest["runtime"]["crate"], env!("CARGO_PKG_NAME"));
    }

    #[test]
    fn the_tools_come_from_the_code_and_not_from_the_manifest() {
        // `tools_generated`, like the other three first-party connectors: the code is the source
        // of truth, which is what will let a keyless `search` appear without a manifest change.
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(manifest["tools_generated"], true);
        assert_eq!(names(), ["fetch_url", "search"]);
    }

    #[test]
    fn the_connector_asks_the_user_for_nothing() {
        // The rule this connector is built to (`docs/connectors/web.md` §1): free, always. No
        // key field, no account, no quota to buy. An earlier build asked for a Brave, Tavily or
        // Exa key, and a `user_config` field reappearing here is how that comes back by
        // accident — so the absence is asserted rather than merely true.
        let manifest = gantry_connectors::manifest::Manifest::parse(MANIFEST).unwrap();
        assert!(
            manifest.user_config_fields().is_empty(),
            "the web connector may not ask for a key"
        );
        assert_eq!(
            manifest.auth.kind(),
            gantry_core::AuthType::None,
            "and it may not ask for one through `auth` either"
        );
        // Nothing in what the model is told may suggest buying one either.
        let text = MANIFEST.to_lowercase();
        for word in ["api key", "brave", "tavily", "exa.ai", "byok"] {
            assert!(!text.contains(word), "the manifest still mentions {word}");
        }
    }

    #[test]
    fn every_tool_reads_and_runs_in_parallel() {
        for def in definitions() {
            assert_eq!(def.tier, RiskTier::Read, "{}", def.name);
            assert!(def.parallel_safe, "{}", def.name);
            assert!(!def.always_confirm, "{}", def.name);
            assert_eq!(
                def.plan_mode,
                gantry_core::PlanModePolicy::Allow,
                "reading around a problem is what Plan mode is for ({})",
                def.name
            );
        }
    }

    #[test]
    fn a_multi_byte_character_at_the_sniff_boundary_is_not_a_panic() {
        // 1023 ASCII bytes then an em dash, so byte 1024 falls inside it. Slicing by bytes here
        // panicked, and the input is whatever a server sent.
        let text = format!("{}{}", "a".repeat(1023), "—tail");
        assert!(
            !text.is_char_boundary(1024),
            "the fixture must straddle the boundary"
        );
        assert!(!looks_like_html(&text));
        // The same, for a page that really is HTML past a long run of leading whitespace.
        let html = format!("{}<html>{}", " ".repeat(2000), "—".repeat(500));
        assert!(looks_like_html(&html));
    }

    #[test]
    fn html_is_recognised_even_when_it_is_served_as_plain_text() {
        assert!(looks_like_html("<!DOCTYPE html><html>"));
        assert!(looks_like_html("\n  <html lang=\"en\">"));
        assert!(looks_like_html("<div><body>x</body></div>"));
        assert!(!looks_like_html(
            "# A markdown file\n\nwith <angle> brackets"
        ));
    }

    /// A document that has already been read, so the result shape can be tested for what it
    /// is: a pure function of a page, with no network anywhere near it.
    fn page(body: &str) -> Page {
        Page::new(
            "https://example.com/article".to_owned(),
            200,
            "text/html".to_owned(),
            Some("An article".to_owned()),
            Vec::new(),
            false,
            body.to_owned(),
        )
    }

    fn result(outcome: &ToolOutcome) -> serde_json::Value {
        let ToolOutcome::Complete { structured, .. } = outcome;
        structured.clone().expect("a structured result")
    }

    #[test]
    fn a_page_that_fits_is_one_window_with_nothing_after_it() {
        let page = page("# Title\n\nA short article.");
        let out = result(&render(&page, Format::Markdown, 0, DEFAULT_MAX_CHARS, None));

        assert_eq!(out["content"], "# Title\n\nA short article.");
        assert_eq!(out["first_char"], 0);
        assert_eq!(out["total_chars"], 25);
        assert_eq!(out["more"], false);
        assert_eq!(out["title"], "An article");
        assert_eq!(out["url"], "https://example.com/article");
        // No next page, so no offset to point at one.
        assert!(out.get("next_offset").is_none());
        // Nothing was served from memory, so nothing claims to have been.
        assert!(out.get("cached").is_none());
    }

    #[test]
    fn a_long_page_hands_back_the_offset_to_carry_on_from() {
        let body: String = (0..200).map(|n| format!("Line {n}.\n")).collect();
        let page = page(&body);
        let first = result(&render(&page, Format::Markdown, 0, 100, None));

        assert_eq!(first["more"], true);
        assert_eq!(first["first_char"], 0);
        let next = first["next_offset"]
            .as_u64()
            .expect("somewhere to carry on");
        assert!(next > 0 && next <= 100, "{next}");

        // The model passes it straight back, and the second window starts exactly there.
        let second = result(&render(&page, Format::Markdown, next as usize, 100, None));
        assert_eq!(second["first_char"], next);
        assert_eq!(second["total_chars"], first["total_chars"]);
        // The two windows together are the start of the document, in order and with no
        // content lost between them.
        let joined = format!(
            "{}\n{}",
            first["content"].as_str().unwrap(),
            second["content"].as_str().unwrap()
        );
        assert!(body.starts_with(joined.trim_end()), "{joined}");
    }

    #[test]
    fn a_window_past_the_end_says_how_long_the_page_actually_is() {
        let page = page("short");
        let out = result(&render(
            &page,
            Format::Markdown,
            4_000,
            DEFAULT_MAX_CHARS,
            None,
        ));

        assert_eq!(out["content"], "");
        assert_eq!(out["total_chars"], 5);
        assert_eq!(out["more"], false);
        assert!(
            out["note"].as_str().unwrap().contains("past the end"),
            "a model that guessed too far should be told, not left with an empty string"
        );
    }

    #[test]
    fn a_page_answered_from_memory_says_that_it_is_not_fresh() {
        let page = page("cached prose");
        let out = result(&render(
            &page,
            Format::Markdown,
            0,
            DEFAULT_MAX_CHARS,
            Some(Duration::from_secs(42)),
        ));

        assert_eq!(out["cached"], true);
        assert_eq!(out["cached_seconds_ago"], 42);
        assert_eq!(out["content"], "cached prose");
    }

    #[test]
    fn an_http_error_is_an_error_with_the_page_still_attached() {
        let mut page = page("The page you asked for moved to /docs/new.");
        page.status = 404;
        let outcome = render(&page, Format::Markdown, 0, DEFAULT_MAX_CHARS, None);

        let ToolOutcome::Complete { is_error, .. } = &outcome;
        assert!(is_error, "a 404 is not a success");
        let out = result(&outcome);
        assert_eq!(out["status"], 404);
        assert!(out["content"].as_str().unwrap().contains("/docs/new"));
        // And it pages like any other document.
        assert_eq!(out["more"], false);
    }

    fn names() -> Vec<String> {
        definitions().into_iter().map(|d| d.name).collect()
    }
}
