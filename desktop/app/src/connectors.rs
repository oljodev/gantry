//! Installing, connecting and authorizing connectors (docs/plan/03 §7, §11).
//!
//! The pieces live in `gantry-connectors`: the catalog, the MCP session, the OAuth flow. This
//! service is what holds them together with the store, the vault and the registry the turn loop
//! reads — and it is the only place that ever sees a decrypted token.

use std::{collections::HashMap, sync::Arc};

use gantry_connectors::{
    ConnectorRegistry,
    auth::{self, AuthError, ClientSource, DeviceStart, discovery, flow},
    catalog::Catalog,
    manifest::Manifest,
    mcp::{Endpoint, McpConnector, McpError},
};
use gantry_core::{
    AuthState, AuthType, ConnectorConfig, ConnectorInstanceDto, ConnectorKind, GantryError,
    InstanceId, ToolInfo,
};
use gantry_secrets::{CredentialKind, OwnerKind, SecretVault};
use gantry_store::{Store, repos};
use serde::{Deserialize, Serialize};

/// A stored OAuth result (06 §5: one credential per authorization, issuer included so a token
/// is never replayed against a server that did not issue it).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Absent when the server named no lifetime.
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
    pub issuer: String,
    #[serde(default)]
    pub client_id: String,
    /// The `Authorization` value as it goes on the wire, scheme included.
    pub header: String,
}

impl StoredToken {
    /// Whether the access token is close enough to expiry to be refreshed first (03 §7 step 4).
    /// A token with no stated lifetime is never stale; a server that wanted otherwise would
    /// have said so.
    #[must_use]
    pub fn stale(&self) -> bool {
        self.expires_at
            .is_some_and(|at| at - gantry_core::now_ms() < 60_000)
    }
}

pub struct ConnectorService {
    pub catalog: Catalog,
    store: Arc<Store>,
    secrets: Arc<SecretVault>,
    registry: Arc<ConnectorRegistry>,
    http: reqwest::Client,
}

