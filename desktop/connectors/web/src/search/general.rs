//! General web search, which is the one part of this connector that cannot be done properly for
//! free (`docs/connectors/web.md` §6.1, §6.4, §6.5).
//!
//! The measured finding the rest of this file follows from: **general web search is severely
//! rationed and nothing makes it otherwise.** DuckDuckGo's Lite endpoint returns good results
//! and blocks after roughly five or six queries in two minutes, for about twenty minutes.
//! Everything else freely available is worse — Mojeek, Startpage, Ecosia and Yep are blocked or
//! behind proof-of-work, Brave's page is the product Brave sells, Marginalia's free key is
//! licensed non-commercially and cannot ship here.
//!
//! So the budget is enforced here rather than discovered by the user. A general query is spent
//! at most once every `GAP`; a query that arrives sooner, or while the circuit breaker is open,
//! goes to mwmbl instead — an open, non-profit, independent crawl with no rate limit, thinner
//! than DuckDuckGo and with no sense of recency, but it belongs to its users rather than
//! blocking them. Which one answered is in every result, because silently serving worse results
//! is what erodes trust in a search feature (§6.5).
//!
//! What is *not* here: a per-turn cap. The gap is the whole ration, and because it is taken
//! under a lock, five parallel searches in one turn spend one general query between them and
//! the rest fall through. A turn-scoped cap on top of that is §6.4's remaining piece.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use dom_query::Document;
use url::Url;

use super::{Hit, Source, get_json, snippet, strip_tags};

/// The shortest time between two general queries.
///
/// §6.1 measured the block at five or six queries in two minutes, which is one per twenty-two
/// seconds; the document's "about twenty seconds" sits exactly on that boundary. This is a few
/// seconds the safe side of it, because the cost of being wrong is a twenty-minute outage and
/// the cost of being cautious is one search going to mwmbl.
pub const GAP: Duration = Duration::from_secs(25);

/// How long the circuit stays open once a block is seen. Retrying into a block extends it, so
/// the only safe response is to stop asking (§6.4).
pub const COOLDOWN: Duration = Duration::from_secs(20 * 60);

/// Snippets from a results page are a sentence or two already.
const SNIPPET_CHARS: usize = 300;

/// What a browser sends. §6.1's other finding: the block is triggered by request headers and
/// not by TLS fingerprint, so a plain client sending a complete, ordinary header set is served
/// normally — which is why no fingerprint-impersonating HTTP client is a dependency here.
const BROWSER: &[(&str, &str)] = &[
    (
        "User-Agent",
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
         Chrome/131.0.0.0 Safari/537.36",
    ),
    (
        "Accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8",
    ),
    ("Accept-Language", "en-US,en;q=0.9"),
    ("Referer", "https://lite.duckduckgo.com/"),
];

/// Why a general query could not be spent right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spent {
    /// The last one was too recent.
    TooSoon(Duration),
    /// A block was seen and the circuit is open.
    Blocked(Duration),
}

/// The general search budget, shared by every call this connector makes.
#[derive(Debug, Default)]
pub struct Ration {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    last: Option<Instant>,
    blocked_until: Option<Instant>,
}

impl Ration {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Claim the right to make one general query, or say why not.
    ///
    /// Claiming and checking are one operation under one lock, which is what keeps several
    /// searches in the same turn — this connector is `parallel_safe`, so that is the normal
    /// case — from all deciding at once that it has been long enough.
    pub fn take(&self) -> Result<(), Spent> {
        self.take_at(Instant::now())
    }

    /// Record that the engine refused, opening the circuit for `COOLDOWN`.
    pub fn blocked(&self) {
        self.blocked_at(Instant::now());
    }

    fn take_at(&self, now: Instant) -> Result<(), Spent> {
        // A poisoned lock is treated as "no budget": the safe answer when the state is unknown
        // is to use the index that cannot be blocked.
        let Ok(mut state) = self.state.lock() else {
            return Err(Spent::TooSoon(GAP));
        };
        if let Some(until) = state.blocked_until {
            if now < until {
                return Err(Spent::Blocked(until - now));
            }
            state.blocked_until = None;
        }
        if let Some(last) = state.last {
            let since = now.saturating_duration_since(last);
            if since < GAP {
                return Err(Spent::TooSoon(GAP - since));
            }
        }
        state.last = Some(now);
        Ok(())
    }

    fn blocked_at(&self, now: Instant) {
        if let Ok(mut state) = self.state.lock() {
            state.blocked_until = Some(now + COOLDOWN);
        }
    }
}

