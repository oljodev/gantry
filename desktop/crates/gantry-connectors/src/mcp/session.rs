//! One live MCP connection (docs/plan/03 §6). The only module besides `mcp::connector` that
//! imports rmcp, so a major version of the SDK touches this directory and nothing else.
//!
//! Version handling is rmcp's `Auto` lifecycle, which is exactly what §6 asks for: probe with
//! `server/discover`, fall back to the `initialize` handshake for the servers that predate it —
//! which, as of the first live probe, is most of them.

use std::{sync::Arc, time::Duration};

use gantry_core::{
    ElicitationAction, ElicitationField, ElicitationFieldKind, ElicitationOption,
    ElicitationRequest, InstanceId, ResultPart, ServerInfo, ToolDef,
};
use rmcp::{
    ClientHandler, ClientServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ClientCapabilities, ClientInfo,
        ContentBlock, ElicitRequestParams, ElicitationAction as McpAction, ElicitationSchema,
        Implementation, InitializeRequestParams, InputRequest, InputResponses, JsonObject,
        PaginatedRequestParams, PrimitiveSchemaDefinition, ProtocolVersion,
    },
    service::{ClientLifecycleMode, NotificationContext, RoleClient, RunningService},
    transport::{
        StreamableHttpClientTransport, TokioChildProcess,
        streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use tokio::process::Command;

use crate::{ToolEventSink, logs::ConnectorLogs, mcp::risk::tier_for};

/// How long a connection may take before we give up and tell the user.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How many times one call may come back asking for more before it is called a loop. rmcp's own
/// default, and for the same reason: a server that has asked ten questions is not converging.
const MAX_ROUNDS: usize = 10;

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
        /// Where this server's stderr goes, and which instance it belongs to (03 §11 step 4).
        /// `None` in a test or a one-shot probe, where nobody will read it.
        log: Option<(ConnectorLogs, InstanceId)>,
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
    service: RunningService<RoleClient, Client>,
    server: ServerInfo,
}

/// What the server said about a tool list, beyond the tools (03 §6, SEP-2549).
#[derive(Debug, Clone, Default)]
pub struct ToolListing {
    pub tools: Vec<ToolDef>,
    /// How long the server says this may be treated as fresh. Absent on every revision before
    /// 2026-07-28, which is most servers today.
    pub ttl: Option<Duration>,
}

/// Gantry as an MCP client.
///
/// It exists for one notification. rmcp's default handler ignores `tools/list_changed`, which is
/// a server saying its tool list is no longer what it told us — the one moment a cache is known
/// to be wrong rather than merely old. The flag is read by `McpConnector`, which owns the cache;
/// a handler cannot refresh it itself, because refreshing needs the session the handler is being
/// called from.
#[derive(Debug, Clone)]
pub struct Client {
    info: ClientInfo,
    tools_changed: Arc<std::sync::atomic::AtomicBool>,
}

impl ClientHandler for Client {
    fn get_info(&self) -> InitializeRequestParams {
        self.info.clone()
    }

    fn on_tool_list_changed(
        &self,
        _context: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        self.tools_changed
            .store(true, std::sync::atomic::Ordering::Relaxed);
        std::future::ready(())
    }
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
    pub async fn connect(
        endpoint: &Endpoint,
        tools_changed: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<Self, McpError> {
        let service = tokio::time::timeout(CONNECT_TIMEOUT, serve(endpoint, tools_changed))
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
    pub async fn tools(&self, remote: bool) -> Result<ToolListing, McpError> {
        // Paginated by hand rather than with `list_all_tools`, which drops the envelope — and
        // the envelope is where `ttlMs` is. The first page's TTL is the listing's: a server that
        // paginates says how long the whole answer is good for, not each slice of it.
        let mut cursor = None;
        let mut ttl = None;
        let mut tools = Vec::new();
        loop {
            let page = self
                .service
                .list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor)))
                .await
                .map_err(|e| McpError::Call(e.to_string()))?;
            if ttl.is_none() {
                ttl = page.ttl_ms.map(Duration::from_millis);
            }
            tools.extend(page.tools);
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        let tools = tools
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
            .collect();
        Ok(ToolListing { tools, ttl })
    }

