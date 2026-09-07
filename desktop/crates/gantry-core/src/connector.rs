//! Connectors as the app sees them (docs/plan/03 §3, §7, 06 §3): the catalog entry a user
//! browses, the installed instance a chat attaches, and the small facts about a connection —
//! what it runs, how it authenticates, which tools it discovered.
//!
//! Nothing here is secret. Configuration carries the *names* of environment variables and
//! headers, never their values; values live in the vault (06 §5).

use serde::{Deserialize, Serialize};

use crate::{ids::InstanceId, tool::RiskTier};

/// How a connector runs (03 §3, `runtime.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectorKind {
    /// Compiled into Gantry.
    Native,
    /// A local process speaking MCP over stdio.
    McpStdio,
    /// An HTTP server speaking MCP.
    McpRemote,
}

impl ConnectorKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectorKind::Native => "native",
            ConnectorKind::McpStdio => "mcp-stdio",
            ConnectorKind::McpRemote => "mcp-remote",
        }
    }
}

/// How a connector proves who it is (03 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    None,
    ApiKey,
    Headers,
    Oauth2,
}

impl AuthType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AuthType::None => "none",
            AuthType::ApiKey => "api_key",
            AuthType::Headers => "headers",
            AuthType::Oauth2 => "oauth2",
        }
    }
}

/// Where an instance stands in the auth state machine (03 §7). A server that needs nothing is
/// `Authorized` from the moment it is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    /// Installed, but the key or the connection is still missing.
    Unconfigured,
    /// Configured and ready to authorize.
    Configured,
    /// The browser is open and we are waiting for the redirect.
    Authorizing,
    Authorized,
    /// The refresh token no longer works: "Reconnect".
    Expired,
    Revoked,
    Error,
}

impl AuthState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            AuthState::Unconfigured => "unconfigured",
            AuthState::Configured => "configured",
            AuthState::Authorizing => "authorizing",
            AuthState::Authorized => "authorized",
            AuthState::Expired => "expired",
            AuthState::Revoked => "revoked",
            AuthState::Error => "error",
        }
    }

    /// Whether a connection may be attempted at all.
    #[must_use]
    pub fn usable(self) -> bool {
        matches!(self, AuthState::Authorized | AuthState::Configured)
    }
}

/// What an instance runs or talks to. Secret values are never in here (06 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ConnectorConfig {
    Native,
    McpStdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        /// Plain environment variables, and the *names* of the ones filled from the vault.
        #[serde(default)]
        env: Vec<(String, String)>,
        #[serde(default)]
        secret_env: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
    },
    McpRemote {
        url: String,
        #[serde(default)]
        headers: Vec<(String, String)>,
        /// Header names whose value comes from the vault, e.g. `Authorization`.
        #[serde(default)]
        secret_headers: Vec<String>,
    },
}

impl ConnectorConfig {
    #[must_use]
    pub fn kind(&self) -> ConnectorKind {
        match self {
            ConnectorConfig::Native => ConnectorKind::Native,
            ConnectorConfig::McpStdio { .. } => ConnectorKind::McpStdio,
            ConnectorConfig::McpRemote { .. } => ConnectorKind::McpRemote,
        }
    }

    /// What the install dialog shows before anything runs (03 §11): the exact command, or the
    /// URL. Never a secret — only the names of the values that will be injected.
    #[must_use]
    pub fn preview(&self) -> String {
        match self {
            ConnectorConfig::Native => "built into Gantry".to_owned(),
            ConnectorConfig::McpStdio { command, args, .. } => {
                let mut line = command.clone();
                for arg in args {
                    line.push(' ');
                    line.push_str(arg);
                }
                line
            }
            ConnectorConfig::McpRemote { url, .. } => url.clone(),
        }
    }
}

/// One tool an instance offers, as the UI lists it. The schema is not carried here: the browse
/// list wants names and tiers, and the model gets the schema from the live session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ToolInfo {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub tier: RiskTier,
}

/// What the server said about itself on the last connection (06 §3, `server_info_json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    /// The negotiated MCP revision, shown on the detail page (03 §6).
    pub protocol: String,
}

/// An installed connector, as Settings → Customize and the composer show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ConnectorInstanceDto {
    pub id: InstanceId,
    /// The catalog entry it came from; `None` for a server the user added by hand.
    pub catalog_id: Option<String>,
    /// The tool namespace prefix, unique across installed instances.
    pub namespace: String,
    pub name: String,
    pub kind: ConnectorKind,
    pub config: ConnectorConfig,
    pub auth: AuthType,
    pub auth_state: AuthState,
    pub enabled: bool,
    pub tools: Vec<ToolInfo>,
    pub server: Option<ServerInfo>,
    pub last_error: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub installed_at: i64,
    #[specta(type = Option<specta_typescript::Number>)]
    pub last_connected_at: Option<i64>,
}

/// A runtime an entry needs before it can be installed (03 §11 step 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct RuntimeRequirement {
    /// `node`, `python`, `uv` or `docker`.
    pub name: String,
    /// The version range asked for, as written in the manifest.
    pub version: String,
}

/// A catalog entry as the Discover list shows it (03 §10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CatalogEntryDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub kind: ConnectorKind,
    pub auth: AuthType,
    pub first_party: bool,
    pub keywords: Vec<String>,
    pub homepage: Option<String>,
    /// What the entry will run or connect to, for the install dialog's preview.
    pub preview: String,
    pub requires: Vec<RuntimeRequirement>,
    /// Instances already installed from this entry.
    pub installed: Vec<InstanceId>,
    /// Whether a second instance may be installed (03 §3, `multi_instance`).
    pub multi_instance: bool,
    /// The manifest's `long_description`, for the detail view.
    pub long_description: Option<String>,
    pub auth_instructions: Option<String>,
    /// The page where the credential is created, opened by a button rather than described in
    /// prose the user has to follow by hand.
    pub auth_setup_url: Option<String>,
    /// A second accepted credential, when the server takes one (03 §7): GitHub signs in with
    /// OAuth or takes a token, and the install dialog offers both.
    pub auth_alternate: Option<AuthType>,
    pub auth_alternate_instructions: Option<String>,
    pub auth_alternate_setup_url: Option<String>,
}
