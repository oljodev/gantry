//! One HTTP GET, with every bound the plan asks for made real (03 §5): 5 MB, ≤5 redirects, a
//! 20 s deadline, no cookies, and the address check of `guard` run again on every hop.
//!
//! Redirects are followed by hand rather than by reqwest, which is the point of the module. A
//! client that follows them itself will happily follow a public URL to `http://127.0.0.1:6379/`,
//! and by the time the response comes back the request that mattered has already been made.
//! Here every hop is a URL that went through `guard::parse` and `guard::check_address` first.

use std::time::Duration;

use futures_util::StreamExt;
use url::Url;

use crate::classify::{self, Verdict};
use crate::guard::{self, Refusal};

/// 03 §5. A page larger than this is not a page anybody wanted read aloud.
pub const MAX_BYTES: usize = 5 * 1024 * 1024;
/// 03 §5.
pub const MAX_REDIRECTS: usize = 5;
/// 03 §5. Per request — see `DEADLINE` for the bound that matters.
pub const TIMEOUT: Duration = Duration::from_secs(20);

/// The whole call, redirects included.
///
/// `TIMEOUT` is reqwest's, and reqwest applies it per request. With six hops allowed that is a
/// two-minute tool call that the plan's "20 s timeout" does not describe and nobody waiting on
/// an answer would expect, so the chain gets a deadline of its own.
pub const DEADLINE: Duration = Duration::from_secs(30);

/// What Gantry says it is. A real product name and a contact page: a fetcher that disguises
/// itself as a browser is asking site owners not to be able to block it, and that is not a
/// position to take on the user's behalf.
pub const USER_AGENT: &str = concat!(
    "GantryBot/",
    env!("CARGO_PKG_VERSION"),
    " (+https://oljo.dev/connectors/web/)"
);

pub struct Fetched {
    /// After redirects — what the content is actually the content *of*.
    pub final_url: Url,
    pub status: u16,
    /// What the status line and the headers say this response *is* (§7.5): a page, or a
    /// challenge, a refusal or a wait wearing a page's content type.
    pub verdict: Verdict,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    /// The 5 MB cap was hit and the rest of the response was not read.
    pub truncated: bool,
    /// The hops taken, final URL included, when there were any.
    pub redirects: Vec<String>,
}

#[derive(Debug)]
pub enum FetchError {
    Refused(Refusal),
    TooManyRedirects(usize),
    /// The redirect chain outlived `DEADLINE`.
    DeadlineExceeded,
    /// A redirect with no usable `Location`, which is a broken server rather than a refusal.
    BadRedirect(String),
    Transport(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(refusal) => refusal.fmt(f),
            Self::DeadlineExceeded => write!(
                f,
                "that URL was still redirecting after {} seconds, so the fetch was stopped.",
                DEADLINE.as_secs()
            ),
            Self::TooManyRedirects(n) => write!(
                f,
                "that URL redirected more than {n} times, so the fetch was stopped. The page may \
                 require a sign-in, or be in a redirect loop."
            ),
            Self::BadRedirect(url) => {
                write!(f, "{url} answered with a redirect to nowhere.")
            }
            Self::Transport(message) => write!(f, "the page could not be fetched: {message}"),
        }
    }
}

impl From<Refusal> for FetchError {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

/// A client with the posture of §5 baked in: it follows nothing by itself and stores no cookies,
/// so neither can be forgotten at a call site.
///
/// Cookies need no switching off: reqwest keeps a store only when its `cookies` feature is on,
/// and the workspace does not enable it. The pin is the guarantee, so there is nothing to call
/// here — but if `cookies` is ever turned on for some other crate, this client would start
/// keeping them, and `.cookie_store(false)` is the line to add back.
#[must_use]
pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        // Only a TLS backend that will not initialise can fail here, and that is not a condition
        // this process can run in at all.
        .expect("an HTTP client with no unusual configuration")
}

/// Fetch one URL, and honour a `Retry-After` once (§7.3: always honoured).
///
/// One retry, not two: the hint is capped at [`classify::MAX_RETRY_AFTER`] before it gets here,
/// and a person is sitting in front of the tool call. A challenge is never retried at all — that
/// is decided in [`classify`], which hands back no retry for one.
pub async fn get(http: &reqwest::Client, raw: &str) -> Result<Fetched, FetchError> {
    let first = get_once(http, raw).await?;
    let Some(after) = first.verdict.retry_after() else {
        return Ok(first);
    };
    log::debug!("{raw} asked for {}s; waiting", after.as_secs());
    tokio::time::sleep(after).await;
    get_once(http, raw).await
}

