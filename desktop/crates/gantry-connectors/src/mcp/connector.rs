//! An MCP server as a `Connector` (docs/plan/03 §4, §6): one lazily-opened session, a cached
//! tool list, and a call path that reconnects once rather than failing a turn because a server
//! idled out from under it.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use gantry_core::{ConnectorKind, InstanceId, ToolDef};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

use crate::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
    mcp::session::{Endpoint, McpError, McpSession, ToolListing},
};

/// A server idle for this long is stopped; the next call opens it again (03 §6).
const IDLE_STOP: Duration = Duration::from_secs(10 * 60);

/// How long a tool list is trusted when the server did not say (03 §6).
///
/// Every revision before 2026-07-28 carries no `ttlMs`, which is most servers today, and the
/// cache used to have no expiry at all: a tool list read at startup was still being handed to the
/// model a week later, and a server that had gained or lost a tool in between was misrepresented
/// until the app restarted. Ten minutes is the same number as [`IDLE_STOP`] on purpose — a server
/// that has been idle that long is about to be re-listed on its next use anyway, so the two
/// timers agree instead of fighting.
const DEFAULT_TTL: Duration = Duration::from_secs(10 * 60);

pub struct McpConnector {
    descriptor: ConnectorDescriptor,
    endpoint: Endpoint,
    remote: bool,
    session: AsyncMutex<Option<Live>>,
    /// The last known tool list, so the tool set can be assembled without waking the server.
    cached_tools: Mutex<Option<Cached>>,
    /// Set by the session's handler when the server sends `tools/list_changed` — the one moment
    /// a cache is known to be wrong rather than merely old (03 §6).
    tools_changed: Arc<AtomicBool>,
}

/// A tool list and when it stops being trusted.
struct Cached {
    tools: Vec<ToolDef>,
    fresh_until: Instant,
}

struct Live {
    session: McpSession,
    last_used: Instant,
}

impl McpConnector {
    #[must_use]
    pub fn new(
        id: String,
        name: String,
        instance_id: InstanceId,
        kind: ConnectorKind,
        endpoint: Endpoint,
        cached_tools: Option<Vec<ToolDef>>,
    ) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id,
                name,
                instance_id: Some(instance_id),
                first_party: false,
            },
            endpoint,
            remote: kind == ConnectorKind::McpRemote,
            session: AsyncMutex::new(None),
            // A list read from the database at startup is a starting point, not a fresh answer:
            // it was true when it was written and nothing since has checked. It gets the default
            // TTL like any other, so the first use after ten minutes re-lists.
            cached_tools: Mutex::new(cached_tools.map(|tools| Cached {
                tools,
                fresh_until: Instant::now() + DEFAULT_TTL,
            })),
            tools_changed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Connects if necessary and returns the tools the server offers, refreshing the cache.
    pub async fn refresh(&self) -> Result<Vec<ToolDef>, McpError> {
        let mut guard = self.session.lock().await;
        let live = self.ensure(&mut guard).await?;
        let listing = live.session.tools(self.remote).await?;
        self.remember(listing.clone());
        Ok(listing.tools)
    }

    fn remember(&self, listing: ToolListing) {
        // A notification that arrived while this listing was in flight is about the list we just
        // asked for, so clearing it here is right; one that arrives after sets the flag again.
        self.tools_changed.store(false, Ordering::Relaxed);
        *self.cached_tools.lock().unwrap_or_else(|e| e.into_inner()) = Some(Cached {
            tools: listing.tools,
            fresh_until: Instant::now() + listing.ttl.unwrap_or(DEFAULT_TTL),
        });
    }

    /// The cached list, when it is still worth trusting: the server has not said it changed, and
    /// the time it said to trust it for has not run out.
    fn fresh(&self) -> Option<Vec<ToolDef>> {
        if self.tools_changed.load(Ordering::Relaxed) {
            return None;
        }
        let guard = self.cached_tools.lock().unwrap_or_else(|e| e.into_inner());
        let cached = guard.as_ref()?;
        (cached.fresh_until > Instant::now()).then(|| cached.tools.clone())
    }

    /// Opens the session if there is none, and lists the tools over it once.
    ///
    /// The listing is not for us — we have the tools cached. It is for the connection: a
    /// server may declare that some arguments travel as `Mcp-Param-*` headers rather than in
    /// the body (SEP-2243, which GitHub's server enforces), and the client learns which ones
    /// from a `tools/list` it has seen on that connection. A session that goes straight to a
    /// call sends none of them and is refused with "header mismatch". Best effort: a server
    /// that cannot list is still worth calling, and its error will say so.
    async fn ensure<'a>(&self, guard: &'a mut Option<Live>) -> Result<&'a mut Live, McpError> {
        if guard.is_none() {
            let session =
                McpSession::connect(&self.endpoint, Arc::clone(&self.tools_changed)).await?;
            match session.tools(self.remote).await {
                Ok(listing) => self.remember(listing),
                Err(err) => log::warn!("{} listed no tools on connect: {err}", self.descriptor.id),
            }
            *guard = Some(Live {
                session,
                last_used: Instant::now(),
            });
        }
        let live = guard.as_mut().expect("just connected");
        live.last_used = Instant::now();
        Ok(live)
    }

    /// Drops the connection: called on uninstall, on disable, and by the idle sweep.
    pub async fn stop(&self) {
        if let Some(live) = self.session.lock().await.take() {
            live.session.close().await;
        }
    }

    /// Closes the connection when it has been unused for `IDLE_STOP`. Returns whether it did.
    pub async fn stop_if_idle(&self) -> bool {
        let mut guard = self.session.lock().await;
        let idle = guard
            .as_ref()
            .is_some_and(|live| live.last_used.elapsed() >= IDLE_STOP);
        if idle && let Some(live) = guard.take() {
            live.session.close().await;
        }
        idle
    }
}

