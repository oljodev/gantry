//! `search`, bring your own key (03 §5, §11 step 2).
//!
//! Gantry has no search key of its own and buys nobody's quota. The user pastes a key from Brave,
//! Tavily or Exa into the connector's `user_config` form; the value is `sensitive`, so it goes to
//! the vault as a `user_config_secret` credential and the config row keeps only the name of the
//! field it fills (06 §3). Nothing here ever sees the database.
//!
//! When no key is configured the tool is not offered at all — `definitions(false)` leaves it out
//! and `Web::tools` returns that. A tool the model can see and cannot use costs a round to find
//! out, and the error it gets back ("configure a key in Settings") is not something a model can
//! act on in the middle of a turn.
//!
//! Provider-native search (Anthropic, OpenAI, Gemini, xAI, OpenRouter) is the provider layer's
//! and is preferred where the chat's model has it; this is the fallback.

use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;

/// Which service the key belongs to. Brave and Exa authenticate with a header, Tavily with a
/// bearer token, and all three answer a different shape, so the key alone is not enough to know
/// what to do with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Brave,
    Tavily,
    Exa,
}

impl Provider {
    /// The `SEARCH_PROVIDER` field of `user_config`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "brave" => Some(Self::Brave),
            "tavily" => Some(Self::Tavily),
            "exa" => Some(Self::Exa),
            _ => None,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Brave => "Brave Search",
            Self::Tavily => "Tavily",
            Self::Exa => "Exa",
        }
    }
}

/// A configured key. `SecretString` rather than `String` so that a stray `{:?}` in a log line
/// prints `SecretBox<str>([REDACTED])` instead of the user's key.
pub struct Search {
    pub provider: Provider,
    key: SecretString,
}

impl std::fmt::Debug for Search {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Search")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

/// One result, the same three fields whichever service answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug)]
pub enum SearchError {
    /// The service answered, and said no.
    Rejected {
        status: u16,
        message: String,
    },
    Transport(String),
    /// A 200 whose body was not the shape the provider documents.
    Unreadable(String),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected { status, message } if *status == 401 || *status == 403 => write!(
                f,
                "the search key was rejected ({status}). Check it in Customize → Connectors → \
                 Web. {message}"
            ),
            Self::Rejected { status, message } if *status == 429 => write!(
                f,
                "the search service is rate-limiting this key ({status}). {message}"
            ),
            Self::Rejected { status, message } => {
                write!(f, "the search service answered {status}: {message}")
            }
            Self::Transport(message) => {
                write!(f, "the search service could not be reached: {message}")
            }
            Self::Unreadable(message) => write!(
                f,
                "the search service answered something this connector could not read: {message}"
            ),
        }
    }
}

/// Results asked for when the model does not say.
pub const DEFAULT_RESULTS: usize = 8;
/// More than this is a research project, not a search, and every provider charges by the call.
pub const MAX_RESULTS: usize = 20;

impl Search {
    #[must_use]
    pub fn new(provider: Provider, key: SecretString) -> Self {
        Self { provider, key }
    }

    /// Build from the two `user_config` answers, or `None` when either is missing — which is how
    /// "no key configured" reaches `definitions` as a plain `bool`.
    #[must_use]
    pub fn from_config(provider: Option<&str>, key: Option<&SecretString>) -> Option<Self> {
        // Trimmed, not merely tested for emptiness after trimming: a key pasted with a trailing
        // newline is the ordinary way to paste one, and sending it as typed is a 401 the user
        // has no way to explain.
        let key = key
            .map(|k| SecretString::from(k.expose_secret().trim().to_owned()))
            .filter(|k| !k.expose_secret().is_empty())?;

        // A key with no provider named is very likely Brave, which is the field's default; but
        // guessing which service to send a secret to is not a guess worth making.
        let provider = Provider::parse(provider?)?;
        Some(Self::new(provider, key))
    }

