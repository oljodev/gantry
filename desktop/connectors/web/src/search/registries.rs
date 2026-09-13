//! Package registries: crates.io and npm, each searched through its own public API.
//!
//! A registry is the right answer to "what is the X crate" in a way no general engine is: it
//! knows the current version, the description the author wrote, and where the documentation
//! lives. Both are keyless. crates.io asks for a descriptive user agent and refuses generic
//! ones, which this connector sends anyway (`fetch::USER_AGENT`).
//!
//! **Both registries match every word of a query against name *and* description, which is why
//! searching is not enough.** Measured: `tokio async runtime` does not return `tokio` anywhere
//! in thirty results, because tokio's own description says "event-driven, non-blocking I/O
//! platform" and never the word "runtime". Re-ranking what came back cannot fix that — the
//! package everybody meant is not in the list at all. So every search is paired with an exact
//! name lookup of the words in the query, which is a different endpoint and answers 404 for a
//! word that is not a package. A name that exists goes to the top; the search fills in behind
//! it.

use futures_util::future::join_all;
use url::Url;

use super::{Hit, Source, get_json, snippet};

/// Registry descriptions are a line or two of prose, sometimes with the author's line breaks.
const SNIPPET_CHARS: usize = 240;

/// Words looked up by name for one query. Three covers "rust http client" without turning one
/// search into a dozen requests; they run concurrently and a miss is a 404, so the cost of an
/// extra one is a few milliseconds and the cost of a missing one is the answer.
const MAX_LOOKUPS: usize = 3;

pub async fn crates_io(
    http: &reqwest::Client,
    query: &str,
    limit: usize,
) -> Result<Vec<Hit>, String> {
    let url = Url::parse_with_params(
        "https://crates.io/api/v1/crates",
        &[("q", query), ("per_page", &limit.to_string())],
    )
    .map_err(|err| err.to_string())?;

    let (found, exact) = futures_util::future::join(get_json(http, url), async {
        join_all(lookups(query).into_iter().map(|name| async move {
            let url = format!("https://crates.io/api/v1/crates/{name}");
            let url = Url::parse(&url).ok()?;
            weighed_crate(&get_json(http, url).await.ok()?)
        }))
        .await
    })
    .await;

    merge(
        dominant(exact.into_iter().flatten().collect()),
        found.map(|body| parse_crates(&body)),
        limit,
    )
}

pub async fn npm(http: &reqwest::Client, query: &str, limit: usize) -> Result<Vec<Hit>, String> {
    let url = Url::parse_with_params(
        "https://registry.npmjs.org/-/v1/search",
        &[("text", query), ("size", &limit.to_string())],
    )
    .map_err(|err| err.to_string())?;

    let (found, exact) = futures_util::future::join(get_json(http, url), async {
        join_all(lookups(query).into_iter().map(|name| async move {
            // `/latest` rather than the package root: the root is the whole publish history,
            // megabytes of it, to read four fields off the end.
            let url = format!("https://registry.npmjs.org/{name}/latest");
            let url = Url::parse(&url).ok()?;
            parse_package(&get_json(http, url).await.ok()?)
        }))
        .await
    })
    .await;

    merge(exact, found.map(|body| parse_npm(&body)), limit)
}

/// Exact name matches worth putting above the search, most-used first.
///
/// Every word of a query is somebody's crate. "tokio async runtime" matches `tokio`, which has
/// 962 million downloads, and `runtime`, which has 100 thousand and has never left 0.0.0;
/// "serde json serialization" matches `serde` and also `serialization`, at 17 thousand. Taking
/// every namesake spends result slots on packages nobody meant.
///
/// The rule is relative rather than a threshold, because "popular" is not a number that holds
/// across ecosystems: a match keeps its place only if it is within a thousandth of the most-used
/// match. When one package obviously dominates, its incidental namesakes go; when the matches are
/// comparable — `serde` and `json` are both real answers to "serde json" — they all stay. A
/// single match is always kept, since it has nothing to lose to.
fn dominant(mut exact: Vec<(u64, Hit)>) -> Vec<Option<Hit>> {
    exact.sort_by(|a, b| b.0.cmp(&a.0));
    let Some((best, _)) = exact.first() else {
        return Vec::new();
    };
    let floor = best / 1000;
    exact
        .into_iter()
        .filter(|(downloads, _)| *downloads >= floor)
        .map(|(_, hit)| Some(hit))
        .collect()
}