#[async_trait]
impl Connector for McpConnector {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    /// The cached list when there is one. Assembling a turn's tool set must not spawn a process
    /// or open a connection: a chat with six connectors attached would pay for all six on every
    /// message (03 §6, tool list caching).
    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        if let Some(tools) = self.fresh() {
            return Ok(tools);
        }
        match self.refresh().await {
            Ok(tools) => Ok(tools),
            // A stale list beats no list. The server is unreachable, and answering a turn with
            // "this connector has no tools" would make the model apologise for a capability it
            // still has; the call it then makes fails with the connection error, which is the
            // true thing to say and says it in the right place.
            Err(err) => {
                let stale = self
                    .cached_tools
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                    .map(|c| c.tools.clone());
                match stale {
                    Some(tools) => {
                        log::warn!("{}: using the last tool list: {err}", self.descriptor.id);
                        Ok(tools)
                    }
                    None => Err(ConnectorError::Failed(err.to_string())),
                }
            }
        }
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        sink: Arc<dyn ToolEventSink>,
        cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        let call = async {
            let mut guard = self.session.lock().await;
            let live = self
                .ensure(&mut guard)
                .await
                .map_err(|e| ConnectorError::Failed(e.to_string()))?;
            // The card this call would raise, minus the parts only the server knows. Built
            // here because this is the only place that has both the call and the connector.
            let waiting = gantry_core::ElicitationRequest {
                call_id: req.call_id.clone(),
                connector: self.descriptor.id.clone(),
                connector_name: self.descriptor.name.clone(),
                message: String::new(),
                fields: Vec::new(),
            };
            match live
                .session
                .call(&req.tool, &req.args, sink.as_ref(), &waiting)
                .await
            {
                Ok(out) => Ok(out),
                // A server that went away between calls gets one reconnection, because the
                // alternative is a failed turn for a connection Gantry closed itself.
                Err(McpError::Call(message)) => {
                    log::debug!("retrying {} after: {message}", req.tool);
                    if let Some(live) = guard.take() {
                        live.session.close().await;
                    }
                    let live = self
                        .ensure(&mut guard)
                        .await
                        .map_err(|e| ConnectorError::Failed(e.to_string()))?;
                    live.session
                        .call(&req.tool, &req.args, sink.as_ref(), &waiting)
                        .await
                        .map_err(|e| ConnectorError::Failed(e.to_string()))
                }
                Err(err) => Err(ConnectorError::Failed(err.to_string())),
            }
        };
        // Biased, so a turn the user has already stopped never reaches the server: an
        // unbiased `select!` picks a ready branch at random, which makes "does Stop stop it"
        // a coin toss rather than an answer.
        let (content, structured, is_error) = tokio::select! {
            biased;
            () = cancel.cancelled() => return Ok(ToolOutcome::cancelled()),
            result = call => result?,
        };
        Ok(ToolOutcome::Complete {
            content,
            structured,
            is_error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gantry_core::RiskTier;

    /// A connector with a cache and an endpoint nobody connects to: these are tests about when a
    /// tool list stops being trusted, and answering that must not need a server.
    fn connector(cached: Option<Vec<ToolDef>>) -> McpConnector {
        McpConnector::new(
            "test".to_owned(),
            "Test".to_owned(),
            InstanceId::new(),
            ConnectorKind::McpRemote,
            Endpoint::Http {
                url: "https://example.invalid/mcp".to_owned(),
                headers: Vec::new(),
                bearer: None,
            },
            cached,
        )
    }

    fn tool(name: &str) -> ToolDef {
        ToolDef::new(name, "d", serde_json::json!({}), RiskTier::Read)
    }

    #[test]
    fn a_list_read_from_the_database_is_a_starting_point_and_still_expires() {
        let c = connector(Some(vec![tool("a")]));
        assert_eq!(c.fresh().map(|t| t.len()), Some(1));

        // It was true when it was written and nothing since has checked, so it ages like any
        // other answer rather than lasting until the app restarts.
        c.cached_tools.lock().unwrap().as_mut().unwrap().fresh_until =
            Instant::now() - Duration::from_secs(1);
        assert!(c.fresh().is_none(), "an expired list is not fresh");
    }

    #[test]
    fn the_servers_own_ttl_beats_the_default() {
        let c = connector(None);
        c.remember(ToolListing {
            tools: vec![tool("a")],
            ttl: Some(Duration::from_millis(1)),
        });
        std::thread::sleep(Duration::from_millis(5));
        assert!(c.fresh().is_none(), "the server said one millisecond");

        c.remember(ToolListing {
            tools: vec![tool("a")],
            ttl: None,
        });
        assert!(c.fresh().is_some(), "and nothing said means the default");
    }

    /// A turn the user has stopped never reaches the server (03 §4). The endpoint here resolves
    /// to nothing, so a call that tried to connect would spend a DNS timeout before failing;
    /// this one answers at once, which is the whole point of reading the token first.
    #[tokio::test]
    async fn a_cancelled_call_never_opens_the_connection() {
        let c = connector(None);
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let started = Instant::now();
        let outcome = c
            .call(
                ToolCallRequest {
                    call_id: gantry_core::CallId::new(),
                    tool: "anything".to_owned(),
                    args: serde_json::json!({}),
                    scope: crate::ChatScope {
                        chat_id: gantry_core::ChatId::new(),
                        turn_id: gantry_core::TurnId::new(),
                        mode: gantry_core::Mode::Auto,
                        attach_decided: true,
                    },
                },
                Arc::new(crate::NoopToolEvents),
                cancelled,
            )
            .await
            .expect("a stopped call is a result, not an error");
        let ToolOutcome::Complete {
            content, is_error, ..
        } = outcome;
        assert!(is_error);
        assert_eq!(
            gantry_core::result_preview(&content, usize::MAX),
            crate::CANCELLED
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "it did not try the network first"
        );
    }

    /// `tools/list_changed` is the one moment a cache is known to be wrong rather than old, so
    /// it does not wait for a TTL — and re-listing clears it, or every later read would refetch.
    #[test]
    fn a_list_changed_notification_expires_the_cache_at_once() {
        let c = connector(Some(vec![tool("a")]));
        assert!(c.fresh().is_some());
        c.tools_changed.store(true, Ordering::Relaxed);
        assert!(c.fresh().is_none());

        c.remember(ToolListing {
            tools: vec![tool("a"), tool("b")],
            ttl: None,
        });
        assert_eq!(c.fresh().map(|t| t.len()), Some(2));
    }
}
