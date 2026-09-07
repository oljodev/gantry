//! The HTTP plumbing every client shares: the reqwest client, authenticated GETs, and the
//! streaming POST with retry before the first byte (docs/plan/02 §7). Header maps carry the
//! key, so nothing here logs them.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;

use crate::{error::ProviderError, retry};

/// The shared HTTP client: connect timeout only; per-request deadlines are applied per stream.
pub fn http_client(app_version: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(retry::CONNECT_TIMEOUT)
        .user_agent(format!("Gantry/{app_version}"))
        .build()
        .expect("a default reqwest client builds")
}

/// Adds a header when both the name and the value are well-formed; silently skips otherwise.
pub(crate) fn put(headers: &mut HeaderMap, name: &str, value: &str) {
    if let (Ok(n), Ok(v)) = (
        HeaderName::from_bytes(name.as_bytes()),
        HeaderValue::from_str(value),
    ) {
        headers.insert(n, v);
    }
}

pub(crate) fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// `base/path` with exactly one slash between them.
pub(crate) fn join(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// A GET mapped to a `ProviderError` on a non-2xx status or a malformed body.
pub(crate) async fn get_json(
    http: &reqwest::Client,
    url: &str,
    headers: HeaderMap,
) -> Result<Value, ProviderError> {
    let response = http
        .get(url)
        .headers(headers)
        .timeout(Duration::from_secs(30))
        .send()
        .await?;
    let status = response.status();
    let retry_after = retry_after(response.headers());
    let body = response.text().await?;
    if !status.is_success() {
        return Err(ProviderError::from_status(
            status.as_u16(),
            &body,
            retry_after,
        ));
    }
    serde_json::from_str(&body)
        .map_err(|e| ProviderError::interrupted(format!("malformed JSON from {url}: {e}")))
}

/// Opens a streaming POST, retrying rate limits, overload and network failures until the
/// first byte; a non-2xx status becomes a classified error.
pub(crate) async fn post_stream(
    http: &reqwest::Client,
    url: &str,
    headers: HeaderMap,
    body: &Value,
) -> Result<reqwest::Response, ProviderError> {
    retry::with_retry(|| async {
        let response = http
            .post(url)
            .headers(headers.clone())
            .json(body)
            .send()
            .await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let retry_after = retry_after(response.headers());
        let text = response.text().await.unwrap_or_default();
        Err(ProviderError::from_status(
            status.as_u16(),
            &text,
            retry_after,
        ))
    })
    .await
}