/// Exact matches first, then whatever the search found, without repeating one.
///
/// A failed *search* is survivable when a name lookup succeeded — "the package you named is
/// this" is a complete answer on its own. It is only an error when there is nothing at all.
fn merge(
    exact: Vec<Option<Hit>>,
    found: Result<Vec<Hit>, String>,
    limit: usize,
) -> Result<Vec<Hit>, String> {
    let mut hits: Vec<Hit> = exact.into_iter().flatten().collect();
    match found {
        Ok(rest) => {
            for hit in rest {
                if !hits.iter().any(|kept| kept.url == hit.url) {
                    hits.push(hit);
                }
            }
        }
        Err(why) if hits.is_empty() => return Err(why),
        Err(_) => {}
    }
    hits.truncate(limit);
    Ok(hits)
}

/// The words of a query worth trying as package names.
///
/// Anything that could be one: lowercase, at least three characters, no punctuation a registry
/// would not accept. Deliberately not filtered against a list of "generic" words — `http`,
/// `regex` and `serde` are all real packages and all read as generic, and a 404 costs nothing,
/// so guessing which word is a name is a judgement this does not have to make.
#[must_use]
pub fn lookups(query: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for token in query.split(|c: char| c.is_whitespace() || c == ',' || c == '/') {
        let name = token
            .trim_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_ascii_lowercase();
        let usable = name.len() >= 3
            && name.starts_with(|c: char| c.is_ascii_alphanumeric())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !name.chars().all(|c| c.is_ascii_digit());
        if usable && !names.contains(&name) {
            names.push(name);
            if names.len() == MAX_LOOKUPS {
                break;
            }
        }
    }
    names
}

/// `crates[]` into hits. The URL is the crate's page rather than its docs, because a model that
/// wants the documentation can follow `docs.rs/<name>` and one that wants the repository needs
/// the page anyway.
#[must_use]
pub fn parse_crates(body: &serde_json::Value) -> Vec<Hit> {
    let Some(items) = body.get("crates").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    items.iter().filter_map(crate_hit).collect()
}

/// One crate from the by-name endpoint, whose envelope is `{ "crate": { … } }`.
#[must_use]
pub fn parse_crate(body: &serde_json::Value) -> Option<Hit> {
    crate_hit(body.get("crate")?)
}

/// The same, with the download count that orders several exact matches against each other.
fn weighed_crate(body: &serde_json::Value) -> Option<(u64, Hit)> {
    let item = body.get("crate")?;
    let downloads = item
        .get("downloads")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    Some((downloads, crate_hit(item)?))
}

