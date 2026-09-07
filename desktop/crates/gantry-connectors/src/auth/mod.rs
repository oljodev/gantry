//! OAuth for MCP servers (docs/plan/03 §7). Discovery and registration in `discovery`, the
//! code flow in `flow`, and here the one type that ties them together plus the decision of
//! which client id to use.

pub mod discovery;
pub mod flow;

pub use discovery::{AuthServer, ProtectedResource, RegisteredClient};
pub use flow::{PORTS, Pending, TokenSet};

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
    fn a_server_that_offers_neither_needs_a_client_id() {
        // This is GitHub, observed 2026-09-07.
        assert!(matches!(
            choose_client(None, None, &server(None, false)),
            Err(AuthError::NeedsClientId)
        ));
    }
}
