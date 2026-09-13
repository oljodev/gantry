//! Package registries: crates.io and npm, each searched through its own public API.
//!
//! A registry is the right answer to "what is the X crate" in a way no general engine is: it
//! knows the current version, the description the author wrote, and where the documentation
//! lives. Both are keyless. crates.io asks for a descriptive user agent and refuses generic
//! ones, which this connector sends anyway (`fetch::USER_AGENT`).
//!
//! The two are one module because they are the same shape of answer — name, version,
//! description, a link to the page a human would read — and keeping them together is what keeps
//! that shape honest as a third registry is added.

use url::Url;

use super::{Hit, Source, get_json, snippet};

/// Registry descriptions are a line or two of prose, sometimes with the author's line breaks.
const SNIPPET_CHARS: usize = 240;

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
    Ok(parse_crates(&get_json(http, url).await?))
}

pub async fn npm(http: &reqwest::Client, query: &str, limit: usize) -> Result<Vec<Hit>, String> {
    let url = Url::parse_with_params(
        "https://registry.npmjs.org/-/v1/search",
        &[("text", query), ("size", &limit.to_string())],
    )
    .map_err(|err| err.to_string())?;
    Ok(parse_npm(&get_json(http, url).await?))
}

/// `crates[]` into hits. The URL is the crate's page rather than its docs, because a model that
/// wants the documentation can follow `docs.rs/<name>` and one that wants the repository needs
/// the page anyway.
pub fn parse_crates(body: &serde_json::Value) -> Vec<Hit> {
    let Some(items) = body.get("crates").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
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
        })
        .collect()
}

/// `objects[].package` into hits.
pub fn parse_npm(body: &serde_json::Value) -> Vec<Hit> {
    let Some(items) = body.get("objects").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let package = item.get("package")?;
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
        })
        .collect()
}