fn crate_hit(item: &serde_json::Value) -> Option<Hit> {
    let name = item.get("name")?.as_str()?;
    // `max_stable_version` is the one a `cargo add` would take; `max_version` can be a
    // pre-release, which is not what "the current version" means to anybody asking.
    let version = item
        .get("max_stable_version")
        .or_else(|| item.get("max_version"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?");
    let description = item
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    Some(Hit {
        title: format!("{name} {version}"),
        url: format!("https://crates.io/crates/{name}"),
        snippet: snippet(description, SNIPPET_CHARS),
        source: Source::CratesIo,
    })
}

/// `objects[].package` into hits.
#[must_use]
pub fn parse_npm(body: &serde_json::Value) -> Vec<Hit> {
    let Some(items) = body.get("objects").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| package_hit(item.get("package")?))
        .collect()
}

/// One package from `registry.npmjs.org/<name>/latest`, which is the manifest itself with no
/// envelope around it.
#[must_use]
pub fn parse_package(body: &serde_json::Value) -> Option<Hit> {
    package_hit(body)
}

fn package_hit(package: &serde_json::Value) -> Option<Hit> {
    let name = package.get("name")?.as_str()?;
    let version = package
        .get("version")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("?");
    let description = package
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let url = package
        .get("links")
        .and_then(|links| links.get("npm"))
        .and_then(serde_json::Value::as_str)
        .map_or_else(
            || format!("https://www.npmjs.com/package/{name}"),
            str::to_owned,
        );
    Some(Hit {
        title: format!("{name} {version}"),
        url,
        snippet: snippet(description, SNIPPET_CHARS),
        source: Source::Npm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_worth_trying_as_a_package_name() {
        assert_eq!(
            lookups("tokio async runtime"),
            ["tokio", "async", "runtime"]
        );
        // Punctuation trimmed, duplicates dropped, and the cap holds.
        assert_eq!(
            lookups("serde, serde and serde_json!"),
            ["serde", "and", "serde_json"]
        );
        assert_eq!(
            lookups("a of to in").len(),
            0,
            "nothing long enough to be a name"
        );
        // A version number is not a name, and neither is a two-character token; the word
        // beside them is.
        assert_eq!(lookups("react 19 v2"), ["react"]);
        assert_eq!(lookups("one two three four five").len(), MAX_LOOKUPS);
    }

    #[test]
    fn an_incidental_namesake_does_not_take_a_slot_from_the_package_that_was_meant() {
        let weighed = |name: &str, downloads: u64| {
            (
                downloads,
                Hit {
                    title: name.to_owned(),
                    url: format!("https://crates.io/crates/{name}"),
                    snippet: String::new(),
                    source: Source::CratesIo,
                },
            )
        };
        let names = |hits: Vec<Option<Hit>>| {
            hits.into_iter()
                .flatten()
                .map(|hit| hit.title)
                .collect::<Vec<_>>()
        };

        // Real counts, from the live API. `runtime` exists, has never left 0.0.0, and is not
        // what "tokio async runtime" was about.
        assert_eq!(
            names(dominant(vec![
                weighed("runtime", 100_661),
                weighed("tokio", 961_955_978),
            ])),
            ["tokio"]
        );
        // `serde` and `json` are both real answers to "serde json", and both stay.
        assert_eq!(
            names(dominant(vec![
                weighed("serde", 1_387_556_739),
                weighed("json", 25_183_080),
                weighed("serialization", 17_533),
            ])),
            ["serde", "json"]
        );
        // One match has nothing to lose to, however small it is — a crate published yesterday
        // is still the crate that was named.
        assert_eq!(
            names(dominant(vec![weighed("brand-new", 3)])),
            ["brand-new"]
        );
        assert!(dominant(Vec::new()).is_empty());
    }

    #[test]
    fn a_name_that_exists_outranks_whatever_the_search_returned() {
        let hit = |name: &str, source: Source| Hit {
            title: name.to_owned(),
            url: format!("https://example.com/{name}"),
            snippet: String::new(),
            source,
        };
        let merged = merge(
            vec![Some(hit("tokio", Source::CratesIo)), None],
            Ok(vec![
                hit("pyo3-async-runtimes", Source::CratesIo),
                hit("tokio", Source::CratesIo),
            ]),
            10,
        )
        .unwrap();
        // The exact match leads, and is not repeated when the search found it too.
        assert_eq!(
            merged.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(),
            ["tokio", "pyo3-async-runtimes"]
        );
    }

    #[test]
    fn a_named_package_is_still_an_answer_when_the_search_itself_failed() {
        let hit = Hit {
            title: "tokio".to_owned(),
            url: "https://crates.io/crates/tokio".to_owned(),
            snippet: String::new(),
            source: Source::CratesIo,
        };
        let merged = merge(vec![Some(hit)], Err("answered 503".to_owned()), 10).unwrap();
        assert_eq!(merged.len(), 1);
        // With nothing from either, the search's reason is what the model is told.
        assert_eq!(
            merge(vec![None], Err("answered 503".to_owned()), 10),
            Err("answered 503".to_owned())
        );
    }
}
