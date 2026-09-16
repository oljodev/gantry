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

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

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

/// A complete, ordinary header set — and an honest name on it (D15).
///
/// §6.1's finding was that the block is triggered by request *headers* rather than by TLS
/// fingerprint, so a plain client sending a full header set is served normally, which is why no
/// fingerprint-impersonating HTTP client is a dependency here. That finding was read too far:
/// what it justifies is sending `Accept` and `Accept-Language` at all, not claiming to be
/// Chrome 131 on X11 while sending neither. §7.3 measured the two and they were
/// byte-identical on every blocking site — so the lie bought nothing and cost the posture the
/// whole connector is built on.
///
/// No `Referer` either. The one that used to be here named the page this request is a form post
/// *to*, as though it had been loaded first. It had not.
const HEADERS: &[(&str, &str)] = &[
    (
        "Accept",
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    ),
    ("Accept-Language", "en-US,en;q=0.9"),
];

/// Why a general query could not be spent right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spent {
    /// The last one was too recent.
    TooSoon(Duration),
    /// A block was seen and the circuit is open.
    Blocked(Duration),
}

/// The general search budget, shared by every call this connector makes — and kept across
/// restarts, which is the whole difference between a ration and a suggestion.
///
/// It used to live in memory on `Instant`s, so quitting Gantry forgot both the gap and the
/// twenty-minute cooldown. A user whose search had just been blocked could restart the app and
/// walk straight back into the block, which extends it (§6.4): the one response that is
/// guaranteed to make things worse. Wall-clock milliseconds in a small file beside the app's
/// data, because a monotonic clock is exactly the thing that does not survive a process.
#[derive(Debug, Default)]
pub struct Ration {
    state: Mutex<State>,
    /// Where the state is kept. `None` in tests and for a connector built without a data
    /// directory, which then behaves as it always did.
    path: Option<PathBuf>,
}

/// Unix milliseconds, not `Instant`: this is written down and read back after a restart.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct State {
    last: Option<i64>,
    blocked_until: Option<i64>,
}

impl State {
    /// The file is two integers; a format is not worth a serde derive, and a state file that
    /// cannot be read is not worth an error — the answer is the same either way, which is to
    /// start from nothing.
    fn parse(text: &str) -> Self {
        let field = |name: &str| -> Option<i64> {
            let at = text.find(name)? + name.len();
            let rest = text[at..].trim_start_matches([':', ' ', '"']);
            let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-')?;
            rest[..end].parse().ok()
        };
        Self {
            last: field("\"last\""),
            blocked_until: field("\"blocked_until\""),
        }
    }

    fn write(self) -> String {
        format!(
            "{{\"last\": {}, \"blocked_until\": {}}}",
            self.last.unwrap_or(0),
            self.blocked_until.unwrap_or(0)
        )
    }
}

impl Ration {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The ration kept in `path`, read now and written on every change.
    #[must_use]
    pub fn at(path: PathBuf) -> Self {
        let state = std::fs::read_to_string(&path)
            .map(|text| State::parse(&text))
            .unwrap_or_default();
        Self {
            state: Mutex::new(state),
            path: Some(path),
        }
    }

    /// Claim the right to make one general query, or say why not.
    ///
    /// Claiming and checking are one operation under one lock, which is what keeps several
    /// searches in the same turn — this connector is `parallel_safe`, so that is the normal
    /// case — from all deciding at once that it has been long enough.
    pub fn take(&self) -> Result<(), Spent> {
        self.take_at(gantry_core::now_ms())
    }

    /// Record that the engine refused, opening the circuit for `COOLDOWN`.
    pub fn blocked(&self) {
        self.blocked_at(gantry_core::now_ms());
    }

    fn take_at(&self, now: i64) -> Result<(), Spent> {
        // A poisoned lock is treated as "no budget": the safe answer when the state is unknown
        // is to use the index that cannot be blocked.
        let Ok(mut state) = self.state.lock() else {
            return Err(Spent::TooSoon(GAP));
        };
        if let Some(until) = state.blocked_until {
            match ahead(until, now, COOLDOWN) {
                Some(left) => return Err(Spent::Blocked(left)),
                None => state.blocked_until = None,
            }
        }
        if let Some(last) = state.last
            && let Some(left) = ahead(last + as_ms(GAP), now, GAP)
        {
            return Err(Spent::TooSoon(left));
        }
        state.last = Some(now);
        let written = *state;
        drop(state);
        self.save(written);
        Ok(())
    }

    fn blocked_at(&self, now: i64) {
        if let Ok(mut state) = self.state.lock() {
            state.blocked_until = Some(now + as_ms(COOLDOWN));
            let written = *state;
            drop(state);
            self.save(written);
        }
    }

