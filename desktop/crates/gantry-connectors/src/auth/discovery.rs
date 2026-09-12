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

/// Whether this authorization server will actually accept `client_id` — asked before a browser
/// window opens, rather than discovered by the user reading the vendor's error page.
///
/// A server that advertises `client_id_metadata_document_supported` is claiming it will take a
/// URL as a client id, and Lovable's says so and answers `401 invalid_client` to one (checked
/// 2026-09-12, against five servers that accept the same id happily). There is nothing to catch
/// in the flow when that happens: the failure is in the browser, the callback never arrives, and
/// Gantry waits five minutes to report a timeout that explains nothing.
///
/// So the authorize endpoint is asked first, with a request that starts no session and grants
/// nothing. Only a refusal of the *client* counts. A `401` or `403` is one; a `400` is one only
/// when the server says `invalid_client`, because a `400` is just as likely to be about something
/// else the pre-flight left out — Perplexity answers `invalid_request` to a request with no
/// scope, which says nothing at all about the client id. Everything else, including a login page,
/// a redirect to one, and a server having a bad day, is taken as acceptance: the point is to
/// catch a certain "no", not to second-guess a "maybe".
pub async fn accepts_client_id(
    http: &reqwest::Client,
    server: &AuthServer,
    client_id: &str,
    redirect_uri: &str,
    scopes: &[String],
) -> bool {
    let Ok(mut url) = Url::parse(&server.authorization_endpoint) else {
        return true;
    };
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        // A throwaway challenge, for a code nobody will exchange.
        .append_pair(
            "code_challenge",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        )
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", "preflight");
    if !scopes.is_empty() {
        url.query_pairs_mut()
            .append_pair("scope", &scopes.join(" "));
    }
    let Ok(response) = http.get(url).send().await else {
        return true;
    };
    let status = response.status().as_u16();
    let body = if status == 400 {
        response.text().await.unwrap_or_default()
    } else {
        String::new()
    };
    !refuses_client(status, &body)
}

/// The rule on its own, so it can be read and tested without a server.
fn refuses_client(status: u16, body: &str) -> bool {
    match status {
        401 | 403 => true,
        400 => body.contains("invalid_client"),
        _ => false,
    }
}

/// What to assume when a server refuses with a `401` and publishes no protected-resource
/// document at all.
///
/// RFC 9728 is how a server *should* say where to sign in, and a good number do not: Atlassian,
/// Datadog, Intercom, Jotform, Replicate, Apify and Windsor all answer `401` and serve
/// authorization-server metadata on the resource's own origin instead (checked 2026-09-12).
/// Treating the resource as its own issuer is the assumption RFC 8414 already describes, and it
/// costs one request that either answers or does not — so a server that skipped the newer
/// document is reachable rather than unsupported.
#[must_use]
pub fn resource_as_issuer(resource: &str) -> Option<ProtectedResource> {
    let url = Url::parse(resource).ok()?;
    let host = url.host_str()?;
    let origin = match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    };
    Some(ProtectedResource {
        resource: resource.to_owned(),
        authorization_servers: vec![origin],
        scopes_supported: Vec::new(),
        resource_name: None,
    })
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
    #[test]
    fn only_a_refusal_of_the_client_counts_as_one() {
        use super::refuses_client;
        // Lovable: a bare 401 from the authorize endpoint.
        assert!(refuses_client(401, ""));
        assert!(refuses_client(400, r#"{"error":"invalid_client"}"#));
        // Perplexity: a 400 about the scope, which says nothing about the client id.
        assert!(!refuses_client(
            400,
            r#"{"error":"invalid_request","error_description":"The scope of your request is missing."}"#
        ));
        // A login page, a redirect to one, a bad afternoon.
        assert!(!refuses_client(200, ""));
        assert!(!refuses_client(302, ""));
        assert!(!refuses_client(503, ""));
    }

    #[test]
    fn a_server_with_no_protected_resource_document_is_its_own_issuer() {
        let assumed = super::resource_as_issuer("https://mcp.atlassian.com/v1/mcp").unwrap();
        assert_eq!(
            assumed.authorization_servers,
            vec!["https://mcp.atlassian.com".to_owned()]
        );
        assert!(
            assumed.scopes_supported.is_empty(),
            "nothing was read, so nothing is claimed about scopes"
        );
        assert!(super::resource_as_issuer("not a url").is_none());
    }

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
