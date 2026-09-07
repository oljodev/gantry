//! OAuth for MCP servers (docs/plan/03 §7). Discovery and registration in `discovery`, the
//! code flow in `flow`, and here the one type that ties them together plus the decision of
//! which client id to use.

pub mod discovery;
pub mod flow;

pub use discovery::{AuthServer, ProtectedResource, RegisteredClient};
pub use flow::{DeviceStart, PORTS, Pending, TokenSet};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("discovery failed: {0}")]
    Discovery(String),
    #[error("this server does not register clients automatically, and no client id was given")]
    NeedsClientId,
    #[error("registration failed: {0}")]
    Registration(String),
    #[error("the sign-in was refused: {0}")]
    Denied(String),
    #[error("the token request failed: {0}")]
    Token(String),
    #[error("no loopback port was free (tried {:?})", PORTS)]
    NoPort,
    #[error("the sign-in was not finished in time")]
    Timeout,
    #[error(
        "this application has not been allowed to sign in with a code; tick \"Enable Device Flow\" in its settings"
    )]
    DeviceFlowDisabled,
    #[error("network: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Where a client id may come from, in the order §7 tries them.
#[derive(Debug, Clone)]
pub enum ClientSource {
    /// The manifest carries one, or the user typed one, or one was registered earlier.
    Preregistered(String),
    /// The server reads a metadata document instead of registering: the id is that URL.
    Cimd,
    /// Register now and remember the result per issuer.
    Dynamic,
}

/// The client-id metadata document Gantry publishes (14 §1). Used as the client id itself by
/// servers that support it, which saves a registration round trip and a stored client.
pub const CIMD_URL: &str = "https://id.oljo.dev/client-metadata.json";

/// Picks how to become a client of this server, given what the manifest allows and what the
/// server supports. A stored client id always wins: registering twice with the same issuer is
/// how rate limits are hit.
pub fn choose_client(
    stored: Option<&str>,
    manifest_client_id: Option<&str>,
    server: &AuthServer,
) -> Result<ClientSource, AuthError> {
    if let Some(id) = stored {
        return Ok(ClientSource::Preregistered(id.to_owned()));
    }
    if let Some(id) = manifest_client_id {
        return Ok(ClientSource::Preregistered(id.to_owned()));
    }
    if server.client_id_metadata_document_supported {
        return Ok(ClientSource::Cimd);
    }
    if server.registration_endpoint.is_some() {
        return Ok(ClientSource::Dynamic);
    }
    // GitHub lands here: no registration endpoint, no metadata document. The install dialog
    // asks for a client id, or the user picks the token instead.
    Err(AuthError::NeedsClientId)
}

/// Whether to sign in with a code the user types instead of a redirect (RFC 8628).
///
/// The rule is what the server says about itself. A server that lists `none` among its token
/// endpoint's authentication methods takes a client with no secret, so the redirect flow with
/// PKCE works — Cloudflare. A server that says nothing wants a secret a desktop application
/// cannot keep, and if it offers a device endpoint that is the honest way in — GitHub, which
/// answers `incorrect_client_credentials` to a PKCE exchange without one.
#[must_use]
pub fn prefers_device(server: &AuthServer) -> bool {
    server.device_authorization_endpoint.is_some()
        && !server
            .token_endpoint_auth_methods_supported
            .iter()
            .any(|m| m == "none")
}

/// The redirect URIs Gantry can serve, for a registration request.
#[must_use]
pub fn redirect_uris() -> Vec<String> {
    PORTS
        .iter()
        .map(|port| format!("http://127.0.0.1:{port}/callback"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(registration: Option<&str>, cimd: bool) -> AuthServer {
        AuthServer {
            issuer: "https://example.test".into(),
            authorization_endpoint: "https://example.test/authorize".into(),
            token_endpoint: "https://example.test/token".into(),
            registration_endpoint: registration.map(str::to_owned),
            code_challenge_methods_supported: vec!["S256".into()],
            scopes_supported: Vec::new(),
            device_authorization_endpoint: None,
            token_endpoint_auth_methods_supported: vec!["none".into()],
            client_id_metadata_document_supported: cimd,
            authorization_response_iss_parameter_supported: true,
        }
    }

    #[test]
    fn a_stored_client_is_reused_before_anything_is_registered() {
        let source = choose_client(
            Some("stored"),
            Some("manifest"),
            &server(Some("/reg"), true),
        );
        assert!(matches!(source, Ok(ClientSource::Preregistered(id)) if id == "stored"));
    }

    #[test]
    fn a_metadata_document_beats_registering() {
        assert!(matches!(
            choose_client(None, None, &server(Some("/reg"), true)),
            Ok(ClientSource::Cimd)
        ));
    }

    #[test]
    fn registration_is_used_when_it_is_offered() {
        assert!(matches!(
            choose_client(None, None, &server(Some("/reg"), false)),
            Ok(ClientSource::Dynamic)
        ));
    }

    #[test]
    fn a_public_client_server_uses_the_redirect() {
        // Cloudflare: `none` is listed, so PKCE without a secret is enough.
        assert!(!prefers_device(&server(Some("/reg"), false)));
    }

    #[test]
    fn a_server_that_wants_a_secret_signs_in_with_a_code() {
        // GitHub: no `none`, but a device endpoint, so the code flow is the way in.
        let mut github = server(None, false);
        github.token_endpoint_auth_methods_supported = Vec::new();
        github.device_authorization_endpoint = Some("https://github.com/login/device/code".into());
        assert!(prefers_device(&github));
    }

    #[test]
    fn a_server_that_offers_neither_needs_a_client_id() {
        // This is GitHub, observed 2026-09-07.
        assert!(matches!(
            choose_client(None, None, &server(None, false)),
            Err(AuthError::NeedsClientId)
        ));
    }
}