impl ConnectorService {
    #[must_use]
    pub fn new(
        store: Arc<Store>,
        secrets: Arc<SecretVault>,
        registry: Arc<ConnectorRegistry>,
    ) -> Self {
        Self {
            catalog: Catalog::embedded(),
            store,
            secrets,
            registry,
            http: reqwest::Client::builder()
                .user_agent(concat!("Gantry/", env!("CARGO_PKG_VERSION")))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn instances(&self) -> Result<Vec<ConnectorInstanceDto>, GantryError> {
        Ok(self.store.read(repos::connectors::list)?)
    }

    pub fn instance(&self, id: InstanceId) -> Result<ConnectorInstanceDto, GantryError> {
        self.store
            .read(move |c| repos::connectors::get(c, id))?
            .ok_or_else(|| GantryError::not_found(format!("connector {id}")))
    }

    /// The catalog with each entry's installed instances filled in.
    pub fn browse(&self) -> Result<Vec<gantry_core::CatalogEntryDto>, GantryError> {
        let installed: Vec<(String, InstanceId)> = self
            .instances()?
            .into_iter()
            .filter_map(|i| i.catalog_id.map(|c| (c, i.id)))
            .collect();
        Ok(self.catalog.list(&installed))
    }

    /// Installs a catalog entry. Nothing is connected yet: a server that needs authorization is
    /// left `Unconfigured` with a Connect button, exactly as §11 requires.
    pub async fn install(&self, catalog_id: &str) -> Result<InstanceId, GantryError> {
        let manifest = self
            .catalog
            .get(catalog_id)
            .ok_or_else(|| GantryError::not_found(format!("catalog entry {catalog_id}")))?;
        let taken = self.store.read(repos::connectors::namespaces)?;
        if !manifest.multi_instance && taken.iter().any(|n| n == &manifest.id) {
            return Err(GantryError::invalid(format!(
                "{} is already installed",
                manifest.name
            )));
        }
        let auth = manifest.auth.kind();
        let new = repos::connectors::NewInstance {
            id: InstanceId::new(),
            catalog_id: Some(manifest.id.clone()),
            namespace: free_namespace(&manifest.id, &taken),
            display_name: manifest.name.clone(),
            config: manifest.config(),
            auth,
            auth_state: if auth == AuthType::None {
                AuthState::Authorized
            } else {
                AuthState::Unconfigured
            },
        };
        let id = new.id;
        self.store
            .write(move |c| repos::connectors::insert(c, &new))
            .await?;
        Ok(id)
    }

    /// Installs a server the user described by hand (03 §8).
    pub async fn install_custom(
        &self,
        name: &str,
        config: ConnectorConfig,
    ) -> Result<InstanceId, GantryError> {
        let taken = self.store.read(repos::connectors::namespaces)?;
        let new = repos::connectors::NewInstance {
            id: InstanceId::new(),
            catalog_id: None,
            namespace: free_namespace(&slug(name), &taken),
            display_name: name.to_owned(),
            config,
            auth: AuthType::None,
            auth_state: AuthState::Authorized,
        };
        let id = new.id;
        self.store
            .write(move |c| repos::connectors::insert(c, &new))
            .await?;
        Ok(id)
    }

    /// Stores a token or key for an instance and marks it ready to connect.
    pub async fn set_token(&self, id: InstanceId, token: &str) -> Result<(), GantryError> {
        let token = token.trim();
        if token.is_empty() {
            return Err(GantryError::invalid("the token is empty"));
        }
        let secret = self
            .secrets
            .set(
                OwnerKind::Instance,
                &id.to_string(),
                CredentialKind::ApiKey,
                Some("token"),
                token,
            )
            .await?;
        let credential = secret.id.clone();
        self.store
            .write(move |c| {
                repos::connectors::set_auth_state(c, id, AuthState::Authorized, Some(&credential))
            })
            .await?;
        Ok(())
    }

    /// Removes an instance, its credentials and its registered OAuth clients (03 §11).
    pub async fn remove(&self, id: InstanceId) -> Result<(), GantryError> {
        let instance = self.instance(id)?;
        self.registry.remove(&instance.namespace);
        for secret in self
            .secrets
            .list_for_owner(OwnerKind::Instance, &id.to_string())?
        {
            self.secrets.delete(&secret.id).await?;
        }
        self.store
            .write(move |c| repos::connectors::remove(c, id))
            .await?;
        Ok(())
    }

    pub async fn set_enabled(&self, id: InstanceId, enabled: bool) -> Result<(), GantryError> {
        self.store
            .write(move |c| repos::connectors::set_enabled(c, id, enabled))
            .await?;
        self.rebuild().await
    }

    /// Connects, lists the tools and caches them. This is what "Install" ends with, what
    /// "Reconnect" does, and what the detail page's refresh calls.
    pub async fn connect(&self, id: InstanceId) -> Result<Vec<ToolInfo>, GantryError> {
        let instance = self.instance(id)?;
        let endpoint = self.endpoint(&instance).await?;
        let remote = instance.kind == ConnectorKind::McpRemote;
        match gantry_connectors::mcp::McpSession::connect(&endpoint).await {
            Ok(session) => {
                let defs = session
                    .tools(remote)
                    .await
                    .map_err(|e| GantryError::internal(e.to_string()))?;
                let manifest = instance
                    .catalog_id
                    .as_deref()
                    .and_then(|c| self.catalog.get(c));
                let tools = tool_infos(&defs, manifest.as_deref());
                let server = session.server().clone();
                session.close().await;
                let (t, d, s) = (
                    tools.clone(),
                    with_overrides(&defs, manifest.as_deref()),
                    server,
                );
                self.store
                    .write(move |c| {
                        repos::connectors::record_connection(c, id, &t, &d, Some(&s), None)
                    })
                    .await?;
                self.rebuild().await?;
                Ok(tools)
            }
            Err(McpError::Unauthorized) => {
                self.store
                    .write(move |c| {
                        repos::connectors::set_auth_state(c, id, AuthState::Unconfigured, None)
                    })
                    .await?;
                Err(GantryError::invalid(
                    "the server refused the connection: sign in or add a token first",
                ))
            }
            Err(err) => {
                let message = err.to_string();
                let recorded = message.clone();
                self.store
                    .write(move |c| {
                        repos::connectors::record_connection(c, id, &[], &[], None, Some(&recorded))
                    })
                    .await?;
                Err(GantryError::internal(message))
            }
        }
    }

    /// Starts the OAuth flow and returns the URL the browser must open, plus everything needed
    /// to finish. Discovery happens here so a server that cannot be authorized says so before a
    /// browser window appears.
    pub async fn begin_authorization(
        &self,
        id: InstanceId,
        client_id_override: Option<String>,
    ) -> Result<Authorization, GantryError> {
        let instance = self.instance(id)?;
        let ConnectorConfig::McpRemote { url, .. } = &instance.config else {
            return Err(GantryError::invalid(
                "only a remote server signs in through the browser",
            ));
        };
        let manifest = instance
            .catalog_id
            .as_deref()
            .and_then(|c| self.catalog.get(c));

        // The challenge names the document; without one, derive it from the URL (03 §7 step 1).
        let metadata_url = self
            .challenge_metadata_url(url)
            .await
            .or_else(|| discovery::protected_resource_url(url))
            .ok_or_else(|| GantryError::invalid("this server publishes no OAuth metadata"))?;
        let resource = discovery::protected_resource(&self.http, &metadata_url)
            .await
            .map_err(auth_error)?;
        let issuer = resource
            .authorization_servers
            .first()
            .cloned()
            .ok_or_else(|| GantryError::invalid("the server names no authorization server"))?;
        let server = discovery::auth_server(&self.http, &issuer)
            .await
            .map_err(auth_error)?;

        let stored = self
            .store
            .read({
                let issuer = issuer.clone();
                move |c| repos::connectors::oauth_client(c, id, &issuer)
            })?
            .map(|c| c.client_id);
        let manifest_client = manifest
            .as_ref()
            .and_then(|m| m.auth.client_id().map(str::to_owned));
        let client_id = match client_id_override.clone().or(stored).or(manifest_client) {
            Some(id) => id,
            None => match auth::choose_client(None, None, &server).map_err(auth_error)? {
                ClientSource::Preregistered(id) => id,
                ClientSource::Cimd => auth::CIMD_URL.to_owned(),
                ClientSource::Dynamic => {
                    let endpoint = server
                        .registration_endpoint
                        .clone()
                        .expect("dynamic registration implies an endpoint");
                    let scopes = scopes_for(manifest.as_deref(), &resource, &server);
                    let registered =
                        discovery::register(&self.http, &endpoint, &auth::redirect_uris(), &scopes)
                            .await
                            .map_err(auth_error)?;
                    let record = repos::connectors::OauthClient {
                        id: ulid::Ulid::new().to_string(),
                        instance_id: id,
                        issuer: issuer.clone(),
                        client_id: registered.client_id.clone(),
                        registration_json: None,
                    };
                    self.store
                        .write(move |c| repos::connectors::put_oauth_client(c, &record))
                        .await?;
                    registered.client_id
                }
            },
        };
        // A user-supplied id is worth keeping: the next Reconnect should not ask again.
        if client_id_override.is_some() {
            let record = repos::connectors::OauthClient {
                id: ulid::Ulid::new().to_string(),
                instance_id: id,
                issuer: issuer.clone(),
                client_id: client_id.clone(),
                registration_json: None,
            };
            self.store
                .write(move |c| repos::connectors::put_oauth_client(c, &record))
                .await?;
        }

        let scopes = scopes_for(manifest.as_deref(), &resource, &server);
        let (url, user_code, step) = if auth::prefers_device(&server) {
            let start = flow::device_begin(&self.http, &server, &client_id, &scopes)
                .await
                .map_err(auth_error)?;
            (
                start
                    .verification_uri_complete
                    .clone()
                    .unwrap_or_else(|| start.verification_uri.clone()),
                Some(start.user_code.clone()),
                Step::Device(Box::new(start)),
            )
        } else {
            let pending = flow::begin(&server, &client_id, &scopes, Some(&resource.resource))
                .await
                .map_err(auth_error)?;
            (
                pending.authorize_url.clone(),
                None,
                Step::Redirect(Box::new(pending)),
            )
        };
        self.store
            .write(move |c| repos::connectors::set_auth_state(c, id, AuthState::Authorizing, None))
            .await?;
        Ok(Authorization {
            url,
            user_code,
            step,
            server,
            client_id,
            resource: resource.resource,
            issuer,
        })
    }

    /// Waits for the browser, redeems the code and stores the token set.
    pub async fn finish_authorization(
        &self,
        id: InstanceId,
        authorization: Authorization,
    ) -> Result<(), GantryError> {
        let Authorization {
            step,
            server,
            client_id,
            resource,
            issuer,
            ..
        } = authorization;
        let result = match step {
            Step::Redirect(pending) => {
                pending
                    .complete(&self.http, &server, &client_id, Some(&resource))
                    .await
            }
            Step::Device(start) => flow::device_wait(&self.http, &server, &client_id, &start).await,
        };
        match result {
            Ok(tokens) => {
                let stored = StoredToken {
                    expires_at: tokens.expires_at(),
                    header: tokens.header(),
                    access_token: tokens.access_token,
                    refresh_token: tokens.refresh_token,
                    scope: tokens.scope,
                    issuer,
                    client_id,
                };
                self.store_token(id, &stored).await?;
                Ok(())
            }
            Err(err) => {
                self.store
                    .write(move |c| {
                        repos::connectors::set_auth_state(c, id, AuthState::Error, None)
                    })
                    .await?;
                Err(auth_error(err))
            }
        }
    }

    async fn store_token(&self, id: InstanceId, token: &StoredToken) -> Result<(), GantryError> {
        let json = serde_json::to_string(token)
            .map_err(|e| GantryError::internal(format!("serializing the token: {e}")))?;
        let secret = self
            .secrets
            .set(
                OwnerKind::Instance,
                &id.to_string(),
                CredentialKind::OauthToken,
                Some(&token.issuer),
                &json,
            )
            .await?;
        let credential = secret.id.clone();
        self.store
            .write(move |c| {
                repos::connectors::set_auth_state(c, id, AuthState::Authorized, Some(&credential))
            })
            .await?;
        Ok(())
    }

    /// Reads the instance's credential and turns the configuration into something connectable,
    /// refreshing an expiring token on the way (03 §7 step 4).
    async fn endpoint(&self, instance: &ConnectorInstanceDto) -> Result<Endpoint, GantryError> {
        match &instance.config {
            ConnectorConfig::Native => Err(GantryError::invalid(
                "a native connector does not connect over MCP",
            )),
            ConnectorConfig::McpStdio {
                command,
                args,
                env,
                cwd,
                ..
            } => Ok(Endpoint::Stdio {
                command: command.clone(),
                args: args.clone(),
                env: env.clone(),
                cwd: cwd.clone(),
            }),
            ConnectorConfig::McpRemote { url, headers, .. } => {
                let bearer = self.authorization_header(instance).await?;
                Ok(Endpoint::Http {
                    url: url.clone(),
                    headers: headers.clone(),
                    bearer,
                })
            }
        }
    }

    /// The `Authorization` value for this instance, if it has a credential at all.
    async fn authorization_header(
        &self,
        instance: &ConnectorInstanceDto,
    ) -> Result<Option<String>, GantryError> {
        let id = instance.id;
        let Some(credential) = self
            .store
            .read(move |c| repos::connectors::credential_id(c, id))?
        else {
            return Ok(None);
        };
        let secret = self.secrets.get(&credential)?;
        use secrecy::ExposeSecret;
        let raw = secret.expose_secret();
        // A pasted token is stored as itself; an OAuth result is stored as JSON.
        let Ok(mut token) = serde_json::from_str::<StoredToken>(raw) else {
            return Ok(Some(format!("Bearer {}", raw.trim())));
        };
        if token.stale()
            && let Some(refresh) = token.refresh_token.clone()
        {
            match self.refresh(&mut token, &refresh).await {
                Ok(()) => self.store_token(id, &token).await?,
                Err(err) => {
                    log::warn!("refreshing {}: {err}", instance.name);
                    self.store
                        .write(move |c| {
                            repos::connectors::set_auth_state(c, id, AuthState::Expired, None)
                        })
                        .await?;
                }
            }
        }
        Ok(Some(token.header.clone()))
    }

    async fn refresh(&self, token: &mut StoredToken, refresh: &str) -> Result<(), GantryError> {
        let server = discovery::auth_server(&self.http, &token.issuer)
            .await
            .map_err(auth_error)?;
        let fresh = flow::refresh(&self.http, &server, &token.client_id, refresh, None)
            .await
            .map_err(auth_error)?;
        token.expires_at = fresh.expires_at();
        token.header = fresh.header();
        token.access_token = fresh.access_token;
        if fresh.refresh_token.is_some() {
            token.refresh_token = fresh.refresh_token;
        }
        Ok(())
    }

    /// The `resource_metadata` URL from the server's own 401, which is the authoritative
    /// pointer (RFC 9728). A server that answers something else gets the derived URL instead.
    async fn challenge_metadata_url(&self, url: &str) -> Option<String> {
        let response = self
            .http
            .post(url)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}))
            .send()
            .await
            .ok()?;
        let challenge = response
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)?
            .to_str()
            .ok()?;
        discovery::resource_metadata_url(challenge)
    }

    /// Rebuilds the registry from the database: every enabled instance that has what it needs
    /// becomes a connector the turn loop can call.
    pub async fn rebuild(&self) -> Result<(), GantryError> {
        let instances = self.instances()?;
        let mut endpoints: HashMap<String, (ConnectorInstanceDto, Endpoint)> = HashMap::new();
        for instance in instances {
            if !instance.enabled || !instance.auth_state.usable() {
                self.registry.remove(&instance.namespace);
                continue;
            }
            match self.endpoint(&instance).await {
                Ok(endpoint) => {
                    endpoints.insert(instance.namespace.clone(), (instance, endpoint));
                }
                Err(err) => log::warn!("{} is not connectable: {err}", instance.name),
            }
        }
        for (namespace, (instance, endpoint)) in endpoints {
            let manifest = instance
                .catalog_id
                .as_deref()
                .and_then(|c| self.catalog.get(c));
            // The stored definitions, schemas and all. An instance that has none — never
            // connected, or last connected before the schemas were kept — starts with no cache,
            // so its first use lists the tools rather than declaring them without arguments.
            let id = instance.id;
            let tools = self
                .store
                .read(move |c| repos::connectors::tool_defs(c, id))
                .unwrap_or_else(|err| {
                    log::warn!("{} has no usable tool cache: {err}", instance.name);
                    None
                })
                .map(|defs| with_overrides(&defs, manifest.as_deref()));
            self.registry.register(Arc::new(McpConnector::new(
                namespace,
                instance.name.clone(),
                instance.id,
                instance.kind,
                endpoint,
                tools,
            )));
        }
        Ok(())
    }
}