    /// Calls one tool, walking the MRTR rounds itself (03 §6).
    ///
    /// Not rmcp's `call_tool`, which walks them too — through the session's `ClientHandler`. A
    /// handler is shared by every call the connector makes and is handed no way to tell which one
    /// it is answering for, and an elicitation that reaches the wrong card is worse than none.
    /// Driving the loop here keeps the call, the user and the answer in one scope.
    pub async fn call(
        &self,
        tool: &str,
        args: &serde_json::Value,
        sink: &dyn ToolEventSink,
        request: &ElicitationRequest,
    ) -> Result<(Vec<ResultPart>, Option<serde_json::Value>, bool), McpError> {
        let arguments: Option<JsonObject> = match args {
            serde_json::Value::Object(map) => Some(map.clone()),
            serde_json::Value::Null => None,
            other => Some(JsonObject::from_iter([("value".to_owned(), other.clone())])),
        };
        let mut params = CallToolRequestParams::new(tool.to_owned());
        params.arguments = arguments;
        for _ in 0..MAX_ROUNDS {
            let response = self
                .service
                .peer()
                .call_tool_once(params.clone())
                .await
                .map_err(|e| McpError::Call(e.to_string()))?;
            match response {
                CallToolResponse::Complete(result) => return Ok(convert(result)),
                CallToolResponse::InputRequired(more) => {
                    let mut answers = InputResponses::new();
                    for (key, input) in more.input_requests.unwrap_or_default() {
                        answers.insert(key, answer(&input, sink, request).await?);
                    }
                    params.input_responses = (!answers.is_empty()).then_some(answers);
                    params.request_state = more.request_state;
                }
                // SEP-2663 tasks: a server parking the work and expecting to be polled. Nothing
                // here polls, and pretending otherwise would hang the turn. `CallToolResponse`
                // is non-exhaustive, so anything a later revision adds lands here too — which is
                // the right place for it: an answer this client does not understand is one it
                // must not guess at.
                _ => {
                    return Err(McpError::Call(
                        "the server answered in a way this version of Gantry does not \
                         understand; it may want to run the call as a background task"
                            .to_owned(),
                    ));
                }
            }
        }
        Err(McpError::Call(format!(
            "the server asked for input {MAX_ROUNDS} times without finishing the call"
        )))
    }

    /// Stops the connection: closes the stream, or kills the child process tree.
    pub async fn close(self) {
        if let Err(err) = self.service.cancel().await {
            log::debug!("closing an MCP session: {err}");
        }
    }
}

/// How to open a connection, best first (03 §6). `Auto` probes for the newest revision and
/// falls back inside rmcp; when even that fails, a plain handshake at one older version is
/// tried, because a server that mishandles the probe is not a server Gantry should refuse.
fn lifecycles() -> [ClientLifecycleMode; 2] {
    [
        ClientLifecycleMode::Auto {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28, ProtocolVersion::LATEST],
            legacy_version: Some(ProtocolVersion::V_2025_06_18),
        },
        ClientLifecycleMode::Initialize,
    ]
}

