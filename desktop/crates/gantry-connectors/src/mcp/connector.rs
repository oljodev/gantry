//! An MCP server as a `Connector` (docs/plan/03 §4, §6): one lazily-opened session, a cached
//! tool list, and a call path that reconnects once rather than failing a turn because a server
//! idled out from under it.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use gantry_core::{ConnectorKind, InstanceId, ToolDef};
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;

use crate::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
    mcp::session::{Endpoint, McpError, McpSession},
};

/// A server idle for this long is stopped; the next call opens it again (03 §6).
const IDLE_STOP: Duration = Duration::from_secs(10 * 60);

pub struct McpConnector {
    descriptor: ConnectorDescriptor,
    endpoint: Endpoint,
    remote: bool,
    session: AsyncMutex<Option<Live>>,
    /// The last known tool list, so the tool set can be assembled without waking the server.
    cached_tools: Mutex<Option<Vec<ToolDef>>>,
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
            cached_tools: Mutex::new(cached_tools),
        }
    }

    /// Connects if necessary and returns the tools the server offers, refreshing the cache.
    pub async fn refresh(&self) -> Result<Vec<ToolDef>, McpError> {
        let mut guard = self.session.lock().await;
        let live = self.ensure(&mut guard).await?;
        let tools = live.session.tools(self.remote).await?;
        self.remember(tools.clone());
        Ok(tools)
    }

    fn remember(&self, tools: Vec<ToolDef>) {
        *self.cached_tools.lock().unwrap_or_else(|e| e.into_inner()) = Some(tools);
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
            let session = McpSession::connect(&self.endpoint).await?;
            match session.tools(self.remote).await {
                Ok(tools) => self.remember(tools),
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
        if let Some(tools) = self
            .cached_tools
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Ok(tools);
        }
        self.refresh()
            .await
            .map_err(|e| ConnectorError::Failed(e.to_string()))
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        _sink: Arc<dyn ToolEventSink>,
        cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        let call = async {
            let mut guard = self.session.lock().await;
            let live = self
                .ensure(&mut guard)
                .await
                .map_err(|e| ConnectorError::Failed(e.to_string()))?;
            match live.session.call(&req.tool, &req.args).await {
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
                        .call(&req.tool, &req.args)
                        .await
                        .map_err(|e| ConnectorError::Failed(e.to_string()))
                }
                Err(err) => Err(ConnectorError::Failed(err.to_string())),
            }
        };
        let (content, structured, is_error) = tokio::select! {
            _ = cancel.cancelled() => return Ok(ToolOutcome::error("Cancelled by the user.")),
            result = call => result?,
        };
        Ok(ToolOutcome::Complete {
            content,
            structured,
            is_error,
        })
    }
}
