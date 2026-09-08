//! `connector_instances`, `oauth_clients` and `chat_connectors` (docs/plan/06 §3): what is
//! installed, what it is allowed to talk to, and which chats may use it. Configuration only —
//! the secrets it needs are rows in `credentials`.

use gantry_core::{
    AuthState, AuthType, ChatId, ConnectorConfig, ConnectorInstanceDto, ConnectorKind, InstanceId,
    ServerInfo, ToolDef, ToolInfo, now_ms,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::{Result, StoreError},
    repos::{enum_from_str, id_from_str},
};

const COLUMNS: &str = "id, catalog_id, namespace, display_name, kind, config_json, auth_type, \
                       auth_state, enabled, tools_cache_json, server_info_json, installed_at, \
                       last_connected_at, last_error";

/// What an install writes. The id and timestamps are the repository's business.
#[derive(Debug, Clone)]
pub struct NewInstance {
    pub id: InstanceId,
    pub catalog_id: Option<String>,
    pub namespace: String,
    pub display_name: String,
    pub config: ConnectorConfig,
    pub auth: AuthType,
    pub auth_state: AuthState,
}

pub fn insert(conn: &Connection, new: &NewInstance) -> Result<()> {
    let now = now_ms();
    conn.execute(
        "INSERT INTO connector_instances (id, catalog_id, namespace, display_name, kind, \
         config_json, user_config_json, auth_type, auth_state, enabled, installed_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, '{}', ?7, ?8, 1, ?9, ?9)",
        params![
            new.id.to_string(),
            new.catalog_id,
            new.namespace,
            new.display_name,
            new.config.kind().as_str(),
            json(&new.config)?,
            new.auth.as_str(),
            new.auth_state.as_str(),
            now,
        ],
    )?;
    Ok(())
}

/// Every installed instance, oldest first, so the model-facing tool array stays stable (03 §6).
pub fn list(conn: &Connection) -> Result<Vec<ConnectorInstanceDto>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM connector_instances ORDER BY installed_at"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get(conn: &Connection, id: InstanceId) -> Result<Option<ConnectorInstanceDto>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM connector_instances WHERE id = ?1"),
            params![id.to_string()],
            from_row,
        )
        .optional()?)
}

/// The namespaces already taken, so a new install can pick one that is free.
pub fn namespaces(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT namespace FROM connector_instances")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn set_enabled(conn: &Connection, id: InstanceId, enabled: bool) -> Result<()> {
    conn.execute(
        "UPDATE connector_instances SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
        params![id.to_string(), enabled, now_ms()],
    )?;
    Ok(())
}

pub fn set_auth_state(
    conn: &Connection,
    id: InstanceId,
    state: AuthState,
    credential_id: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE connector_instances SET auth_state = ?2, \
         credential_id = COALESCE(?3, credential_id), updated_at = ?4 WHERE id = ?1",
        params![id.to_string(), state.as_str(), credential_id, now_ms()],
    )?;
    Ok(())
}

pub fn credential_id(conn: &Connection, id: InstanceId) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT credential_id FROM connector_instances WHERE id = ?1",
            params![id.to_string()],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

pub fn set_config(conn: &Connection, id: InstanceId, config: &ConnectorConfig) -> Result<()> {
    conn.execute(
        "UPDATE connector_instances SET config_json = ?2, kind = ?3, updated_at = ?4 WHERE id = ?1",
        params![
            id.to_string(),
            json(config)?,
            config.kind().as_str(),
            now_ms()
        ],
    )?;
    Ok(())
}

/// What the last connection discovered: the tool list as the UI shows it, the same tools with
/// their argument schemas as the model needs them, who the server said it was, and the error if
/// it failed. One write, so a half-connected instance is never shown.
pub fn record_connection(
    conn: &Connection,
    id: InstanceId,
    tools: &[ToolInfo],
    defs: &[ToolDef],
    server: Option<&ServerInfo>,
    error: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE connector_instances SET tools_cache_json = ?2, tool_defs_json = ?3, \
         server_info_json = ?4, last_connected_at = ?5, last_error = ?6, updated_at = ?5 \
         WHERE id = ?1",
        params![
            id.to_string(),
            json(&tools)?,
            json(&defs)?,
            server.map(json).transpose()?,
            now_ms(),
            error,
        ],
    )?;
    Ok(())
}

