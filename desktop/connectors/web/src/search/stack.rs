//! Stack Overflow through the Stack Exchange API: the index for a programming question.
//!
//! Keyless. Unauthenticated calls are quota'd at 300 a day per address, which is generous enough
//! that no ration is enforced here — a chat that burns 300 searches in a day has a different
//! problem — but the quota the API reports is worth watching if that ever stops being true.
//!
//! The API gzips by default and this connector's client does not advertise gzip, so responses
//! arrive as plain JSON. That is why no compression dependency appears in `Cargo.toml`.

use url::Url;

use super::{Hit, Source, get_json, snippet, strip_tags};

/// Question bodies are whole posts; a result list wants the first paragraph of one.
const SNIPPET_CHARS: usize = 300;

pub async fn search(http: &reqwest::Client, query: &str, limit: usize) -> Result<Vec<Hit>, String> {
    let url = Url::parse_with_params(
        "https://api.stackexchange.com/2.3/search/advanced",
        &[
            ("order", "desc"),
            ("sort", "relevance"),
            ("q", query),
            ("site", "stackoverflow"),
            ("pagesize", &limit.to_string()),
            // The default filter returns no body at all, which would make every snippet empty.
            ("filter", "withbody"),
        ],
    )
    .map_err(|err| err.to_string())?;
    Ok(parse(&get_json(http, url).await?))
}

/// `items[]` into hits.
///
/// An answered question is worth more than a popular one, so the accepted-answer state and the
/// score go into the title rather than being dropped: a model choosing which result to fetch
/// should be able to see that one of them has an accepted answer and another has none.
pub fn parse(body: &serde_json::Value) -> Vec<Hit> {
    let Some(items) = body.get("items").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let title = item.get("title")?.as_str()?;
            let link = item.get("link")?.as_str()?;
            let answers = item
                .get("answer_count")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let answered = item
                .get("is_answered")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let state = match (answered, answers) {
                (true, _) => "accepted answer".to_owned(),
                (false, 0) => "no answers".to_owned(),
                (false, 1) => "1 answer, none accepted".to_owned(),
                (false, n) => format!("{n} answers, none accepted"),
            };
            let body = item
                .get("body")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            Some(Hit {
                // Titles arrive HTML-escaped — `&quot;` and `&#39;` are routine in them.
                title: strip_tags(title),
                url: link.to_owned(),
                snippet: format!("[{state}] {}", snippet(&strip_tags(body), SNIPPET_CHARS)),
                source: Source::StackOverflow,
            })
        })
        .collect()
}
