//! The connector manifest (docs/plan/03 §3), as read from `desktop/connectors/<id>/manifest.json`
//! and validated against `desktop/schemas/connector-manifest.schema.json`.
//!
//! Only the fields Gantry acts on are modelled. Unknown fields are ignored rather than refused,
//! so a manifest written for a later version of the schema still installs.

use std::collections::BTreeMap;

use gantry_core::{
    AuthType, CatalogEntryDto, ConnectorConfig, ConnectorKind, RiskTier, RuntimeRequirement,
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