/// A general query, through whichever engine is available.
///
/// Never fails over to nothing: when DuckDuckGo cannot be asked, or answers with a block, mwmbl
/// answers instead. The caller learns which from the `source` on each hit.
pub async fn search(
    http: &reqwest::Client,
    ration: &Ration,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>, String> {
    match ration.take() {
        Ok(()) => match duckduckgo(http, query, limit).await {
            Ok(hits) if !hits.is_empty() => Ok(hits),
            Ok(_) => mwmbl(http, query, limit).await,
            Err(Refused::Blocked) => {
                ration.blocked();
                mwmbl(http, query, limit).await
            }
            Err(Refused::Failed(why)) => match mwmbl(http, query, limit).await {
                Ok(hits) if !hits.is_empty() => Ok(hits),
                _ => Err(why),
            },
        },
        Err(_) => mwmbl(http, query, limit).await,
    }
}

enum Refused {
    /// The engine served a challenge or a refusal rather than results.
    Blocked,
    Failed(String),
}

async fn duckduckgo(
    http: &reqwest::Client,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>, Refused> {
    let mut request = http
        .post("https://lite.duckduckgo.com/lite/")
        .form(&[("q", query)]);
    for (name, value) in BROWSER {
        request = request.header(*name, *value);
    }
    let response = request
        .send()
        .await
        .map_err(|err| Refused::Failed(err.to_string()))?;
    let status = response.status();
    // 403 and 429 are the block saying so outright.
    if status.as_u16() == 403 || status.as_u16() == 429 {
        return Err(Refused::Blocked);
    }
    if !status.is_success() {
        return Err(Refused::Failed(format!("answered {}", status.as_u16())));
    }
    let body = response
        .text()
        .await
        .map_err(|err| Refused::Failed(err.to_string()))?;

    let hits = parse_duckduckgo(&body, limit);
    if hits.is_empty() && looks_blocked(&body) {
        return Err(Refused::Blocked);
    }
    Ok(hits)
}

/// A results page that is not a results page.
///
/// Only consulted when nothing parsed, so a genuine no-results page for an obscure query is not
/// mistaken for a block — that distinction is the difference between "try different words" and
/// "stop asking for twenty minutes".
fn looks_blocked(body: &str) -> bool {
    let text = body.to_lowercase();
    [
        "anomaly",
        "captcha",
        "unusual traffic",
        "are you a robot",
        "blocked",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

/// The Lite page's result rows.
///
/// Parsed as HTML rather than with a regular expression, because the markup uses single-quoted
/// attributes in some places and double in others, and a pattern that assumes either is one
/// template change from returning nothing at all.
#[must_use]
pub fn parse_duckduckgo(body: &str, limit: usize) -> Vec<Hit> {
    let document = Document::from(body);
    let links: Vec<(String, String)> = document
        .select("a.result-link")
        .iter()
        .filter_map(|node| {
            let href = node.attr("href")?.to_string();
            let title = node.text().trim().to_string();
            (!title.is_empty() && href.starts_with("http")).then_some((title, href))
        })
        .collect();
    // Snippets are in their own rows, in the same order, and a result without one is possible.
    let snippets: Vec<String> = document
        .select(".result-snippet")
        .iter()
        .map(|node| snippet(&strip_tags(&node.text()), SNIPPET_CHARS))
        .collect();

    links
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(at, (title, url))| Hit {
            title,
            url,
            snippet: snippets.get(at).cloned().unwrap_or_default(),
            source: Source::DuckDuckGo,
        })
        .collect()
}

/// mwmbl on its own, for a caller that asked for it by name rather than falling to it.
pub async fn mwmbl_only(
    http: &reqwest::Client,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>, String> {
    mwmbl(http, query, limit).await
}

async fn mwmbl(http: &reqwest::Client, query: &str, limit: usize) -> Result<Vec<Hit>, String> {
    let url = Url::parse_with_params("https://api.mwmbl.org/api/v1/search/", &[("s", query)])
        .map_err(|err| err.to_string())?;
    Ok(parse_mwmbl(&get_json(http, url).await?, limit))
}

/// mwmbl's results, whose titles and extracts arrive as runs of text with the matched words
/// flagged rather than as strings.
#[must_use]
pub fn parse_mwmbl(body: &serde_json::Value, limit: usize) -> Vec<Hit> {
    let Some(items) = body.as_array() else {
        return Vec::new();
    };
    let joined = |value: Option<&serde_json::Value>| -> String {
        value
            .and_then(serde_json::Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part.get("value").and_then(serde_json::Value::as_str))
                    .collect::<String>()
            })
            .unwrap_or_default()
    };
    items
        .iter()
        .filter_map(|item| {
            let url = item.get("url")?.as_str()?;
            let title = joined(item.get("title"));
            (!title.trim().is_empty()).then(|| Hit {
                title: title.trim().to_owned(),
                url: url.to_owned(),
                snippet: snippet(joined(item.get("extract")).trim(), SNIPPET_CHARS),
                source: Source::Mwmbl,
            })
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_general_query_per_gap_and_no_more() {
        let ration = Ration::new();
        let start = Instant::now();
        assert_eq!(ration.take_at(start), Ok(()));
        // Straight away, and just under the gap: refused, with how long is left.
        assert!(matches!(
            ration.take_at(start + Duration::from_secs(1)),
            Err(Spent::TooSoon(_))
        ));
        assert!(matches!(
            ration.take_at(start + GAP - Duration::from_millis(1)),
            Err(Spent::TooSoon(_))
        ));
        assert_eq!(ration.take_at(start + GAP), Ok(()));
    }

    #[test]
    fn a_block_closes_the_engine_for_the_whole_cooldown() {
        let ration = Ration::new();
        let start = Instant::now();
        ration.blocked_at(start);
        assert!(matches!(
            ration.take_at(start + Duration::from_secs(1)),
            Err(Spent::Blocked(_))
        ));
        // Still shut well after the gap would have allowed another.
        assert!(matches!(
            ration.take_at(start + GAP * 4),
            Err(Spent::Blocked(_))
        ));
        assert!(matches!(
            ration.take_at(start + COOLDOWN - Duration::from_secs(1)),
            Err(Spent::Blocked(_))
        ));
        // And open again after it, without anything having to reset it.
        assert_eq!(ration.take_at(start + COOLDOWN), Ok(()));
    }

    #[test]
    fn a_no_results_page_is_not_a_block() {
        // The distinction that decides between "try different words" and "stop asking for
        // twenty minutes", so it is not left to whether the page happened to be empty.
        assert!(!looks_blocked(
            "<html><body>No results found.</body></html>"
        ));
        assert!(looks_blocked(
            "<html><body>Our systems detected unusual traffic</body></html>"
        ));
        assert!(looks_blocked("<p>anomaly detected</p>"));
    }
}