    /// A failure to write is logged and no more: the ration in memory is still right for this
    /// run, and refusing to search because a state file would not open would be a worse answer
    /// than forgetting it at the next restart.
    fn save(&self, state: State) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent()
            && let Err(err) = std::fs::create_dir_all(dir)
        {
            log::debug!("could not make room for the search ration: {err}");
            return;
        }
        if let Err(err) = std::fs::write(path, state.write()) {
            log::debug!("could not write the search ration: {err}");
        }
    }
}

fn as_ms(d: Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}

/// How long until `deadline`, or `None` when it has passed.
///
/// `limit` is what makes this safe across a clock that moved: a deadline further away than the
/// longest wait that could ever have been set is not a deadline, it is a machine whose clock
/// went backwards, and the answer there is to forget it rather than to wait out a cooldown that
/// might be years long.
fn ahead(deadline: i64, now: i64, limit: Duration) -> Option<Duration> {
    let left = deadline.checked_sub(now)?;
    if left <= 0 || left > as_ms(limit) {
        return None;
    }
    Some(Duration::from_millis(u64::try_from(left).unwrap_or(0)))
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
    for (name, value) in HEADERS {
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

    /// A fixed point on the wall clock, so the tests read as times rather than as arithmetic.
    const START: i64 = 1_760_000_000_000;

    fn after(d: Duration) -> i64 {
        START + as_ms(d)
    }

    #[test]
    fn one_general_query_per_gap_and_no_more() {
        let ration = Ration::new();
        assert_eq!(ration.take_at(START), Ok(()));
        // Straight away, and just under the gap: refused, with how long is left.
        assert!(matches!(
            ration.take_at(after(Duration::from_secs(1))),
            Err(Spent::TooSoon(_))
        ));
        assert!(matches!(
            ration.take_at(after(GAP) - 1),
            Err(Spent::TooSoon(_))
        ));
        assert_eq!(ration.take_at(after(GAP)), Ok(()));
    }

    #[test]
    fn a_block_closes_the_engine_for_the_whole_cooldown() {
        let ration = Ration::new();
        ration.blocked_at(START);
        assert!(matches!(
            ration.take_at(after(Duration::from_secs(1))),
            Err(Spent::Blocked(_))
        ));
        // Still shut well after the gap would have allowed another.
        assert!(matches!(
            ration.take_at(after(GAP * 4)),
            Err(Spent::Blocked(_))
        ));
        assert!(matches!(
            ration.take_at(after(COOLDOWN) - 1_000),
            Err(Spent::Blocked(_))
        ));
        // And open again after it, without anything having to reset it.
        assert_eq!(ration.take_at(after(COOLDOWN)), Ok(()));
    }

    /// The point of writing it down: restarting Gantry inside a block used to clear it, and
    /// walking back into a block extends it (§6.4). A quit is not a reason to be let through.
    #[test]
    fn a_block_outlives_the_process_that_earned_it() {
        let dir = std::env::temp_dir().join(format!("gantry-ration-{}", std::process::id()));
        let path = dir.join("ration.json");
        let _ = std::fs::remove_dir_all(&dir);

        let first = Ration::at(path.clone());
        first.blocked_at(START);
        drop(first);

        let restarted = Ration::at(path.clone());
        assert!(
            matches!(restarted.take_at(after(GAP * 4)), Err(Spent::Blocked(_))),
            "the cooldown was still running"
        );
        assert_eq!(restarted.take_at(after(COOLDOWN)), Ok(()));

        // And the gap it just spent is written down too, so two restarts in a row are not two
        // free queries.
        drop(restarted);
        let again = Ration::at(path);
        assert!(matches!(
            again.take_at(after(COOLDOWN) + 1_000),
            Err(Spent::TooSoon(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A clock that moved is not a cooldown. Without this, one backwards jump would shut the
    /// engine for as long as the jump was.
    #[test]
    fn a_deadline_further_off_than_it_could_possibly_be_is_forgotten() {
        let ration = Ration::new();
        ration.blocked_at(START + as_ms(COOLDOWN) * 100);
        assert_eq!(
            ration.take_at(START),
            Ok(()),
            "a cooldown ending in a fortnight is a clock, not a cooldown"
        );
    }

    #[test]
    fn the_state_file_survives_a_round_trip_and_a_bad_one_reads_as_nothing() {
        let state = State {
            last: Some(START),
            blocked_until: Some(START + 1),
        };
        assert_eq!(State::parse(&state.write()), state);
        assert_eq!(State::parse("not json at all"), State::default());
        assert_eq!(State::parse(""), State::default());
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
