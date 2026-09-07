//! Finding out how to authorize (docs/plan/03 §7 steps 1–2): the protected-resource document,
//! the authorization server's metadata, and dynamic registration where a server offers it.
//!
//! Three details are load-bearing, and each was observed on a real server on 2026-09-07:
//! the metadata may live at RFC 8414's path-insertion URL rather than under the issuer;
//! a server may support neither dynamic registration nor a client-id metadata document, and
//! then a pre-registered client is the only way in; and the challenge in a 401 names the
//! document to read, so it is worth reading rather than guessing.

use serde::Deserialize;
use url::Url;

use crate::auth::AuthError;

/// RFC 9728. What the server says about itself when it refuses a request.
#[derive(Debug, Clone, Deserialize)]
pub struct ProtectedResource {
    pub resource: String,
    #[serde(default)]
    pub authorization_servers: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
    #[serde(default)]
    pub resource_name: Option<String>,
}

/// RFC 8414, plus the two fields that decide how a client is registered.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthServer {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub registration_endpoint: Option<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
    #[serde(default)]
    pub scopes_supported: Vec<String>,
    /// RFC 8628. Present on servers that let a device sign in with a code the user types.
    #[serde(default)]
    pub device_authorization_endpoint: Option<String>,
    /// A server that lists `none` accepts a client with no secret; one that lists nothing has
    /// not said, and GitHub's silence means it wants a secret it will never get from a desktop
    /// application.
    #[serde(default)]
    pub token_endpoint_auth_methods_supported: Vec<String>,
    #[serde(default)]
    pub client_id_metadata_document_supported: bool,
    #[serde(default)]
    pub authorization_response_iss_parameter_supported: bool,
}

impl AuthServer {
    /// Whether the server takes the PKCE method Gantry uses. A server that lists no methods at
    /// all predates the requirement; sending a challenge anyway is harmless.
    #[must_use]
    pub fn supports_s256(&self) -> bool {
        self.code_challenge_methods_supported.is_empty()
            || self
                .code_challenge_methods_supported
                .iter()
                .any(|m| m == "S256")
    }
}

/// The `resource_metadata` URL out of a `WWW-Authenticate` challenge, if it names one.
#[must_use]
pub fn resource_metadata_url(challenge: &str) -> Option<String> {
    let key = "resource_metadata=";
    let start = challenge.find(key)? + key.len();
    let rest = &challenge[start..];
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let end = rest.find(['"', ',', ' ']).unwrap_or(rest.len());
    Some(rest[..end].to_owned())
}

/// Reads the protected-resource document. `url` is the one the challenge named; without a
/// challenge, the well-known URL is derived from the resource itself (RFC 9728 §3.1).
pub async fn protected_resource(
    http: &reqwest::Client,
    url: &str,
) -> Result<ProtectedResource, AuthError> {
    let response = http.get(url).send().await?;
    if !response.status().is_success() {
        return Err(AuthError::Discovery(format!(
            "the protected-resource document at {url} answered {}",
            response.status()
        )));
    }
    Ok(response.json().await?)
}

/// The well-known URL for a resource, with the resource's path inserted after the well-known
/// segment: `https://host/mcp` becomes `https://host/.well-known/oauth-protected-resource/mcp`.
#[must_use]
pub fn protected_resource_url(resource: &str) -> Option<String> {
    let url = Url::parse(resource).ok()?;
    let path = url.path().trim_end_matches('/');
    let origin = format!(
        "{}://{}",
        url.scheme(),
        url.host_str().map(|h| match url.port() {
            Some(port) => format!("{h}:{port}"),
            None => h.to_owned(),
        })?
    );
    Some(format!(
        "{origin}/.well-known/oauth-protected-resource{path}"
    ))
}

