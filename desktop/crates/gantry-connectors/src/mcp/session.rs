//! One live MCP connection (docs/plan/03 §6). The only module besides `mcp::connector` that
//! imports rmcp, so a major version of the SDK touches this directory and nothing else.
//!
//! Version handling is rmcp's `Auto` lifecycle, which is exactly what §6 asks for: probe with
//! `server/discover`, fall back to the `initialize` handshake for the servers that predate it —
//! which, as of the first live probe, is most of them.

use std::{sync::Arc, time::Duration};

use gantry_core::{ResultPart, ServerInfo, ToolDef};
use rmcp::{
    ClientServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, ClientCapabilities, ClientInfo, ContentBlock,
        Implementation, JsonObject, ProtocolVersion,
    },
    service::{ClientLifecycleMode, RoleClient, RunningService},
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use tokio::process::Command;

use crate::mcp::risk::tier_for;

/// How long a connection may take before we give up and tell the user.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("the server needs authorization")]
    Unauthorized,
    #[error("could not start {command}: {source}")]
    Spawn {
        command: String,
        source: std::io::Error,
    },
    #[error("could not connect: {0}")]
    Connect(String),
    #[error("the server took too long to answer")]
    Timeout,
    #[error("{0}")]
    Call(String),
}

/// What to connect to. Secrets are already resolved into `bearer` and `headers` by the caller;
/// this type is short-lived and never persisted.
#[derive(Debug, Clone)]
pub enum Endpoint {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
        cwd: Option<String>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
        /// The `Authorization` value, kept apart because rmcp sends it on every request
        /// including the SSE stream.
        bearer: Option<String>,
    },
}

pub struct McpSession {
    service: RunningService<RoleClient, ClientInfo>,
    server: ServerInfo,
}

impl std::fmt::Debug for McpSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpSession")
            .field("server", &self.server)
            .finish()
    }
}

impl McpSession {
    /// Opens a connection and completes the handshake. A 401 is reported as `Unauthorized` so
    /// the caller can start the OAuth flow instead of showing a connection error (03 §7).
    pub async fn connect(endpoint: &Endpoint) -> Result<Self, McpError> {
        let service = tokio::time::timeout(CONNECT_TIMEOUT, serve(endpoint))
            .await
            .map_err(|_| McpError::Timeout)??;
        let info = service.peer_info();
        let implementation = info.as_ref().and_then(|i| i.server_info.as_ref());
        let server = ServerInfo {
            name: implementation
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "server".to_owned()),
            version: implementation
                .map(|s| s.version.clone())
                .unwrap_or_default(),
            protocol: info
                .as_ref()
                .map(|i| i.protocol_version.as_str().to_owned())
                .unwrap_or_default(),
        };
        Ok(Self { service, server })
    }

    #[must_use]
    pub fn server(&self) -> &ServerInfo {
        &self.server
    }

    /// Every tool the server offers, with a tier derived from its annotations (03 §6), in the
    /// order the server listed them so the model-facing array stays stable.
    pub async fn tools(&self, remote: bool) -> Result<Vec<ToolDef>, McpError> {
        let tools = self
            .service
            .list_all_tools()
            .await
            .map_err(|e| McpError::Call(e.to_string()))?;
        Ok(tools
            .into_iter()
            .map(|t| {
                let tier = tier_for(t.annotations.as_ref(), remote);
                let mut def = ToolDef::new(
                    t.name.to_string(),
                    t.description.map(|d| d.to_string()).unwrap_or_default(),
                    serde_json::Value::Object((*t.input_schema).clone()),
                    tier,
                );
                // An annotation may say a tool is idempotent and read-only; nothing a server
                // claims is trusted enough to make a call parallel-safe by default.
                def.parallel_safe = t
                    .annotations
                    .as_ref()
                    .and_then(|a| a.read_only_hint)
                    .unwrap_or(false);
                def
            })
            .collect())
    }

    /// Calls one tool. MRTR input requests are not answered here: rmcp's `call_tool` walks the
    /// rounds it can, and anything still incomplete comes back as content the model can read.
    pub async fn call(
        &self,
        tool: &str,
        args: &serde_json::Value,
    ) -> Result<(Vec<ResultPart>, Option<serde_json::Value>, bool), McpError> {
        let arguments: Option<JsonObject> = match args {
            serde_json::Value::Object(map) => Some(map.clone()),
            serde_json::Value::Null => None,
            other => Some(JsonObject::from_iter([("value".to_owned(), other.clone())])),
        };
        let mut params = CallToolRequestParams::new(tool.to_owned());
        params.arguments = arguments;
        let result = self
            .service
            .call_tool(params)
            .await
            .map_err(|e| McpError::Call(e.to_string()))?;
        Ok(convert(result))
    }

    /// Stops the connection: closes the stream, or kills the child process tree.
    pub async fn close(self) {
        if let Err(err) = self.service.cancel().await {
            log::debug!("closing an MCP session: {err}");
        }
    }
}