/// Fetch one URL, following redirects by hand and checking each one.
async fn get_once(http: &reqwest::Client, raw: &str) -> Result<Fetched, FetchError> {
    let mut url = guard::parse(raw)?;
    let mut redirects: Vec<String> = Vec::new();
    let started = tokio::time::Instant::now();

    for _ in 0..=MAX_REDIRECTS {
        if started.elapsed() >= DEADLINE {
            return Err(FetchError::DeadlineExceeded);
        }
        guard::check_address(&url).await?;
        let response = http
            .get(url.clone())
            .header(reqwest::header::ACCEPT, "text/html,text/*;q=0.9,*/*;q=0.5")
            .send()
            .await
            .map_err(|err| FetchError::Transport(transport_message(&err)))?;

        let status = response.status();
        if status.is_redirection() {
            // Read lossily rather than with `to_str`, which refuses any byte over 0x7f: a
            // `Location` with a non-ASCII path is against the letter of the spec and common in
            // the wild, and "a redirect to nowhere" is the wrong thing to say about it. `join`
            // percent-encodes what comes out.
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
                .ok_or_else(|| FetchError::BadRedirect(url.to_string()))?;
            let location = location.trim();
            // Relative redirects are the common case, so the next hop is resolved against the
            // one that issued it rather than parsed on its own.
            let next = url
                .join(location)
                .map_err(|_| FetchError::BadRedirect(url.to_string()))?;
            // Re-checked as a *new* URL: `https://ok.example/x` may not redirect to `file:///`
            // any more than the model may ask for it directly.
            let next = guard::parse(next.as_str())?;
            redirects.push(url.to_string());
            url = next;
            continue;
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        // Decided before the body is read, from the status line and the headers alone (§7.5).
        // A challenge is a 200 as often as it is a 403, so this cannot wait for the reader.
        let verdict = classify::from_response(status.as_u16(), |name| {
            response
                .headers()
                .get(name)
                .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
        });
        let (body, truncated) = read_capped(response).await?;
        if !redirects.is_empty() {
            redirects.push(url.to_string());
        }
        return Ok(Fetched {
            final_url: url,
            status: status.as_u16(),
            verdict,
            content_type,
            body,
            truncated,
            redirects,
        });
    }
    Err(FetchError::TooManyRedirects(MAX_REDIRECTS))
}

/// Read the body, stopping at the cap.
///
/// Streamed rather than `bytes()`, because the cap has to bound what is *read* and not only what
/// is kept: a `Content-Length` of 4 GB is a promise the server makes and not one it has to keep,
/// and `bytes()` would buffer all of it before anything here got to object.
pub(crate) async fn read_capped(
    response: reqwest::Response,
) -> Result<(Vec<u8>, bool), FetchError> {
    let mut body: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|err| FetchError::Transport(transport_message(&err)))?;
        let room = MAX_BYTES - body.len();
        // `>` and not `>=`: a chunk that exactly fills the cap left nothing out, and the next
        // turn of the loop is what discovers whether the stream had more. Reporting a complete
        // 5 MB body as truncated would send the model back for a second copy of all of it.
        if chunk.len() > room {
            body.extend_from_slice(&chunk[..room]);
            return Ok((body, true));
        }
        body.extend_from_slice(&chunk);
    }
    Ok((body, false))
}

/// reqwest's own `Display` is a chain of wrapper types; the model wants the sentence at the end
/// of it, and "operation timed out" is more useful than "error sending request for url (…)".
fn transport_message(err: &reqwest::Error) -> String {
    if err.is_timeout() {
        return format!("it did not answer within {} seconds", TIMEOUT.as_secs());
    }
    if err.is_connect() {
        return "the connection was refused".to_owned();
    }
    let mut source: &dyn std::error::Error = err;
    while let Some(next) = source.source() {
        source = next;
    }
    source.to_string()
}

/// What a `Content-Type` says the body is, lowercased and without its parameters.
#[must_use]
pub fn mime(content_type: Option<&str>) -> String {
    content_type
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

/// Whether a MIME type is text this connector can turn into something readable.
#[must_use]
pub fn is_readable(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime.ends_with("+json")
        || mime.ends_with("+xml")
        || matches!(
            mime,
            "application/json"
                | "application/xml"
                | "application/xhtml+xml"
                | "application/javascript"
                | "application/x-ndjson"
                | ""
        )
}

#[must_use]
pub fn is_html(mime: &str) -> bool {
    matches!(mime, "text/html" | "application/xhtml+xml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_content_type_is_reduced_to_its_type() {
        assert_eq!(mime(Some("text/html; charset=utf-8")), "text/html");
        assert_eq!(mime(Some("TEXT/HTML")), "text/html");
        assert_eq!(mime(None), "");
    }

    #[test]
    fn text_is_readable_and_a_binary_is_not() {
        assert!(is_readable("text/html"));
        assert!(is_readable("text/plain"));
        assert!(is_readable("application/json"));
        assert!(is_readable("application/ld+json"));
        assert!(is_readable(""));
        assert!(!is_readable("application/pdf"));
        assert!(!is_readable("image/png"));
        assert!(!is_readable("application/zip"));
    }
}
