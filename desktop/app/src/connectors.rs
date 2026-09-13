//! Installing, connecting and authorizing connectors (docs/plan/03 §7, §11).
//!
//! The pieces live in `gantry-connectors`: the catalog, the MCP session, the OAuth flow. This
//! service is what holds them together with the store, the vault and the registry the turn loop
//! reads — and it is the only place that ever sees a decrypted token.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use gantry_connector_shell::ShellEnv;
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
use gantry_workspace::Workspace;
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
    /// Roots, file IO and the journal, which every native connector shares (03 §5).
    workspace: Arc<Workspace>,
    /// The login shell and its environment, captured once (`docs/connectors/shell.md` D2).
    shell_env: Arc<ShellEnv>,
    /// What each local server wrote to stderr, for the failure that has to explain itself
    /// (03 §11 step 4).
    logs: gantry_connectors::logs::ConnectorLogs,
    http: reqwest::Client,
}

impl ConnectorService {
    #[must_use]
    pub fn new(
        store: Arc<Store>,
        secrets: Arc<SecretVault>,
        registry: Arc<ConnectorRegistry>,
        workspace: Arc<Workspace>,
        shell_env: Arc<ShellEnv>,
    ) -> Self {
        Self {
            catalog: Catalog::embedded(),
            store,
            secrets,
            registry,
            workspace,
            shell_env,
            logs: gantry_connectors::logs::ConnectorLogs::new(),
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
    /// What this connector needs before it can run, and whether this machine has it (03 §11
    /// step 1). Empty for everything that is not a local process.
    pub async fn runtime_check(
        &self,
        catalog_id: &str,
    ) -> Result<Vec<gantry_connectors::runtime::RuntimeStatus>, GantryError> {
        let manifest = self
            .catalog
            .get(catalog_id)
            .ok_or_else(|| GantryError::not_found(format!("catalog entry {catalog_id}")))?;
        Ok(gantry_connectors::runtime::detect(&manifest.requires(), &self.shell_env.vars).await)
    }

    /// The form a connector asks for at install (03 §11 step 2), and what this instance already
    /// answered. Empty for everything that asks for nothing, which is most of the catalogue.
    pub fn user_config_form(
        &self,
        catalog_id: &str,
    ) -> Result<Vec<gantry_core::UserConfigField>, GantryError> {
        Ok(self
            .catalog
            .get(catalog_id)
            .map(|m| m.user_config_fields())
            .unwrap_or_default())
    }

    pub fn user_config(&self, id: InstanceId) -> Result<BTreeMap<String, String>, GantryError> {
        Ok(self
            .store
            .read(move |c| repos::connectors::user_config(c, id))?)
    }

    /// Writes the answers and rebuilds the runtime from them.
    ///
    /// Both, together, because they are one fact seen twice: the answers are what the user typed
    /// and the config is where they end up, and a config rebuilt from stale answers — or answers
    /// saved without rebuilding — is a connector that runs against the host it used to have.
    /// A sensitive answer goes to the vault instead and is injected at call time (06 §5).
    pub async fn set_user_config(
        &self,
        id: InstanceId,
        values: BTreeMap<String, String>,
    ) -> Result<(), GantryError> {
        let instance = self.instance(id)?;
        let Some(manifest) = instance
            .catalog_id
            .as_deref()
            .and_then(|c| self.catalog.get(c))
        else {
            return Err(GantryError::invalid(
                "a server you added by hand has no form to fill in; edit it instead",
            ));
        };
        for field in manifest.user_config_fields() {
            let given = values.get(&field.key).map(|v| v.trim()).unwrap_or("");
            if field.required && given.is_empty() {
                return Err(GantryError::invalid(format!("{} is required", field.title)));
            }
        }
        for field in manifest.user_config_fields().iter().filter(|f| f.sensitive) {
            // Trimmed, like the public answers below. Pasting a key picks up a newline, and a
            // secret stored with it is a credential that fails against the service for a reason
            // nothing in the interface can show — the one kind of value whose whitespace is
            // invisible everywhere it is later read.
            let given = values.get(&field.key).map(|v| v.trim()).unwrap_or("");
            if !given.is_empty() {
                self.secrets
                    .set(
                        OwnerKind::Instance,
                        &id.to_string(),
                        CredentialKind::UserConfigSecret,
                        Some(&field.key),
                        given,
                    )
                    .await?;
            }
        }
        let public: BTreeMap<String, String> = values
            .iter()
            .filter(|(key, _)| {
                !manifest
                    .user_config_fields()
                    .iter()
                    .any(|f| &f.key == *key && f.sensitive)
            })
            .map(|(k, v)| (k.clone(), v.trim().to_owned()))
            .collect();
        let config = manifest.config_with(&values);
        let saved = public.clone();
        self.store
            .write(move |c| {
                repos::connectors::set_user_config(c, id, &saved)?;
                repos::connectors::set_config(c, id, &config)
            })
            .await?;
        // A native connector's tool list can depend on its answers — `web` offers `search` only
        // once there is a key — so the recorded list is taken again rather than only the
        // registry rebuilt. Without this the connector starts answering `search` calls while its
        // page still lists the tools it had before the form was filled in.
        if instance.kind == ConnectorKind::Native {
            self.record_native(&instance).await?;
        } else {
            self.rebuild().await?;
        }
        Ok(())
    }

    pub async fn install(&self, catalog_id: &str) -> Result<InstanceId, GantryError> {
        let manifest = self
            .catalog
            .get(catalog_id)
            .ok_or_else(|| GantryError::not_found(format!("catalog entry {catalog_id}")))?;
        // 03 §11: there is no "install anyway". An instance whose runtime is missing is an entry
        // in the list that fails every call with an error about `npx`, which means nothing to the
        // person reading it; refusing here is the only message that says what to do.
        let missing = self.runtime_check(catalog_id).await?;
        let missing: Vec<&gantry_connectors::runtime::RuntimeStatus> =
            missing.iter().filter(|r| !r.ok).collect();
        if let Some(first) = missing.first() {
            return Err(GantryError::invalid(
                first
                    .problem
                    .clone()
                    .unwrap_or_else(|| format!("{} is missing", first.name)),
            ));
        }
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

    /// The stderr of a local server, newest last (03 §11 step 4). Empty for a remote one, which
    /// has no process and explains itself over HTTP.
    #[must_use]
    pub fn logs(&self, id: InstanceId) -> Vec<String> {
        self.logs.lines(id)
    }

    /// Removes an instance, its credentials and its registered OAuth clients (03 §11).
    pub async fn remove(&self, id: InstanceId) -> Result<(), GantryError> {
        let instance = self.instance(id)?;
        self.logs.clear(id);
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
        if instance.kind == ConnectorKind::Native {
            return self.record_native(&instance).await;
        }
        let endpoint = self.endpoint(&instance).await?;
        let remote = instance.kind == ConnectorKind::McpRemote;
        // A one-shot session for the install's first connection: it is closed at the end of
        // this function, so a `tools/list_changed` arriving over it has nothing left to expire.
        match gantry_connectors::mcp::McpSession::connect(&endpoint, Default::default()).await {
            Ok(session) => {
                let defs = session
                    .tools(remote)
                    .await
                    .map_err(|e| GantryError::internal(e.to_string()))?
                    .tools;
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

    /// Registers Gantry with this authorization server and remembers the client it issued, so a
    /// later Reconnect does not create a second one (03 §7 step 2).
    async fn register_client(
        &self,
        id: InstanceId,
        issuer: &str,
        server: &discovery::AuthServer,
        manifest: Option<&Manifest>,
        resource: &discovery::ProtectedResource,
    ) -> Result<String, GantryError> {
        let endpoint = server
            .registration_endpoint
            .clone()
            .ok_or_else(|| GantryError::invalid("this server registers no clients"))?;
        let scopes = scopes_for(manifest, resource, server);
        let registered =
            discovery::register(&self.http, &endpoint, &auth::redirect_uris(), &scopes)
                .await
                .map_err(auth_error)?;
        // Registration is meant to produce a public client — PKCE, no secret — and some servers
        // issue one anyway, because their token endpoint takes nothing else: Supabase advertises
        // `client_secret_basic` and `client_secret_post` and no `none`. Dropping it, which is what
        // used to happen here, made the sign-in succeed in the browser and the token exchange
        // fail afterwards. It is a per-installation secret this machine was handed, not one
        // shipped with Gantry, so it goes to the vault like any other credential (06 §5).
        if let Some(secret) = registered.client_secret.as_deref() {
            self.secrets
                .set(
                    OwnerKind::Instance,
                    &id.to_string(),
                    CredentialKind::OauthClientSecret,
                    Some(issuer),
                    secret,
                )
                .await?;
        }
        let record = repos::connectors::OauthClient {
            id: ulid::Ulid::new().to_string(),
            instance_id: id,
            issuer: issuer.to_owned(),
            client_id: registered.client_id.clone(),
            registration_json: None,
        };
        self.store
            .write(move |c| repos::connectors::put_oauth_client(c, &record))
            .await?;
        Ok(registered.client_id)
    }

    /// The secret a registration handed this instance for that issuer, if there was one.
    fn client_secret(&self, id: InstanceId, issuer: &str) -> Option<String> {
        self.secret_named(id, issuer).ok().flatten()
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
        // A server that publishes no protected-resource document is not necessarily one Gantry
        // cannot sign in to: several serve authorization-server metadata on their own origin and
        // skip the newer document entirely, so that is tried before giving up.
        let resource = match discovery::protected_resource(&self.http, &metadata_url).await {
            Ok(document) if !document.authorization_servers.is_empty() => document,
            answer => {
                if let Err(err) = answer {
                    log::info!("{metadata_url}: {err}; assuming the resource is its own issuer");
                }
                discovery::resource_as_issuer(url).ok_or_else(|| {
                    GantryError::invalid("this server publishes no OAuth metadata")
                })?
            }
        };
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
                ClientSource::Cimd => {
                    // Every server that advertises a client-id metadata document is believed
                    // until it is asked, and one of them says no: Lovable answers `invalid_client`
                    // to the id its own metadata promised to take. Asking costs one request and
                    // saves the user a browser window that can only end in an error page.
                    let redirect = auth::redirect_uris().first().cloned().unwrap_or_default();
                    let scopes = scopes_for(manifest.as_deref(), &resource, &server);
                    if discovery::accepts_client_id(
                        &self.http,
                        &server,
                        auth::CIMD_URL,
                        &redirect,
                        &scopes,
                    )
                    .await
                    {
                        auth::CIMD_URL.to_owned()
                    } else if server.registration_endpoint.is_some() {
                        log::info!(
                            "{issuer} advertises a client-id metadata document and refuses ours; \
                             registering instead"
                        );
                        self.register_client(id, &issuer, &server, manifest.as_deref(), &resource)
                            .await?
                    } else {
                        auth::CIMD_URL.to_owned()
                    }
                }
                ClientSource::Dynamic => {
                    self.register_client(id, &issuer, &server, manifest.as_deref(), &resource)
                        .await?
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
        let secret = self.client_secret(id, &issuer);
        let result = match step {
            Step::Redirect(pending) => {
                pending
                    .complete(
                        &self.http,
                        &server,
                        &client_id,
                        secret.as_deref(),
                        Some(&resource),
                    )
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

    /// Every `user_config` answer for one instance, the sensitive ones fetched from the vault.
    ///
    /// The public answers are a column on the instance row. A `sensitive` one never is (06 §3):
    /// it was filed as a `user_config_secret` credential under the name of the field it fills,
    /// and this is the other half of that — `set_user_config` writes it, and nothing read it
    /// back until now.
    ///
    /// A vault that will not open, or an answer that is not there, is an absent answer and not
    /// an error. Every native `user_config` field is optional by construction: a connector whose
    /// key cannot be read should offer less, not fail to load and take its other tools with it.
    fn native_config(&self, id: InstanceId) -> crate::native::NativeConfig {
        let public = self
            .store
            .read(move |conn| repos::connectors::user_config(conn, id))
            .unwrap_or_default();
        let mut secrets = BTreeMap::new();
        if let Ok(stored) = self
            .secrets
            .list_for_owner(OwnerKind::Instance, &id.to_string())
        {
            for secret in stored
                .iter()
                .filter(|s| s.kind == CredentialKind::UserConfigSecret.as_str())
            {
                if let Some(field) = secret.label.clone()
                    && let Ok(value) = self.secrets.get(&secret.id)
                {
                    secrets.insert(field, value);
                }
            }
        }
        crate::native::NativeConfig { public, secrets }
    }

    /// A native connector's tools come from its own code, so "connect" means recording what it
    /// offers. There is no process to start and nothing to authorize.
    async fn record_native(
        &self,
        instance: &ConnectorInstanceDto,
    ) -> Result<Vec<ToolInfo>, GantryError> {
        let catalog_id = instance.catalog_id.as_deref().unwrap_or_default();
        let defs = crate::native::definitions(catalog_id, &self.native_config(instance.id))
            .ok_or_else(|| {
                GantryError::internal(format!("{} has no code in this build", instance.name))
            })?;
        let manifest = self.catalog.get(catalog_id);
        let tools = tool_infos(&defs, manifest.as_deref());
        let id = instance.id;
        let (t, d) = (tools.clone(), with_overrides(&defs, manifest.as_deref()));
        self.store
            .write(move |c| repos::connectors::record_connection(c, id, &t, &d, None, None))
            .await?;
        self.rebuild().await?;
        Ok(tools)
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
                secret_env,
                cwd,
            } => {
                // The config row carries only the *names* of the secret variables (06 §3); their
                // values are vault credentials, labelled with the name, and this is the one
                // place they are decrypted — into a short-lived `Endpoint` that is never stored.
                let mut env = env.clone();
                for name in secret_env {
                    match self.secret_named(instance.id, name)? {
                        Some(value) => env.push((name.clone(), value)),
                        None => log::warn!(
                            "{}: no value stored for {name}; the server will start without it",
                            instance.name
                        ),
                    }
                }
                // A local server reads its key from the environment, so `inject.in` is `env`
                // and the name is the variable the server documents.
                if let Some((inject, value)) = self.pasted_key(instance)? {
                    if inject.location == "env" {
                        env.push((inject.name.clone(), inject.render(&value)));
                    } else {
                        log::warn!(
                            "{}: the manifest injects its key in `{}`, which a local process has \
                             no way to read",
                            instance.name,
                            inject.location
                        );
                    }
                }
                Ok(Endpoint::Stdio {
                    command: command.clone(),
                    args: args.clone(),
                    env,
                    cwd: cwd.clone(),
                    log: Some((self.logs.clone(), instance.id)),
                })
            }
            ConnectorConfig::McpRemote { url, headers, .. } => {
                let mut url = url.clone();
                let mut headers = headers.clone();
                let mut bearer = None;
                // A key the user pasted goes where the manifest says the server wants it, which
                // for most servers is `Authorization: Bearer …` and for several is not: Exa
                // reads a query parameter, Tinybird its own header. An OAuth token is always the
                // `Authorization` value and says so in the credential itself.
                match self.pasted_key(instance)? {
                    Some((inject, value)) => {
                        place_key(&inject, &value, &mut url, &mut headers, &mut bearer);
                    }
                    None => bearer = self.authorization_header(instance).await?,
                }
                Ok(Endpoint::Http {
                    url,
                    headers,
                    bearer,
                })
            }
        }
    }

    /// The key this instance was given and where its manifest says it goes, for a connector
    /// that authenticates with a pasted key rather than a sign-in (03 §7). `None` for everything
    /// else — an OAuth instance, a server that needs nothing, one the user added by hand.
    fn pasted_key(
        &self,
        instance: &ConnectorInstanceDto,
    ) -> Result<Option<(gantry_connectors::manifest::Inject, String)>, GantryError> {
        if instance.auth != AuthType::ApiKey {
            return Ok(None);
        }
        let Some(manifest) = instance
            .catalog_id
            .as_deref()
            .and_then(|c| self.catalog.get(c))
        else {
            return Ok(None);
        };
        let gantry_connectors::manifest::Auth::ApiKey { inject, .. } = &manifest.auth else {
            return Ok(None);
        };
        let id = instance.id;
        let Some(credential) = self
            .store
            .read(move |c| repos::connectors::credential_id(c, id))?
        else {
            return Ok(None);
        };
        use secrecy::ExposeSecret;
        let secret = self.secrets.get(&credential)?;
        let raw = secret.expose_secret().trim();
        // An entry can accept either a key or a sign-in (`auth_alternate`, 03 §7), and this
        // instance may have taken the other road: an OAuth result is stored as JSON and belongs
        // in the `Authorization` header it carries, not in whatever slot the key would fill.
        if serde_json::from_str::<StoredToken>(raw).is_ok() {
            return Ok(None);
        }
        Ok(Some((inject.clone(), raw.to_owned())))
    }

    /// The `Authorization` value for this instance, if it has a credential at all.
    /// One of an instance's secrets by the label it was stored under, which for a `user_config`
    /// answer is the key the manifest named.
    fn secret_named(&self, id: InstanceId, label: &str) -> Result<Option<String>, GantryError> {
        let Some(found) = self
            .secrets
            .list_for_owner(OwnerKind::Instance, &id.to_string())?
            .into_iter()
            .find(|s| s.label.as_deref() == Some(label))
        else {
            return Ok(None);
        };
        use secrecy::ExposeSecret;
        Ok(Some(
            self.secrets.get(&found.id)?.expose_secret().to_owned(),
        ))
    }

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
            match self.refresh(id, &mut token, &refresh).await {
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

    async fn refresh(
        &self,
        id: InstanceId,
        token: &mut StoredToken,
        refresh: &str,
    ) -> Result<(), GantryError> {
        let server = discovery::auth_server(&self.http, &token.issuer)
            .await
            .map_err(auth_error)?;
        let secret = self.client_secret(id, &token.issuer);
        let fresh = flow::refresh(
            &self.http,
            &server,
            &token.client_id,
            secret.as_deref(),
            refresh,
            None,
        )
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
            if instance.kind == ConnectorKind::Native {
                let config = self.native_config(instance.id);
                match instance.catalog_id.as_deref().and_then(|catalog_id| {
                    crate::native::build(
                        catalog_id,
                        instance.namespace.clone(),
                        instance.id,
                        &self.workspace,
                        &self.shell_env,
                        &config,
                    )
                }) {
                    Some(connector) => self.registry.register(connector),
                    None => log::warn!(
                        "{} says it is native, but no code is registered for it",
                        instance.name
                    ),
                }
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

/// Puts a pasted key on the wire the way the manifest says: `Authorization` when that is the
/// header named, another header when the server uses its own, or a query parameter when it reads
/// the key from the URL. The `Authorization` case is kept apart because rmcp sends that one on
/// every request including the event stream, which is exactly what a key has to do too.
fn place_key(
    inject: &gantry_connectors::manifest::Inject,
    value: &str,
    url: &mut String,
    headers: &mut Vec<(String, String)>,
    bearer: &mut Option<String>,
) {
    let rendered = inject.render(value);
    match inject.location.as_str() {
        "header" if inject.name.eq_ignore_ascii_case("authorization") => *bearer = Some(rendered),
        "header" => headers.push((inject.name.clone(), rendered)),
        "query" => {
            let separator = if url.contains('?') { '&' } else { '?' };
            url.push(separator);
            url.push_str(
                &url::form_urlencoded::Serializer::new(String::new())
                    .append_pair(&inject.name, &rendered)
                    .finish(),
            );
        }
        // `env` on a remote server: there is no process to give it to. The manifest is wrong and
        // the connector will fail to authorize, which is the honest outcome.
        other => log::warn!("a remote server cannot take its key in `{other}`"),
    }
}

#[cfg(test)]
mod tests {
    use gantry_secrets::ExposeSecret;

    use super::*;

    /// A service on a temporary database and a vault with a known key, so a test can watch a
    /// secret go in one end and come out the other.
    fn service() -> (tempfile::TempDir, ConnectorService) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(gantry_store::Store::open(dir.path().join("t.db")).unwrap());
        let blobs = Arc::new(gantry_store::BlobStore::open(dir.path().join("blobs")).unwrap());
        let secrets = Arc::new(SecretVault::with_key(
            [7u8; 32],
            store.clone(),
            gantry_secrets::SecretStoreStatus::FileFallback {
                path: "test".into(),
            },
        ));
        let registry = Arc::new(ConnectorRegistry::new());
        let workspace = Arc::new(Workspace::new(
            store.clone(),
            blobs,
            dir.path().join("app-data"),
        ));
        let service = ConnectorService::new(
            store,
            secrets,
            registry,
            workspace,
            Arc::new(ShellEnv::inherited()),
        );
        (dir, service)
    }

    /// The BYOK round trip end to end (03 §5, §11 step 2): the key the user types reaches the
    /// connector, and reaches it from the vault rather than from a row anybody can read.
    #[tokio::test]
    async fn a_search_key_turns_the_search_tool_on_without_being_written_to_the_database() {
        let (_dir, service) = service();
        let id = service.install("web").await.unwrap();
        // Install writes the row; the UI then connects, which for a native connector means
        // recording what its code offers. That is the order the app does it in.
        let recorded = service.connect(id).await.unwrap();
        assert_eq!(
            recorded.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["fetch_url"],
            "with no key there is no search tool to record"
        );

        // Before any key: the connector is there and offers only what needs no account.
        assert_eq!(offered(&service).await, ["fetch_url"]);

        service
            .set_user_config(
                id,
                BTreeMap::from([
                    ("SEARCH_PROVIDER".to_owned(), "brave".to_owned()),
                    ("SEARCH_API_KEY".to_owned(), "  a-real-key\n".to_owned()),
                ]),
            )
            .await
            .unwrap();

        // The tool the key pays for is now offered, and the recorded list agrees with it.
        assert_eq!(offered(&service).await, ["fetch_url", "search"]);
        let listed: Vec<String> = service
            .instance(id)
            .unwrap()
            .tools
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(listed, ["fetch_url", "search"], "the connector's page");

        // And the key is not in the database: the row keeps the provider and nothing else.
        let public = service
            .store
            .read(move |c| repos::connectors::user_config(c, id))
            .unwrap();
        assert_eq!(
            public.get("SEARCH_PROVIDER").map(String::as_str),
            Some("brave")
        );
        assert!(
            !public.contains_key("SEARCH_API_KEY"),
            "a sensitive answer never reaches the config row (06 §3): {public:?}"
        );
        let instance = service.instance(id).unwrap();
        let rendered = format!("{:?}", instance.config);
        assert!(!rendered.contains("a-real-key"), "{rendered}");

        // It is in the vault, trimmed on the way in, under the name of the field it fills.
        let stored = service
            .secrets
            .list_for_owner(OwnerKind::Instance, &id.to_string())
            .unwrap();
        let secret = stored
            .iter()
            .find(|s| s.label.as_deref() == Some("SEARCH_API_KEY"))
            .expect("the key is in the vault");
        assert_eq!(secret.kind, CredentialKind::UserConfigSecret.as_str());
        assert_eq!(
            service.secrets.get(&secret.id).unwrap().expose_secret(),
            "a-real-key"
        );
    }

    /// What the registered `web` connector says it offers right now — the same question the
    /// turn loop asks when it assembles a turn's tools.
    async fn offered(service: &ConnectorService) -> Vec<String> {
        let connector = service.registry.get("web").expect("web is registered");
        connector
            .tools()
            .await
            .unwrap()
            .into_iter()
            .map(|d| d.name)
            .collect()
    }

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

    #[test]
    fn a_key_goes_where_the_manifest_says_and_nowhere_else() {
        use gantry_connectors::manifest::Inject;
        let mut url = "https://mcp.example.test/mcp".to_owned();
        let mut headers = Vec::new();
        let mut bearer = None;

        // The common case: the server wants a bearer, and rmcp carries it on every request.
        place_key(
            &Inject {
                location: "header".into(),
                name: "Authorization".into(),
                format: Some("Bearer {value}".into()),
            },
            "k1",
            &mut url,
            &mut headers,
            &mut bearer,
        );
        assert_eq!(bearer.as_deref(), Some("Bearer k1"));
        assert!(headers.is_empty(), "and not a second time as a header");

        // A server with its own header, and one that reads the URL.
        let mut bearer = None;
        place_key(
            &Inject {
                location: "header".into(),
                name: "X-Api-Key".into(),
                format: None,
            },
            "k2",
            &mut url,
            &mut headers,
            &mut bearer,
        );
        assert_eq!(headers, vec![("X-Api-Key".to_owned(), "k2".to_owned())]);
        assert!(bearer.is_none());

        place_key(
            &Inject {
                location: "query".into(),
                name: "exaApiKey".into(),
                format: None,
            },
            "k 3",
            &mut url,
            &mut headers,
            &mut bearer,
        );
        assert_eq!(url, "https://mcp.example.test/mcp?exaApiKey=k+3");
    }
}