/// An authorization in flight. Which half of it depends on what the server accepts (03 §7):
/// a redirect to a loopback port, or a code the user types on the server's own page.
pub struct Authorization {
    /// The page to open in the browser.
    pub url: String,
    /// The code to type there, when the server asked for one.
    pub user_code: Option<String>,
    step: Step,
    server: discovery::AuthServer,
    client_id: String,
    resource: String,
    issuer: String,
}

enum Step {
    Redirect(Box<flow::Pending>),
    Device(Box<DeviceStart>),
}

/// The scopes to ask for: the manifest's list, else what the resource says it supports.
fn scopes_for(
    manifest: Option<&Manifest>,
    resource: &discovery::ProtectedResource,
    server: &discovery::AuthServer,
) -> Vec<String> {
    let from_manifest = manifest.map(|m| m.auth.scopes()).unwrap_or_default();
    if !from_manifest.is_empty() {
        return from_manifest;
    }
    if !resource.scopes_supported.is_empty() {
        return resource.scopes_supported.clone();
    }
    server.scopes_supported.clone()
}

/// The tools with the manifest's tier and confirmation overrides applied (03 §3).
fn with_overrides(
    defs: &[gantry_core::ToolDef],
    manifest: Option<&Manifest>,
) -> Vec<gantry_core::ToolDef> {
    defs.iter()
        .map(|def| {
            let mut def = def.clone();
            apply_override(&mut def, manifest);
            def
        })
        .collect()
}

