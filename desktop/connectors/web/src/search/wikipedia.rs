//! Wikipedia's action API: the nearest thing to a general index that needs no account.
//!
//! Free, no key, no practical ration, and it answers "what is this", "who was that" and "when
//! did that happen" better than a general engine would. It is also the backstop in `route`: a
//! query with no programming signal in it goes here rather than nowhere.

use url::Url;

use super::{Hit, Source, get_json, snippet, strip_tags};

/// Snippets come back as highlighted HTML and are a sentence or two; this is a cap, not a
/// target.
const SNIPPET_CHARS: usize = 300;

pub async fn search(http: &reqwest::Client, query: &str, limit: usize) -> Result<Vec<Hit>, String> {
    let url = Url::parse_with_params(
        "https://en.wikipedia.org/w/api.php",
        &[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("srlimit", &limit.to_string()),
            // The default `srwhat=text` searches article text rather than titles only, which is
            // what makes a question phrased as a sentence find anything at all.
            ("format", "json"),
            ("utf8", "1"),
            // Without this the API answers in a legacy shape on some wikis.
            ("formatversion", "1"),
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(parse(&get_json(http, url).await?))
}

/// `query.search[]` into hits, with the article URL built from the title.
///
/// The API returns no URL of its own — it returns a `pageid` and a `title`, and every caller is
/// expected to build the link. `?curid=` is used rather than `/wiki/<title>` because a title
/// with a slash, a question mark or an apostrophe in it makes the pretty form ambiguous, and the
/// numeric form is stable across renames.
pub fn parse(body: &serde_json::Value) -> Vec<Hit> {
    let Some(results) = body
        .get("query")
        .and_then(|q| q.get("search"))
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|item| {
            let title = item.get("title")?.as_str()?;
            let id = item.get("pageid")?.as_u64()?;
            let text = item
                .get("snippet")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            Some(Hit {
                title: title.to_owned(),
                url: format!("https://en.wikipedia.org/?curid={id}"),
                snippet: snippet(&strip_tags(text), SNIPPET_CHARS),
                source: Source::Wikipedia,
            })
        })
        .collect()
}