    pub async fn run(
        &self,
        http: &reqwest::Client,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<Hit>, SearchError> {
        let count = max_results.clamp(1, MAX_RESULTS);
        let request = match self.provider {
            Provider::Brave => {
                // Built with `url` rather than reqwest's `query`, which is behind a feature the
                // workspace does not turn on; the escaping is the same crate underneath either
                // way.
                let endpoint = url::Url::parse_with_params(
                    "https://api.search.brave.com/res/v1/web/search",
                    &[("q", query), ("count", &count.to_string())],
                )
                .map_err(|err| SearchError::Transport(err.to_string()))?;
                http.get(endpoint)
                    .header("Accept", "application/json")
                    .header("X-Subscription-Token", self.key.expose_secret())
            }
            Provider::Tavily => http
                .post("https://api.tavily.com/search")
                .bearer_auth(self.key.expose_secret())
                .json(&serde_json::json!({
                    "query": query,
                    "max_results": count,
                    "search_depth": "basic",
                })),
            Provider::Exa => http
                .post("https://api.exa.ai/search")
                .header("x-api-key", self.key.expose_secret())
                .json(&serde_json::json!({
                    "query": query,
                    "numResults": count,
                    "type": "auto",
                    "contents": { "text": { "maxCharacters": 600 } },
                })),
        };

        let response = request
            .send()
            .await
            .map_err(|err| SearchError::Transport(err.to_string()))?;
        let status = response.status().as_u16();
        // Read through the same cap a page gets. A search service answering with a gigabyte is
        // not a case worth trusting differently just because the user chose the vendor.
        let (bytes, _) = crate::fetch::read_capped(response)
            .await
            .map_err(|err| SearchError::Transport(err.to_string()))?;
        let body = String::from_utf8_lossy(&bytes).into_owned();
        if !(200..300).contains(&status) {
            return Err(SearchError::Rejected {
                status,
                // The provider's own explanation where there is one: it is more specific than
                // anything this connector could say about a key it cannot see.
                message: error_message(&body),
            });
        }
        let json: Value =
            serde_json::from_str(&body).map_err(|err| SearchError::Unreadable(err.to_string()))?;
        Ok(parse_hits(self.provider, &json))
    }
}

/// The message inside an error body, whatever the provider called the field.
fn error_message(body: &str) -> String {
    let trimmed = body.trim();
    let Ok(json) = serde_json::from_str::<Value>(trimmed) else {
        return first_line(trimmed);
    };
    for path in [
        json.get("error").and_then(|e| e.get("detail")),
        json.get("error").and_then(|e| e.get("message")),
        json.get("error"),
        json.get("detail"),
        json.get("message"),
    ] {
        match path {
            Some(Value::String(message)) if !message.is_empty() => return message.clone(),
            _ => {}
        }
    }
    first_line(trimmed)
}

fn first_line(body: &str) -> String {
    let line = body.lines().next().unwrap_or("").trim();
    line.chars().take(300).collect()
}

/// The three answer shapes, normalized to `Hit`.
///
/// Public so the tests can drive it from recorded JSON with no network and no key, which is the
/// only way this part is ever exercised offline.
#[must_use]
pub fn parse_hits(provider: Provider, json: &Value) -> Vec<Hit> {
    let rows = match provider {
        Provider::Brave => json.get("web").and_then(|w| w.get("results")),
        Provider::Tavily => json.get("results"),
        Provider::Exa => json.get("results"),
    };
    let Some(Value::Array(rows)) = rows else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let url = string(row, "url")?;
            let title = string(row, "title").unwrap_or_else(|| url.clone());
            let snippet = match provider {
                // Brave's descriptions carry <strong> around the matched terms.
                Provider::Brave => string(row, "description").map(|d| strip_tags(&d)),
                Provider::Tavily => string(row, "content"),
                Provider::Exa => string(row, "text").or_else(|| string(row, "summary")),
            }
            .unwrap_or_default();
            Some(Hit {
                title: collapse(&title),
                url,
                snippet: collapse(&snippet),
            })
        })
        .collect()
}

fn string(row: &Value, key: &str) -> Option<String> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The highlight markup in a snippet, removed, and the entities that come with it decoded.
///
/// A full HTML parse for `<strong>` would be a lot of machinery for a field that is one sentence
/// long — but treating every `<` as the start of a tag is worse than no stripping at all: a
/// snippet reading "for x < y" loses everything after the `<`. Only a run that actually looks
/// like a tag is dropped.
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        let after = &rest[at + 1..];
        let name = after.strip_prefix('/').unwrap_or(after);
        let tag = name.starts_with(|c: char| c.is_ascii_alphabetic())
            && after.find('>').is_some_and(|end| end <= 32);
        if !tag {
            out.push_str(&rest[..=at]);
            rest = after;
            continue;
        }
        out.push_str(&rest[..at]);
        // `find` succeeded in the check above, so the tag has an end.
        let end = after.find('>').unwrap_or(after.len() - 1);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    decode_entities(&out)
}

/// The handful of entities that actually turn up in a search snippet.
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        // Last, so that `&amp;lt;` decodes to `&lt;` and not to `<`.
        .replace("&amp;", "&")
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_provider_comes_from_the_config_field() {
        assert_eq!(Provider::parse("brave"), Some(Provider::Brave));
        assert_eq!(Provider::parse(" Tavily "), Some(Provider::Tavily));
        assert_eq!(Provider::parse("EXA"), Some(Provider::Exa));
        assert_eq!(Provider::parse("google"), None);
    }

    #[test]
    fn search_is_configured_only_when_both_answers_are_there() {
        let key = || SecretString::from("k".to_owned());
        assert!(Search::from_config(Some("brave"), Some(&key())).is_some());
        assert!(Search::from_config(None, Some(&key())).is_none());
        assert!(Search::from_config(Some("brave"), None).is_none());
        assert!(Search::from_config(Some("nope"), Some(&key())).is_none());
        // A field left blank is not a key.
        assert!(
            Search::from_config(Some("brave"), Some(&SecretString::from("   ".to_owned())))
                .is_none()
        );
    }

    #[test]
    fn a_key_pasted_with_a_newline_is_the_key_without_it() {
        // Pasting a key picks up whitespace. Testing the trimmed form for emptiness but sending
        // the untrimmed one is a 401 the user has no way to explain.
        let search = Search::from_config(
            Some("brave"),
            Some(&SecretString::from("  a-real-key\n".to_owned())),
        )
        .expect("a key with whitespace around it is still a key");
        assert_eq!(search.key.expose_secret(), "a-real-key");
    }

    #[test]
    fn the_key_stays_out_of_debug_output() {
        let search = Search::new(
            Provider::Brave,
            SecretString::from("super-secret".to_owned()),
        );
        let rendered = format!("{search:?}");
        assert!(!rendered.contains("super-secret"), "{rendered}");
    }

    #[test]
    fn a_providers_error_body_is_quoted_back() {
        assert_eq!(
            error_message(r#"{"error":{"detail":"Subscription token invalid"}}"#),
            "Subscription token invalid"
        );
        assert_eq!(
            error_message(r#"{"detail":"rate limited"}"#),
            "rate limited"
        );
        assert_eq!(error_message("plain words"), "plain words");
    }

    #[test]
    fn highlight_markup_is_not_part_of_a_snippet() {
        assert_eq!(strip_tags("a <strong>b</strong> c"), "a b c");
    }
}