/// The discovered tools as the UI lists them, with the manifest's tier overrides applied.
fn tool_infos(defs: &[gantry_core::ToolDef], manifest: Option<&Manifest>) -> Vec<ToolInfo> {
    with_overrides(defs, manifest)
        .into_iter()
        .map(|def| ToolInfo {
            name: def.name,
            title: None,
            description: def.description,
            tier: def.tier,
        })
        .collect()
}

/// A manifest may correct what a server claims about one of its tools (03 §6).
fn apply_override(def: &mut gantry_core::ToolDef, manifest: Option<&Manifest>) {
    let Some(over) = manifest.and_then(|m| m.tool_overrides.get(&def.name)) else {
        return;
    };
    if let Some(tier) = over.risk {
        def.tier = tier;
    }
    if let Some(always) = over.always_confirm {
        def.always_confirm = always;
    }
    if let Some(parallel) = over.parallel_safe {
        def.parallel_safe = parallel;
    }
}

/// A namespace nobody else is using: `github`, then `github-2`.
fn free_namespace(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken.iter().any(|t| t == candidate))
        .unwrap_or_else(|| format!("{base}-{}", ulid::Ulid::new()))
}

/// A display name as a tool-namespace prefix.
fn slug(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').replace("--", "-");
    if slug.is_empty() {
        "server".to_owned()
    } else {
        slug.chars().take(40).collect()
    }
}

fn auth_error(err: AuthError) -> GantryError {
    match err {
        AuthError::NeedsClientId => GantryError::invalid(err.to_string()),
        AuthError::Denied(_) | AuthError::Timeout => GantryError::invalid(err.to_string()),
        other => GantryError::internal(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_instance_gets_its_own_namespace() {
        let taken = vec!["github".to_owned(), "github-2".to_owned()];
        assert_eq!(free_namespace("github", &taken), "github-3");
        assert_eq!(free_namespace("linear", &taken), "linear");
    }

    #[test]
    fn a_display_name_becomes_a_usable_prefix() {
        assert_eq!(slug("My Server!"), "my-server");
        assert_eq!(slug("  "), "server");
    }
}