/// The full tool definitions of one instance, schemas and all, for rebuilding the registry
/// without waking the server. `None` when the instance has never connected, or connected
/// before 0007: the connector then lists its tools on its next connection.
pub fn tool_defs(conn: &Connection, id: InstanceId) -> Result<Option<Vec<ToolDef>>> {
    let json: Option<String> = conn
        .query_row(
            "SELECT tool_defs_json FROM connector_instances WHERE id = ?1",
            params![id.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let Some(json) = json else { return Ok(None) };
    let defs: Vec<ToolDef> =
        serde_json::from_str(&json).map_err(|e| StoreError::Other(e.to_string()))?;
    Ok((!defs.is_empty()).then_some(defs))
}

/// Removes the instance and everything that hangs off it. Credentials are the caller's to
/// delete: the vault, not the database, owns them (03 §11).
pub fn remove(conn: &Connection, id: InstanceId) -> Result<()> {
    conn.execute(
        "DELETE FROM connector_instances WHERE id = ?1",
        params![id.to_string()],
    )?;
    Ok(())
}

// ---- OAuth clients -------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OauthClient {
    pub id: String,
    pub instance_id: InstanceId,
    pub issuer: String,
    pub client_id: String,
    pub registration_json: Option<String>,
}

/// The client registered with one issuer, if there is one. Registration is expensive and
/// rate-limited, so it is done once per issuer and kept (03 §7 step 2).
pub fn oauth_client(
    conn: &Connection,
    instance_id: InstanceId,
    issuer: &str,
) -> Result<Option<OauthClient>> {
    Ok(conn
        .query_row(
            "SELECT id, instance_id, issuer, client_id, registration_json FROM oauth_clients \
             WHERE instance_id = ?1 AND issuer = ?2",
            params![instance_id.to_string(), issuer],
            |r| {
                Ok(OauthClient {
                    id: r.get(0)?,
                    instance_id: id_from_str(r, 1)?,
                    issuer: r.get(2)?,
                    client_id: r.get(3)?,
                    registration_json: r.get(4)?,
                })
            },
        )
        .optional()?)
}

pub fn put_oauth_client(conn: &Connection, client: &OauthClient) -> Result<()> {
    conn.execute(
        "INSERT INTO oauth_clients (id, instance_id, issuer, client_id, registration_json, \
         created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (instance_id, issuer) DO UPDATE SET client_id = excluded.client_id, \
         registration_json = excluded.registration_json",
        params![
            client.id,
            client.instance_id.to_string(),
            client.issuer,
            client.client_id,
            client.registration_json,
            now_ms(),
        ],
    )?;
    Ok(())
}

// ---- Attachment ----------------------------------------------------------------------------

/// Attaches a connector to a chat. Attaching twice is not an error; the first source wins.
pub fn attach(
    conn: &Connection,
    chat_id: ChatId,
    instance_id: InstanceId,
    source: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO chat_connectors (chat_id, instance_id, source, attached_at) \
         VALUES (?1, ?2, ?3, ?4) ON CONFLICT (chat_id, instance_id) DO NOTHING",
        params![
            chat_id.to_string(),
            instance_id.to_string(),
            source,
            now_ms()
        ],
    )?;
    Ok(())
}

pub fn detach(conn: &Connection, chat_id: ChatId, instance_id: InstanceId) -> Result<()> {
    conn.execute(
        "DELETE FROM chat_connectors WHERE chat_id = ?1 AND instance_id = ?2",
        params![chat_id.to_string(), instance_id.to_string()],
    )?;
    Ok(())
}

/// The instances one chat may use, in attachment order.
pub fn attached(conn: &Connection, chat_id: ChatId) -> Result<Vec<InstanceId>> {
    let mut stmt = conn.prepare(
        "SELECT instance_id FROM chat_connectors WHERE chat_id = ?1 ORDER BY attached_at",
    )?;
    let rows = stmt.query_map(params![chat_id.to_string()], |r| id_from_str(r, 0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// The tool namespaces one chat may call, for assembling its tool set (03 §11).
pub fn attached_namespaces(conn: &Connection, chat_id: ChatId) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT i.namespace FROM chat_connectors c JOIN connector_instances i \
         ON i.id = c.instance_id WHERE c.chat_id = ?1 AND i.enabled = 1 ORDER BY c.attached_at",
    )?;
    let rows = stmt.query_map(params![chat_id.to_string()], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<ConnectorInstanceDto> {
    let config: ConnectorConfig = from_json(r, 5)?;
    let kind: ConnectorKind = enum_from_str(r, 4)?;
    Ok(ConnectorInstanceDto {
        id: id_from_str(r, 0)?,
        catalog_id: r.get(1)?,
        namespace: r.get(2)?,
        name: r.get(3)?,
        kind,
        config,
        auth: enum_from_str(r, 6)?,
        auth_state: enum_from_str(r, 7)?,
        enabled: r.get(8)?,
        tools: opt_json(r, 9)?.unwrap_or_default(),
        server: opt_json(r, 10)?,
        last_error: r.get(13)?,
        installed_at: r.get(11)?,
        last_connected_at: r.get(12)?,
    })
}

fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|e| StoreError::Other(e.to_string()))
}

fn from_json<T: serde::de::DeserializeOwned>(r: &Row<'_>, idx: usize) -> rusqlite::Result<T> {
    let raw: String = r.get(idx)?;
    serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn opt_json<T: serde::de::DeserializeOwned>(
    r: &Row<'_>,
    idx: usize,
) -> rusqlite::Result<Option<T>> {
    let raw: Option<String> = r.get(idx)?;
    match raw {
        None => Ok(None),
        Some(raw) => serde_json::from_str(&raw).map(Some).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
        }),
    }
}