async fn serve(
    endpoint: &Endpoint,
    tools_changed: Arc<std::sync::atomic::AtomicBool>,
) -> Result<RunningService<RoleClient, Client>, McpError> {
    let info = Client {
        info: client_info(),
        tools_changed,
    };
    match endpoint {
        Endpoint::Stdio {
            command,
            args,
            env,
            cwd,
            log,
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
            // Piped rather than inherited, so the server's own explanation of its failure ends
            // up somewhere the app can show it. Inherited, it goes to Gantry's terminal — which
            // in a packaged build is nowhere.
            let (transport, stderr) = TokioChildProcess::builder(cmd)
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|source| McpError::Spawn {
                    command: command.clone(),
                    source,
                })?;
            if let (Some((logs, id)), Some(stderr)) = (log.clone(), stderr) {
                logs.clear(id);
                logs.drain(id, stderr);
            }
            // A process cannot be handed to a second attempt, so it gets the one lifecycle that
            // already falls back internally.
            info.serve_with_lifecycle(transport, lifecycles().into_iter().next().expect("auto"))
                .await
                .map_err(|e| McpError::Connect(e.to_string()))
        }
        Endpoint::Http {
            url,
            headers,
            bearer,
        } => {
            let mut config = StreamableHttpClientTransportConfig::with_uri(url.clone());
            // rmcp's `auth_header` is the *token*: it calls `bearer_auth`, which writes the
            // scheme itself. Handing it a full header value sends `Bearer Bearer …` and every
            // authenticated server answers 401. A credential with another scheme cannot go
            // through that field at all, so it travels as an ordinary header.
            let mut headers = headers.clone();
            if let Some(value) = bearer {
                match bearer_token(value) {
                    Some(token) => config.auth_header = Some(token.to_owned()),
                    None => headers.push(("Authorization".to_owned(), value.clone())),
                }
            }
            let client = reqwest::Client::builder()
                .default_headers(header_map(&headers))
                .build()
                .map_err(|e| McpError::Connect(e.to_string()))?;
            let mut last = String::new();
            for lifecycle in lifecycles() {
                let transport =
                    StreamableHttpClientTransport::with_client(client.clone(), config.clone());
                match info
                    .clone()
                    .serve_with_lifecycle(transport, lifecycle)
                    .await
                {
                    Ok(service) => return Ok(service),
                    Err(err) => last = err.to_string(),
                }
            }
            // rmcp reports "discover and legacy initialize both failed" and keeps the server's
            // own answer to itself, which is the one thing worth reading. So ask the server one
            // plain question and put its reply in the error.
            Err(explain(&client, url, bearer.as_deref(), &last).await)
        }
    }
}

/// Asks the server to initialize, in the open, and turns its answer into something a person can
/// act on. A 401 is not a connection failure but the start of the OAuth flow (03 §7).
async fn explain(
    client: &reqwest::Client,
    url: &str,
    bearer: Option<&str>,
    original: &str,
) -> McpError {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "Gantry", "version": env!("CARGO_PKG_VERSION") },
        },
    });
    // The credential rides on rmcp's transport, not on this client, so it goes on by hand —
    // otherwise every diagnosis would read "401" and blame the token that is working fine.
    let mut request = client.post(url).header(
        reqwest::header::ACCEPT,
        "application/json, text/event-stream",
    );
    if let Some(bearer) = bearer {
        request = request.header(reqwest::header::AUTHORIZATION, bearer);
    }
    let response = request.json(&body).send().await;
    let Ok(response) = response else {
        return McpError::Connect(original.to_owned());
    };
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return McpError::Unauthorized;
    }
    let text = response.text().await.unwrap_or_default();
    let detail: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let detail: String = detail.chars().take(300).collect();
    if status.is_success() && detail.contains("\"error\"") {
        // The transport is fine and the server is refusing on its own terms.
        return McpError::Connect(format!("the server answered: {detail}"));
    }
    if status.is_success() {
        return McpError::Connect(format!(
            "{original} (a plain initialize did work: {detail})"
        ));
    }
    McpError::Connect(format!("the server answered {status}: {detail}"))
}