async fn serve(endpoint: &Endpoint) -> Result<RunningService<RoleClient, ClientInfo>, McpError> {
    let info = client_info();
    let lifecycle = ClientLifecycleMode::Auto {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28, ProtocolVersion::LATEST],
        legacy_version: Some(ProtocolVersion::V_2025_06_18),
    };
    match endpoint {
        Endpoint::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let mut cmd = Command::new(command);
            cmd.args(args);
            // A minimal environment (03 §6): what the manifest asked for, plus the little the
            // shell needs to find anything at all.
            cmd.env_clear();
            for key in [
                "PATH",
                "HOME",
                "USERPROFILE",
                "SystemRoot",
                "TMPDIR",
                "TEMP",
            ] {
                if let Ok(value) = std::env::var(key) {
                    cmd.env(key, value);
                }
            }
            for (key, value) in env {
                cmd.env(key, value);
            }
            if let Some(dir) = cwd {
                cmd.current_dir(dir);
            }
            let transport = TokioChildProcess::new(cmd).map_err(|source| McpError::Spawn {
                command: command.clone(),
                source,
            })?;
            info.serve_with_lifecycle(transport, lifecycle)
                .await
                .map_err(|e| McpError::Connect(e.to_string()))
        }
        Endpoint::Http {
            url,
            headers,
            bearer,
        } => {
            let mut config = StreamableHttpClientTransportConfig::with_uri(url.clone());
            config.auth_header = bearer.clone();
            let client = reqwest::Client::builder()
                .default_headers(header_map(headers))
                .build()
                .map_err(|e| McpError::Connect(e.to_string()))?;
            let transport = StreamableHttpClientTransport::with_client(client, config);
            info.serve_with_lifecycle(transport, lifecycle)
                .await
                .map_err(|e| classify_http(&e.to_string()))
        }
    }
}

/// A 401 is not a connection failure, it is the start of the OAuth flow (03 §7).
fn classify_http(message: &str) -> McpError {
    if message.contains("401") || message.to_ascii_lowercase().contains("unauthorized") {
        McpError::Unauthorized
    } else {
        McpError::Connect(message.to_owned())
    }
}

fn header_map(headers: &[(String, String)]) -> reqwest::header::HeaderMap {
    let mut map = reqwest::header::HeaderMap::new();
    for (name, value) in headers {
        match (
            reqwest::header::HeaderName::try_from(name.as_str()),
            reqwest::header::HeaderValue::from_str(value),
        ) {
            (Ok(name), Ok(value)) => {
                map.insert(name, value);
            }
            _ => log::warn!("dropping an unusable header name: {name}"),
        }
    }
    map
}

fn client_info() -> ClientInfo {
    ClientInfo::new(
        ClientCapabilities::default(),
        Implementation::new("Gantry", env!("CARGO_PKG_VERSION")).with_title("Gantry"),
    )
}

/// MCP content blocks as Gantry's result parts. Anything that is not text or an image becomes
/// its JSON, so nothing a server returns is silently dropped from the transcript.
fn convert(result: CallToolResult) -> (Vec<ResultPart>, Option<serde_json::Value>, bool) {
    let is_error = result.is_error.unwrap_or(false);
    let mut parts = Vec::new();
    for block in result.content {
        match block {
            ContentBlock::Text(text) => parts.push(ResultPart::Text { text: text.text }),
            ContentBlock::Image(image) => parts.push(ResultPart::Image {
                mime: image.mime_type.clone(),
                data: image.data.clone(),
            }),
            other => parts.push(ResultPart::Json {
                json: serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
            }),
        }
    }
    if let Some(structured) = &result.structured_content
        && parts.is_empty()
    {
        parts.push(ResultPart::Json {
            json: structured.clone(),
        });
    }
    (parts, result.structured_content, is_error)
}

/// A session that can be reopened. Kept as a field of the connector so a dropped connection is
/// re-established on the next call instead of failing the turn (03 §6).
pub type SharedSession = Arc<tokio::sync::Mutex<Option<McpSession>>>;