/// The authorization server's metadata, trying every place the specifications put it. GitHub's
/// issuer (`https://github.com/login/oauth`) serves it only at the path-insertion URL, so that
/// form is tried first; Cloudflare's issuer has no path and answers on the second.
pub async fn auth_server(http: &reqwest::Client, issuer: &str) -> Result<AuthServer, AuthError> {
    let mut tried = Vec::new();
    for url in metadata_urls(issuer) {
        match http.get(&url).send().await {
            Ok(response) if response.status().is_success() => match response.json().await {
                Ok(meta) => return Ok(meta),
                Err(err) => tried.push(format!("{url}: {err}")),
            },
            Ok(response) => tried.push(format!("{url}: {}", response.status())),
            Err(err) => tried.push(format!("{url}: {err}")),
        }
    }
    Err(AuthError::Discovery(format!(
        "no authorization server metadata for {issuer} ({})",
        tried.join("; ")
    )))
}

/// The three documented locations, in the order that answers fastest in practice.
fn metadata_urls(issuer: &str) -> Vec<String> {
    let Ok(url) = Url::parse(issuer) else {
        return Vec::new();
    };
    let path = url.path().trim_end_matches('/');
    let Some(host) = url.host_str() else {
        return Vec::new();
    };
    let origin = match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    };
    let mut urls = Vec::new();
    if !path.is_empty() {
        urls.push(format!(
            "{origin}/.well-known/oauth-authorization-server{path}"
        ));
        urls.push(format!("{origin}/.well-known/openid-configuration{path}"));
    }
    urls.push(format!(
        "{origin}{path}/.well-known/oauth-authorization-server"
    ));
    urls.push(format!("{origin}{path}/.well-known/openid-configuration"));
    urls
}

#[derive(Debug, Clone, Deserialize)]
pub struct RegisteredClient {
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
}

/// Dynamic Client Registration (RFC 7591). Public native client: no secret, PKCE instead.
pub async fn register(
    http: &reqwest::Client,
    endpoint: &str,
    redirect_uris: &[String],
    scopes: &[String],
) -> Result<RegisteredClient, AuthError> {
    let body = serde_json::json!({
        "client_name": "Gantry",
        "client_uri": "https://oljo.dev",
        "redirect_uris": redirect_uris,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
        "application_type": "native",
        "scope": scopes.join(" "),
    });
    let response = http.post(endpoint).json(&body).send().await?;
    let status = response.status();
    if !status.is_success() {
        let detail = response.text().await.unwrap_or_default();
        return Err(AuthError::Registration(format!(
            "{status}: {}",
            detail.chars().take(300).collect::<String>()
        )));
    }
    Ok(response.json().await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_challenge_names_the_document_to_read() {
        let challenge = "Bearer error=\"invalid_request\", error_description=\"No access token \
                         was provided\", resource_metadata=\"https://api.githubcopilot.com/\
                         .well-known/oauth-protected-resource/mcp/\"";
        assert_eq!(
            resource_metadata_url(challenge).as_deref(),
            Some("https://api.githubcopilot.com/.well-known/oauth-protected-resource/mcp/")
        );
        assert_eq!(resource_metadata_url("Bearer realm=\"OAuth\""), None);
    }

    #[test]
    fn an_issuer_with_a_path_is_tried_at_the_inserted_url_first() {
        // GitHub answers here and 404s at the suffix form, which is why the order matters.
        let urls = metadata_urls("https://github.com/login/oauth");
        assert_eq!(
            urls[0],
            "https://github.com/.well-known/oauth-authorization-server/login/oauth"
        );
        assert!(urls.contains(
            &"https://github.com/login/oauth/.well-known/openid-configuration".to_owned()
        ));
    }

    #[test]
    fn an_issuer_without_a_path_has_one_form() {
        let urls = metadata_urls("https://bindings.mcp.cloudflare.com");
        assert_eq!(
            urls[0],
            "https://bindings.mcp.cloudflare.com/.well-known/oauth-authorization-server"
        );
    }

    #[test]
    fn the_resource_path_moves_after_the_well_known_segment() {
        assert_eq!(
            protected_resource_url("https://bindings.mcp.cloudflare.com/mcp").as_deref(),
            Some("https://bindings.mcp.cloudflare.com/.well-known/oauth-protected-resource/mcp")
        );
    }
}