/// The token out of an `Authorization` value, when the scheme is Bearer.
fn bearer_token(value: &str) -> Option<&str> {
    let rest = value.strip_prefix("Bearer ").or_else(|| {
        value
            .get(..7)
            .filter(|p| p.eq_ignore_ascii_case("bearer "))
            .map(|_| &value[7..])
    })?;
    let token = rest.trim();
    (!token.is_empty()).then_some(token)
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

/// One server-initiated request, answered.
///
/// Only elicitation is answered by a person. Sampling asks Gantry to run *its* model on the
/// server's prompt, and roots asks which folders it may see — both are capabilities Gantry does
/// not offer, and the honest answer is the specification's own "no", not silence.
async fn answer(
    input: &InputRequest,
    sink: &dyn ToolEventSink,
    request: &ElicitationRequest,
) -> Result<serde_json::Value, McpError> {
    let InputRequest::Elicitation(elicit) = input else {
        return Ok(serde_json::json!({ "action": "decline" }));
    };
    let (message, schema) = match &elicit.params {
        ElicitRequestParams::FormElicitationParams {
            message,
            requested_schema,
            ..
        } => (message.clone(), Some(requested_schema)),
        // A URL elicitation sends the user to a web page to finish something. Gantry has no way
        // to know when they have, so declining is the true answer rather than a wait with no end.
        ElicitRequestParams::UrlElicitationParams { message, .. } => (message.clone(), None),
        _ => (String::new(), None),
    };
    let Some(schema) = schema else {
        return Ok(serde_json::json!({ "action": "decline" }));
    };
    let answer = sink
        .elicit(ElicitationRequest {
            message,
            fields: fields_of(schema),
            ..request.clone()
        })
        .await;
    let action = match answer.action {
        ElicitationAction::Accept => McpAction::Accept,
        ElicitationAction::Decline => McpAction::Decline,
        ElicitationAction::Cancel => McpAction::Cancel,
    };
    let mut value = serde_json::json!({ "action": action });
    if answer.action == ElicitationAction::Accept
        && let Some(object) = value.as_object_mut()
    {
        object.insert("content".to_owned(), answer.values);
    }
    Ok(value)
}

/// An elicitation schema as the card's form. The specification allows primitives and nothing
/// nested, which is why this is a flat list rather than a schema renderer.
fn fields_of(schema: &ElicitationSchema) -> Vec<ElicitationField> {
    let required = |key: &str| schema.required.iter().flatten().any(|r| r == key);
    let mut fields = Vec::new();
    for (key, property) in &schema.properties {
        let raw = serde_json::to_value(property).unwrap_or(serde_json::Value::Null);
        let text = |name: &str| {
            raw.get(name)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        };
        let options: Vec<ElicitationOption> = raw
            .get("enum")
            .and_then(serde_json::Value::as_array)
            .map(|values| {
                let names = raw.get("enumNames").and_then(serde_json::Value::as_array);
                values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let value = v.as_str().unwrap_or_default().to_owned();
                        let label = names
                            .and_then(|n| n.get(i))
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(&value)
                            .to_owned();
                        ElicitationOption { value, label }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let kind = if !options.is_empty() {
            ElicitationFieldKind::Enum
        } else {
            match property {
                PrimitiveSchemaDefinition::Boolean(_) => ElicitationFieldKind::Boolean,
                PrimitiveSchemaDefinition::Number(_) => ElicitationFieldKind::Number,
                PrimitiveSchemaDefinition::Integer(_) => ElicitationFieldKind::Integer,
                _ => ElicitationFieldKind::String,
            }
        };
        fields.push(ElicitationField {
            key: key.clone(),
            kind,
            // A server that gave no title gets the key, which is at least the truth.
            title: text("title").unwrap_or_else(|| key.clone()),
            description: text("description"),
            required: required(key),
            options,
            format: text("format"),
        });
    }
    fields
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

#[cfg(test)]
mod tests {
    use super::bearer_token;

    #[test]
    fn a_bearer_value_gives_up_its_token() {
        // rmcp writes the scheme, so what it receives must not carry one (03 §6).
        assert_eq!(bearer_token("Bearer ghu_abc"), Some("ghu_abc"));
        assert_eq!(bearer_token("bearer ghu_abc"), Some("ghu_abc"));
    }

    #[test]
    fn another_scheme_is_left_alone() {
        assert_eq!(bearer_token("token ghp_abc"), None);
        assert_eq!(bearer_token("Bearer "), None);
        assert_eq!(bearer_token(""), None);
    }
}
