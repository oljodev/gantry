//! Retry before the first byte only (docs/plan/02 §7): three attempts with jittered backoff on
//! rate limits, overload and network failures. Once output has started nothing is re-sent.

use std::{future::Future, time::Duration};

use crate::error::ProviderError;

pub const ATTEMPTS: u32 = 3;
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
pub const FIRST_TOKEN_TIMEOUT: Duration = Duration::from_secs(60);
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Runs `attempt` up to [`ATTEMPTS`] times, sleeping between retryable failures.
pub async fn with_retry<T, F, Fut>(mut attempt: F) -> Result<T, ProviderError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ProviderError>>,
{
    let mut n = 0;
    loop {
        n += 1;
        match attempt().await {
            Ok(v) => return Ok(v),
            Err(err) if err.is_retryable() && n < ATTEMPTS => {
                let delay = backoff(n, err.retry_after);
                log::warn!(
                    "provider attempt {n} failed ({}); retrying in {delay:?}",
                    err.message
                );
                tokio::time::sleep(delay).await;
            }
            Err(err) => return Err(err),
        }
    }
}

/// 500 ms, 1 s, 2 s … plus up to 250 ms of jitter, or the provider's own `Retry-After`
/// (capped at 30 s) when it sent one.
fn backoff(attempt: u32, retry_after: Option<Duration>) -> Duration {
    if let Some(ra) = retry_after {
        return ra.min(Duration::from_secs(30));
    }
    let base = Duration::from_millis(500 * 2u64.pow(attempt.saturating_sub(1)));
    let jitter = Duration::from_millis((gantry_core::now_ms().unsigned_abs()) % 250);
    base + jitter
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use gantry_core::ProviderErrorKind;

    use super::*;

    #[tokio::test]
    async fn retries_retryable_failures_then_gives_up() {
        let calls = AtomicU32::new(0);
        let result: Result<(), ProviderError> = with_retry(|| async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(ProviderError::new(ProviderErrorKind::Overloaded, "busy"))
        })
        .await;
        assert_eq!(result.unwrap_err().kind, ProviderErrorKind::Overloaded);
        assert_eq!(calls.load(Ordering::SeqCst), ATTEMPTS);
    }

    #[tokio::test]
    async fn does_not_retry_auth_failures() {
        let calls = AtomicU32::new(0);
        let result: Result<(), ProviderError> = with_retry(|| async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(ProviderError::auth("bad key"))
        })
        .await;
        assert_eq!(result.unwrap_err().kind, ProviderErrorKind::Auth);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
