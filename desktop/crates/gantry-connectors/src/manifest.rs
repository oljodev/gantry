//! The connector manifest (docs/plan/03 §3), as read from `desktop/connectors/<id>/manifest.json`
//! and validated against `desktop/schemas/connector-manifest.schema.json`.
//!
//! Only the fields Gantry acts on are modelled. Unknown fields are ignored rather than refused,
//! so a manifest written for a later version of the schema still installs.

use std::collections::BTreeMap;

use gantry_core::{
    AuthType, CatalogEntryDto, ConnectorConfig, ConnectorKind, RiskTier, RuntimeRequirement,
    UserConfigField, UserConfigKind,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub long_description: Option<String>,
    pub version: String,
    #[serde(default)]
    pub icon: Option<String>,
    pub category: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub first_party: bool,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub multi_instance: bool,
    pub runtime: Runtime,
    pub auth: Auth,
    /// A second way in, offered beside `auth` (03 §7). GitHub takes either a sign-in or a
    /// personal access token, and which one suits depends on whether the user has an OAuth
    /// application to point at.
    #[serde(default)]
    pub auth_alternate: Option<Auth>,
    pub risk: Risk,
    /// Keys the user fills in at install (03 §11 step 2), referenced from the runtime as
    /// `${user_config.KEY}`. Ordered by key, because a form has to come out in *some* order and
    /// the alternative — whatever order serde read the object in — changes between runs.
    #[serde(default)]
    pub user_config: BTreeMap<String, UserConfigFieldSpec>,
    #[serde(default)]
    pub tool_overrides: BTreeMap<String, ToolOverride>,
    #[serde(default)]
    pub catalog: CatalogMeta,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Runtime {
    Native {
        #[serde(rename = "crate")]
        crate_name: String,
    },
    McpStdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        requires: BTreeMap<String, String>,
        #[serde(default)]
        platform_overrides: BTreeMap<String, PlatformOverride>,
    },
    McpRemote {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlatformOverride {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>,
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Auth {
    None,
    ApiKey {
        inject: Inject,
        #[serde(default)]
        instructions: Option<String>,
        #[serde(default)]
        setup_url: Option<String>,
    },
    Headers {
        fields: Vec<HeaderField>,
        #[serde(default)]
        instructions: Option<String>,
        #[serde(default)]
        setup_url: Option<String>,
    },
    Oauth2 {
        #[serde(default)]
        registration: Vec<String>,
        #[serde(default)]
        scopes: Vec<String>,
        #[serde(default)]
        client: Option<OauthClientRef>,
        #[serde(default)]
        user_supplied_fields: Vec<String>,
        #[serde(default)]
        instructions: Option<String>,
        #[serde(default)]
        setup_url: Option<String>,
    },
}

impl Auth {
    #[must_use]
    pub fn kind(&self) -> AuthType {
        match self {
            Auth::None => AuthType::None,
            Auth::ApiKey { .. } => AuthType::ApiKey,
            Auth::Headers { .. } => AuthType::Headers,
            Auth::Oauth2 { .. } => AuthType::Oauth2,
        }
    }

    /// What the install dialog tells the user to do, when the manifest says.
    #[must_use]
    pub fn instructions(&self) -> Option<&str> {
        match self {
            Auth::None => None,
            Auth::ApiKey { instructions, .. }
            | Auth::Headers { instructions, .. }
            | Auth::Oauth2 { instructions, .. } => instructions.as_deref(),
        }
    }

    /// The page where this credential is created, for the button that opens it.
    #[must_use]
    pub fn setup_url(&self) -> Option<&str> {
        match self {
            Auth::None => None,
            Auth::ApiKey { setup_url, .. }
            | Auth::Headers { setup_url, .. }
            | Auth::Oauth2 { setup_url, .. } => setup_url.as_deref(),
        }
    }

    /// The client id the manifest pins, for a server that registers nobody (03 §7).
    #[must_use]
    pub fn client_id(&self) -> Option<&str> {
        match self {
            Auth::Oauth2 { client, .. } => client.as_ref().and_then(|c| c.client_id.as_deref()),
            _ => None,
        }
    }

    /// Whether the user has to make a client of their own before signing in: the manifest asks
    /// for a `client_id` and ships none itself. Everything else — dynamic registration, a
    /// client-id metadata document, an id pinned in the manifest — happens without them.
    #[must_use]
    pub fn needs_client_id(&self) -> bool {
        match self {
            Auth::Oauth2 {
                user_supplied_fields,
                client,
                ..
            } => {
                user_supplied_fields.iter().any(|f| f == "client_id")
                    && client
                        .as_ref()
                        .and_then(|c| c.client_id.as_deref())
                        .is_none()
            }
            _ => false,
        }
    }

    #[must_use]
    pub fn scopes(&self) -> Vec<String> {
        match self {
            Auth::Oauth2 { scopes, .. } => scopes.clone(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct OauthClientRef {
    #[serde(default)]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Inject {
    #[serde(rename = "in")]
    pub location: String,
    pub name: String,
    #[serde(default)]
    pub format: Option<String>,
}

impl Inject {
    /// The value as it goes on the wire: `Bearer abc` from a format of `Bearer {value}`.
    #[must_use]
    pub fn render(&self, value: &str) -> String {
        match &self.format {
            Some(format) => format.replace("{value}", value),
            None => value.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeaderField {
    pub name: String,
    pub header: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default = "yes")]
    pub sensitive: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct Risk {
    pub network: String,
    pub local_system: String,
    pub default_tool_tier: RiskTier,
    #[serde(default)]
    pub notes: Option<String>,
}

/// A `user_config` entry as the manifest writes it: the key is the map key, so it is not here.
#[derive(Debug, Clone, Deserialize)]
pub struct UserConfigFieldSpec {
    #[serde(rename = "type")]
    pub kind: UserConfigKind,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub sensitive: bool,
    /// Whatever JSON the manifest wrote; the form shows it as text either way.
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ToolOverride {
    #[serde(default)]
    pub risk: Option<RiskTier>,
    #[serde(default)]
    pub always_confirm: Option<bool>,
    #[serde(default)]
    pub parallel_safe: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CatalogMeta {
    #[serde(default)]
    pub featured: bool,
    #[serde(default)]
    pub sort_weight: f64,
    #[serde(default)]
    pub suggest_for: Vec<String>,
}

/// `${user_config.KEY}` replaced by what the user gave, everywhere it appears.
///
/// A key with no answer is left standing rather than blanked. `--host ${user_config.HOST}` with
/// the placeholder still in it fails with a message naming the key; the same line with an empty
/// string fails somewhere inside the server, later, saying something else.
fn substitute(text: &str, values: &BTreeMap<&str, &str>) -> String {
    const OPEN: &str = "${user_config.";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(OPEN) {
        let after = &rest[at + OPEN.len()..];
        let Some(close) = after.find('}') else { break };
        match values.get(&after[..close]) {
            Some(value) => {
                out.push_str(&rest[..at]);
                out.push_str(value);
            }
            None => out.push_str(&rest[..at + OPEN.len() + close + 1]),
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("{id}: {message}")]
    Invalid { id: String, message: String },
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Self, ManifestError> {
        let manifest: Manifest =
            serde_json::from_str(json).map_err(|e| ManifestError::Invalid {
                id: "manifest".to_owned(),
                message: e.to_string(),
            })?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// What the schema cannot express: an id that matches its folder, and a runtime that suits
    /// the auth (a native connector cannot speak OAuth to anything).
    fn validate(&self) -> Result<(), ManifestError> {
        let bad_id = self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if bad_id {
            return Err(ManifestError::Invalid {
                id: self.id.clone(),
                message: "an id is lowercase letters, digits and hyphens".to_owned(),
            });
        }
        if matches!(self.runtime, Runtime::Native { .. })
            && !matches!(self.auth, Auth::None | Auth::ApiKey { .. })
        {
            return Err(ManifestError::Invalid {
                id: self.id.clone(),
                message: "a native connector authenticates with a key or not at all".to_owned(),
            });
        }
        Ok(())
    }

    #[must_use]
    pub fn kind(&self) -> ConnectorKind {
        match self.runtime {
            Runtime::Native { .. } => ConnectorKind::Native,
            Runtime::McpStdio { .. } => ConnectorKind::McpStdio,
            Runtime::McpRemote { .. } => ConnectorKind::McpRemote,
        }
    }

    /// The runtime as an installable configuration, with this platform's overrides applied and
    /// the user's answers substituted into it (03 §11 step 2).
    ///
    /// A `sensitive` answer is not among them. Its value is a vault credential, and the config
    /// row carries only the name of the environment variable or header it fills (06 §3), so a
    /// database anybody can read never holds a token. Substituting it here would put it there.
    #[must_use]
    pub fn config_with(&self, values: &BTreeMap<String, String>) -> ConnectorConfig {
        let public: BTreeMap<&str, &str> = values
            .iter()
            .filter(|(key, _)| !self.user_config.get(*key).is_some_and(|f| f.sensitive))
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let fill = |text: &String| substitute(text, &public);
        match self.config() {
            ConnectorConfig::Native => ConnectorConfig::Native,
            ConnectorConfig::McpStdio {
                command,
                args,
                env,
                secret_env,
                cwd,
            } => ConnectorConfig::McpStdio {
                command: fill(&command),
                args: args.iter().map(fill).collect(),
                env: env.iter().map(|(k, v)| (k.clone(), fill(v))).collect(),
                secret_env,
                cwd: cwd.as_ref().map(fill),
            },
            ConnectorConfig::McpRemote {
                url,
                headers,
                secret_headers,
            } => ConnectorConfig::McpRemote {
                url: fill(&url),
                headers: headers.iter().map(|(k, v)| (k.clone(), fill(v))).collect(),
                secret_headers,
            },
        }
    }

    /// The runtime as an installable configuration, with this platform's overrides applied.
    #[must_use]
    pub fn config(&self) -> ConnectorConfig {
        match &self.runtime {
            Runtime::Native { .. } => ConnectorConfig::Native,
            Runtime::McpStdio {
                command,
                args,
                env,
                cwd,
                platform_overrides,
                ..
            } => {
                let over = platform_overrides.get(platform());
                ConnectorConfig::McpStdio {
                    command: over
                        .and_then(|o| o.command.clone())
                        .unwrap_or_else(|| command.clone()),
                    args: over
                        .and_then(|o| o.args.clone())
                        .unwrap_or_else(|| args.clone()),
                    env: over
                        .and_then(|o| o.env.clone())
                        .unwrap_or_else(|| env.clone())
                        .into_iter()
                        .collect(),
                    secret_env: Vec::new(),
                    cwd: cwd.clone(),
                }
            }
            Runtime::McpRemote { url, headers } => ConnectorConfig::McpRemote {
                url: url.clone(),
                headers: headers.clone().into_iter().collect(),
                secret_headers: Vec::new(),
            },
        }
    }

    /// The form the install dialog shows at step 2, in key order.
    #[must_use]
    pub fn user_config_fields(&self) -> Vec<UserConfigField> {
        self.user_config
            .iter()
            .map(|(key, spec)| UserConfigField {
                key: key.clone(),
                kind: spec.kind,
                title: spec.title.clone(),
                description: spec.description.clone(),
                required: spec.required,
                sensitive: spec.sensitive,
                default: spec.default.as_ref().map(|v| match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                }),
            })
            .collect()
    }

    /// The runtimes that must be present before this can be installed (03 §11 step 1).
    #[must_use]
    pub fn requires(&self) -> Vec<RuntimeRequirement> {
        match &self.runtime {
            Runtime::McpStdio { requires, .. } => requires
                .iter()
                .map(|(name, version)| RuntimeRequirement {
                    name: name.clone(),
                    version: version.clone(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    #[must_use]
    pub fn entry(&self, installed: Vec<gantry_core::InstanceId>) -> CatalogEntryDto {
        CatalogEntryDto {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            category: self.category.clone(),
            kind: self.kind(),
            auth: self.auth.kind(),
            first_party: self.first_party,
            keywords: self.keywords.clone(),
            homepage: self.homepage.clone(),
            preview: self.config().preview(),
            requires: self.requires(),
            installed,
            multi_instance: self.multi_instance,
            long_description: self.long_description.clone(),
            auth_instructions: self.auth.instructions().map(str::to_owned),
            auth_setup_url: self.auth.setup_url().map(str::to_owned),
            auth_alternate: self.auth_alternate.as_ref().map(Auth::kind),
            auth_alternate_instructions: self
                .auth_alternate
                .as_ref()
                .and_then(|a| a.instructions())
                .map(str::to_owned),
            auth_alternate_setup_url: self
                .auth_alternate
                .as_ref()
                .and_then(|a| a.setup_url())
                .map(str::to_owned),
            auth_needs_client_id: self.auth.needs_client_id(),
        }
    }

    /// How well this entry answers a search, for the browse list and for
    /// `gantry__search_connectors` (03 §9). Zero means no match.
    #[must_use]
    pub fn score(&self, query: &str) -> u32 {
        let q = query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return 1;
        }
        let mut score = 0;
        if self.id == q || self.name.to_ascii_lowercase() == q {
            score += 100;
        }
        if self.name.to_ascii_lowercase().contains(&q) {
            score += 40;
        }
        if self.description.to_ascii_lowercase().contains(&q) {
            score += 15;
        }
        for word in self.keywords.iter().chain(self.catalog.suggest_for.iter()) {
            if word.to_ascii_lowercase().contains(&q) {
                score += 10;
            }
        }
        score
    }
}

#[must_use]
pub fn platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        _ => "linux",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_answer_reaches_every_place_the_runtime_names_it() {
        let manifest = Manifest::parse(
            r#"{"manifest_version":"1","id":"x","name":"X","description":"d","version":"1.0.0",
                "icon":"icon.svg","category":"data","publisher":{"name":"p"},
                "runtime":{"kind":"mcp-stdio","command":"npx",
                  "args":["-y","srv","--host","${user_config.HOST}"],
                  "env":{"SRV_URL":"https://${user_config.HOST}/api"}},
                "auth":{"type":"none"},"risk":{"network":"any","local_system":"none",
                  "default_tool_tier":"read"},
                "user_config":{
                  "HOST":{"type":"string","title":"Host","required":true},
                  "TOKEN":{"type":"string","title":"Token","sensitive":true}}}"#,
        )
        .unwrap();
        let answers = BTreeMap::from([
            ("HOST".to_owned(), "metabase.example".to_owned()),
            ("TOKEN".to_owned(), "s3cret".to_owned()),
        ]);
        let ConnectorConfig::McpStdio { args, env, .. } = manifest.config_with(&answers) else {
            panic!("stdio")
        };
        assert_eq!(args.last().unwrap(), "metabase.example");
        assert_eq!(env[0].1, "https://metabase.example/api");

        // A sensitive answer is a vault credential (06 §3). Substituting it would write a token
        // into a config row that anybody with the database can read.
        let text = serde_json::to_string(&manifest.config_with(&answers)).unwrap();
        assert!(!text.contains("s3cret"));
    }

    /// An unanswered key is left standing: the failure then names the key, where blanking it
    /// produces a failure somewhere inside the server saying something else.
    #[test]
    fn a_key_with_no_answer_is_left_where_it_is() {
        assert_eq!(
            substitute("--host ${user_config.HOST}", &BTreeMap::new()),
            "--host ${user_config.HOST}"
        );
        assert_eq!(
            substitute(
                "a${user_config.A}b${user_config.B}c",
                &BTreeMap::from([("A", "1")])
            ),
            "a1b${user_config.B}c"
        );
        assert_eq!(
            substitute("nothing to do", &BTreeMap::new()),
            "nothing to do"
        );
    }

    #[test]
    fn the_form_comes_out_in_key_order_whatever_order_it_was_written_in() {
        let manifest = Manifest::parse(
            r#"{"manifest_version":"1","id":"x","name":"X","description":"d","version":"1.0.0",
                "icon":"icon.svg","category":"data","publisher":{"name":"p"},
                "runtime":{"kind":"mcp-remote","url":"https://x.test/mcp"},
                "auth":{"type":"none"},"risk":{"network":"any","local_system":"none",
                  "default_tool_tier":"read"},
                "user_config":{
                  "ZONE":{"type":"string","title":"Zone"},
                  "ACCOUNT":{"type":"string","title":"Account","default":"main"}}}"#,
        )
        .unwrap();
        let fields = manifest.user_config_fields();
        assert_eq!(fields[0].key, "ACCOUNT");
        assert_eq!(fields[0].default.as_deref(), Some("main"));
        assert_eq!(fields[1].key, "ZONE");
    }

    use super::*;

    const REMOTE: &str = r#"{
      "manifest_version": "1", "id": "cloudflare-docs", "name": "Cloudflare Docs",
      "description": "Reference for every Cloudflare product.", "version": "1.0.0",
      "icon": "icon.svg", "category": "developer", "publisher": { "name": "Cloudflare" },
      "keywords": ["workers", "dns"],
      "runtime": { "kind": "mcp-remote", "url": "https://docs.mcp.cloudflare.com/mcp" },
      "auth": { "type": "none" },
      "risk": { "network": "internet", "local_system": "none", "default_tool_tier": "read" }
    }"#;

    #[test]
    fn a_remote_entry_becomes_a_remote_configuration() {
        let manifest = Manifest::parse(REMOTE).expect("a manifest");
        assert_eq!(manifest.kind(), ConnectorKind::McpRemote);
        assert_eq!(manifest.auth.kind(), AuthType::None);
        assert_eq!(
            manifest.config().preview(),
            "https://docs.mcp.cloudflare.com/mcp"
        );
        assert!(manifest.requires().is_empty());
    }

    #[test]
    fn search_prefers_the_name_over_the_description() {
        let manifest = Manifest::parse(REMOTE).expect("a manifest");
        assert!(manifest.score("cloudflare") > manifest.score("reference"));
        assert_eq!(manifest.score("postgres"), 0);
        assert!(manifest.score("dns") > 0, "keywords count");
    }

    #[test]
    fn a_native_connector_may_not_claim_oauth() {
        let json = REMOTE
            .replace(
                r#""runtime": { "kind": "mcp-remote", "url": "https://docs.mcp.cloudflare.com/mcp" }"#,
                r#""runtime": { "kind": "native", "crate": "gantry-connector-filesystem" }"#,
            )
            .replace(
                r#""auth": { "type": "none" }"#,
                r#""auth": { "type": "oauth2", "registration": ["dcr"] }"#,
            );
        assert!(Manifest::parse(&json).is_err());
    }

    #[test]
    fn an_injected_key_takes_the_format_the_manifest_gives() {
        let inject = Inject {
            location: "header".into(),
            name: "Authorization".into(),
            format: Some("Bearer {value}".into()),
        };
        assert_eq!(inject.render("abc"), "Bearer abc");
    }
}
