//! How fast this connector is allowed to ask (`docs/connectors/web.md` §7.3).
//!
//! | Policy | Value |
//! |--------|-------|
//! | Per host | 2 concurrent, at least half a second apart |
//! | Globally | A handful of concurrent requests; the user's own browser wants the uplink too |
//!
//! Neither was enforced. `fetch_url` is `parallel_safe`, so a model that opens eight pages of
//! one documentation site opens eight connections to it at once — and §15 measured what that
//! earns: ten-way concurrency drew a rate-limit refusal from a site whose robots file asks for a
//! thirty-second delay. The refusal is then attributed to Gantry for twenty minutes, and the
//! person who pays for it is the user whose address it is.
//!
//! The gap is on the *start* of a request, not on its finish: two requests to one host may
//! overlap, they may just not begin together. That is what the table says, and it is also the
//! part a server notices.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

/// Requests in flight to one host.
pub const PER_HOST: usize = 2;

/// The shortest time between two requests to the same host.
pub const MIN_GAP: Duration = Duration::from_millis(500);

/// Requests in flight altogether. A handful: the uplink belongs to the person using the
/// computer, and a tool call is not the only thing on it.
pub const GLOBAL: usize = 6;

/// The gate every page fetch passes through.
#[derive(Debug)]
pub struct Politeness {
    global: Arc<Semaphore>,
    hosts: Mutex<HashMap<String, Arc<Host>>>,
}

#[derive(Debug)]
struct Host {
    slots: Arc<Semaphore>,
    /// When a request to this host last *started*. An async mutex because it is held across the
    /// wait, which is what serialises the starts.
    last: tokio::sync::Mutex<Option<Instant>>,
}

/// Permission to make one request, for as long as it is held.
#[derive(Debug)]
pub struct Pass {
    _global: OwnedSemaphorePermit,
    _host: OwnedSemaphorePermit,
}

impl Default for Politeness {
    fn default() -> Self {
        Self::new()
    }
}

impl Politeness {
    #[must_use]
    pub fn new() -> Self {
        Self {
            global: Arc::new(Semaphore::new(GLOBAL)),
            hosts: Mutex::new(HashMap::new()),
        }
    }

    /// Wait until it is this request's turn. Held for the request; dropped when it is done.
    pub async fn wait(&self, host: &str) -> Pass {
        self.wait_with(host, MIN_GAP).await
    }

    pub(crate) async fn wait_with(&self, host: &str, gap: Duration) -> Pass {
        let gate = self.host(host);
        // Global first, then the host: the other order lets a host's two slots be held by tasks
        // queueing for a global permit, so one slow host would stall every other one behind it.
        let global = Arc::clone(&self.global)
            .acquire_owned()
            .await
            .expect("the global gate is never closed");
        let permit = Arc::clone(&gate.slots)
            .acquire_owned()
            .await
            .expect("a host gate is never closed");
        {
            let mut last = gate.last.lock().await;
            if let Some(at) = *last {
                let since = at.elapsed();
                if since < gap {
                    tokio::time::sleep(gap - since).await;
                }
            }
            *last = Some(Instant::now());
        }
        Pass {
            _global: global,
            _host: permit,
        }
    }

    fn host(&self, host: &str) -> Arc<Host> {
        let mut hosts = self.hosts.lock().unwrap_or_else(|e| e.into_inner());
        // Hosts are not forgotten. One entry is two words and a semaphore, a chat reaches a
        // handful of hosts, and the alternative — expiring them — would be a second clock to
        // get wrong for no measurable saving.
        Arc::clone(hosts.entry(host.to_owned()).or_insert_with(|| {
            Arc::new(Host {
                slots: Arc::new(Semaphore::new(PER_HOST)),
                last: tokio::sync::Mutex::new(None),
            })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn two_requests_to_one_host_start_a_gap_apart() {
        let gate = Politeness::new();
        let started = Instant::now();
        let _first = gate.wait("example.com").await;
        assert_eq!(
            started.elapsed(),
            Duration::ZERO,
            "the first one waits for nothing"
        );
        let _second = gate.wait("example.com").await;
        assert!(
            started.elapsed() >= MIN_GAP,
            "the second waited: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_different_host_is_not_kept_waiting() {
        let gate = Politeness::new();
        let _first = gate.wait("example.com").await;
        let started = Instant::now();
        let _other = gate.wait("other.example").await;
        assert_eq!(
            started.elapsed(),
            Duration::ZERO,
            "one slow host does not stall the rest"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_third_request_to_one_host_waits_for_a_slot() {
        let gate = Arc::new(Politeness::new());
        let first = gate.wait("example.com").await;
        let _second = gate.wait("example.com").await;

        let waiting = tokio::spawn({
            let gate = Arc::clone(&gate);
            async move { gate.wait("example.com").await }
        });
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(!waiting.is_finished(), "both slots are taken");

        drop(first);
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(waiting.is_finished(), "and it goes when one is given back");
        drop(waiting.await.unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn the_global_gate_bounds_everything_together() {
        let gate = Arc::new(Politeness::new());
        let mut held = Vec::new();
        for i in 0..GLOBAL {
            held.push(gate.wait(&format!("host-{i}.example")).await);
        }
        let waiting = tokio::spawn({
            let gate = Arc::clone(&gate);
            async move { gate.wait("one-more.example").await }
        });
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(
            !waiting.is_finished(),
            "a {GLOBAL}th host does not make it {} requests",
            GLOBAL + 1
        );
        held.pop();
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(waiting.is_finished());
        drop(waiting.await.unwrap());
    }
}
