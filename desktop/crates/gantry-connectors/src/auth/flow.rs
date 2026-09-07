//! The authorization code flow with PKCE (docs/plan/03 §7 steps 3–4): a loopback listener on a
//! fixed port set, the browser hand-off, and the token exchange and refresh.
//!
//! The listener is bound *before* the URL is built, because the redirect URI has to name the
//! port that will actually be listening, and it is closed the moment one request arrives.

use std::time::Duration;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use url::Url;

use crate::auth::{AuthError, discovery::AuthServer};

/// The ports the client-metadata document registers as redirect URIs (03 §7). Nothing outside
/// this set is ever a valid redirect, so a server cannot be talked into sending a code anywhere
/// else.
pub const PORTS: [u16; 5] = [17321, 17322, 17323, 17324, 17325];

/// How long the browser half may take before the attempt is abandoned.
const WAIT: Duration = Duration::from_secs(5 * 60);

/// One authorization attempt in flight.
pub struct Pending {
    listener: TcpListener,
    pub redirect_uri: String,
    pub authorize_url: String,
    state: String,
    verifier: String,
    issuer: String,
}

/// The set of tokens one authorization produces. Stored as a single vault credential (06 §5).
#[derive(Debug, Clone, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub token_type: Option<String>,
}

impl TokenSet {
    /// When the access token stops working, in epoch milliseconds. A server that says nothing
    /// gets an hour, which is what most of them mean.
    #[must_use]
    pub fn expires_at(&self) -> i64 {
        let seconds = self.expires_in.unwrap_or(3600).max(0);
        gantry_core::now_ms() + seconds * 1000
    }

    /// The `Authorization` header value.
    #[must_use]
    pub fn header(&self) -> String {
        let scheme = self.token_type.as_deref().unwrap_or("Bearer");
        // Servers write "bearer", "Bearer" and "BEARER"; the header is case-insensitive but
        // some servers are not, so the canonical spelling goes out.
        let scheme = if scheme.eq_ignore_ascii_case("bearer") {
            "Bearer"
        } else {
            scheme
        };
        format!("{scheme} {}", self.access_token)
    }
}

/// Binds a loopback port and builds the authorization URL for it.
pub async fn begin(
    server: &AuthServer,
    client_id: &str,
    scopes: &[String],
    resource: Option<&str>,
) -> Result<Pending, AuthError> {
    let listener = bind().await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let state = random(32);
    let verifier = random(64);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let mut url = Url::parse(&server.authorization_endpoint)
        .map_err(|e| AuthError::Discovery(format!("bad authorization endpoint: {e}")))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("response_type", "code");
        q.append_pair("client_id", client_id);
        q.append_pair("redirect_uri", &redirect_uri);
        q.append_pair("state", &state);
        q.append_pair("code_challenge", &challenge);
        q.append_pair("code_challenge_method", "S256");
        if !scopes.is_empty() {
            q.append_pair("scope", &scopes.join(" "));
        }
        // RFC 8707: say which resource the token is for, so a stolen token is useless
        // elsewhere on servers that honour it.
        if let Some(resource) = resource {
            q.append_pair("resource", resource);
        }
    }
    Ok(Pending {
        listener,
        redirect_uri,
        authorize_url: url.to_string(),
        state,
        verifier,
        issuer: server.issuer.clone(),
    })
}

impl Pending {
    /// Waits for the browser to come back, checks `state` and `iss`, and redeems the code.
    pub async fn complete(
        self,
        http: &reqwest::Client,
        server: &AuthServer,
        client_id: &str,
        resource: Option<&str>,
    ) -> Result<TokenSet, AuthError> {
        let query = tokio::time::timeout(WAIT, accept(&self.listener))
            .await
            .map_err(|_| AuthError::Timeout)??;

        if let Some(error) = query.get("error") {
            let detail = query
                .get("error_description")
                .cloned()
                .unwrap_or_else(|| error.clone());
            return Err(AuthError::Denied(detail));
        }
        // The state check is what stops a code from another session being redeemed here.
        if query.get("state").map(String::as_str) != Some(self.state.as_str()) {
            return Err(AuthError::Denied(
                "the redirect did not match this attempt".into(),
            ));
        }
        // RFC 9207: when the server names itself, it must be the server we started with.
        if let Some(iss) = query.get("iss")
            && iss != &self.issuer
        {
            return Err(AuthError::Denied(format!(
                "the code came back from {iss}, not {}",
                self.issuer
            )));
        }
        let code = query
            .get("code")
            .ok_or_else(|| AuthError::Denied("the redirect carried no code".into()))?;

        let mut form = vec![
            ("grant_type", "authorization_code".to_owned()),
            ("code", code.clone()),
            ("redirect_uri", self.redirect_uri.clone()),
            ("client_id", client_id.to_owned()),
            ("code_verifier", self.verifier.clone()),
        ];
        if let Some(resource) = resource {
            form.push(("resource", resource.to_owned()));
        }
        post_token(http, &server.token_endpoint, &form).await
    }
}

/// Exchanges a refresh token for a new set. A failure here means "Reconnect" (03 §7 step 4).
pub async fn refresh(
    http: &reqwest::Client,
    server: &AuthServer,
    client_id: &str,
    refresh_token: &str,
    resource: Option<&str>,
) -> Result<TokenSet, AuthError> {
    let mut form = vec![
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", refresh_token.to_owned()),
        ("client_id", client_id.to_owned()),
    ];
    if let Some(resource) = resource {
        form.push(("resource", resource.to_owned()));
    }
    post_token(http, &server.token_endpoint, &form).await
}

/// What a token endpoint said. An OAuth error is a *shape*, not a status: GitHub answers 200
/// with `{"error":"..."}`, and the device flow's "not yet" arrives the same way, so the code has
/// to be read rather than the status.
enum Answer {
    Token(Box<TokenSet>),
    Error {
        code: Option<String>,
        message: String,
    },
}

async fn post_token(
    http: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, String)],
) -> Result<TokenSet, AuthError> {
    match ask(http, endpoint, form).await? {
        Answer::Token(set) => Ok(*set),
        Answer::Error { message, .. } => Err(AuthError::Token(message)),
    }
}

async fn ask(
    http: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, String)],
) -> Result<Answer, AuthError> {
    let response = http
        .post(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(form)
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let answer = read(&body);
    // A failing status with nothing readable in it still has to say something.
    if let Answer::Error { code, message } = &answer
        && !status.is_success()
        && code.is_none()
        && message.is_empty()
    {
        return Ok(Answer::Error {
            code: None,
            message: format!("{status}: {}", body.chars().take(300).collect::<String>()),
        });
    }
    Ok(answer)
}

/// JSON or form-encoded, a token or an error, in one reading.
fn read(body: &str) -> Answer {
    if body.trim_start().starts_with('{') {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            return Answer::Error {
                code: None,
                message: String::new(),
            };
        };
        if let Some(code) = value.get("error").and_then(|v| v.as_str()) {
            return Answer::Error {
                message: value
                    .get("error_description")
                    .and_then(|v| v.as_str())
                    .unwrap_or(code)
                    .to_owned(),
                code: Some(code.to_owned()),
            };
        }
        return match serde_json::from_value::<TokenSet>(value) {
            Ok(set) => Answer::Token(Box::new(set)),
            Err(err) => Answer::Error {
                code: None,
                message: err.to_string(),
            },
        };
    }
    // GitHub answers form-encoded unless asked for JSON, and asking is not always enough.
    match form_token(body) {
        Ok(set) => Answer::Token(Box::new(set)),
        Err(err) => Answer::Error {
            code: None,
            message: err.to_string(),
        },
    }
}

/// What the user has to do for a device sign-in (RFC 8628): a code to type, and where.
#[derive(Debug, Clone)]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// The page with the code already filled in, when the server offers one.
    pub verification_uri_complete: Option<String>,
    pub interval: Duration,
    pub expires_in: Duration,
}

/// Asks the server for a code the user can type (RFC 8628 §3.1). This is the way in for a
/// server that will not take a client without a secret — GitHub, which is the reason it exists
/// here at all (03 §7).
pub async fn device_begin(
    http: &reqwest::Client,
    server: &AuthServer,
    client_id: &str,
    scopes: &[String],
) -> Result<DeviceStart, AuthError> {
    let endpoint = server
        .device_authorization_endpoint
        .as_deref()
        .ok_or_else(|| AuthError::Discovery("this server has no device endpoint".into()))?;
    let mut form = vec![("client_id", client_id.to_owned())];
    if !scopes.is_empty() {
        form.push(("scope", scopes.join(" ")));
    }
    let response = http
        .post(endpoint)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&form)
        .send()
        .await?;
    let body = response.text().await.unwrap_or_default();
    let value: serde_json::Value = if body.trim_start().starts_with('{') {
        serde_json::from_str(&body).map_err(|e| AuthError::Token(e.to_string()))?
    } else {
        let mut map = serde_json::Map::new();
        for (k, v) in url::form_urlencoded::parse(body.as_bytes()) {
            map.insert(k.into_owned(), serde_json::Value::String(v.into_owned()));
        }
        serde_json::Value::Object(map)
    };
    if let Some(code) = value.get("error").and_then(|v| v.as_str()) {
        let message = value
            .get("error_description")
            .and_then(|v| v.as_str())
            .unwrap_or(code);
        // The one failure worth explaining: the application exists but has not been allowed to
        // use this flow, which is a checkbox on the server's own settings page.
        if code == "device_flow_disabled" {
            return Err(AuthError::DeviceFlowDisabled);
        }
        return Err(AuthError::Token(message.to_owned()));
    }
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    let seconds = |key: &str, fallback: u64| {
        value
            .get(key)
            .and_then(|v| {
                v.as_u64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(fallback)
    };
    Ok(DeviceStart {
        device_code: text("device_code")
            .ok_or_else(|| AuthError::Token("the server sent no device code".into()))?,
        user_code: text("user_code")
            .ok_or_else(|| AuthError::Token("the server sent no user code".into()))?,
        verification_uri: text("verification_uri")
            .or_else(|| text("verification_url"))
            .ok_or_else(|| AuthError::Token("the server sent nowhere to go".into()))?,
        verification_uri_complete: text("verification_uri_complete"),
        interval: Duration::from_secs(seconds("interval", 5).clamp(1, 60)),
        expires_in: Duration::from_secs(seconds("expires_in", 900)),
    })
}

/// Polls until the user has approved, refused, or run out of time (RFC 8628 §3.4–3.5).
pub async fn device_wait(
    http: &reqwest::Client,
    server: &AuthServer,
    client_id: &str,
    start: &DeviceStart,
) -> Result<TokenSet, AuthError> {
    let deadline = tokio::time::Instant::now() + start.expires_in;
    let mut interval = start.interval;
    loop {
        tokio::time::sleep(interval).await;
        if tokio::time::Instant::now() >= deadline {
            return Err(AuthError::Timeout);
        }
        let form = vec![
            ("client_id", client_id.to_owned()),
            ("device_code", start.device_code.clone()),
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code".to_owned(),
            ),
        ];
        match ask(http, &server.token_endpoint, &form).await? {
            Answer::Token(set) => return Ok(*set),
            Answer::Error { code, message } => match code.as_deref() {
                Some("authorization_pending") => {}
                // The server is asking to be left alone for longer; obliging is the protocol.
                Some("slow_down") => interval += Duration::from_secs(5),
                Some("access_denied") => {
                    return Err(AuthError::Denied("the sign-in was refused".into()));
                }
                Some("expired_token") => return Err(AuthError::Timeout),
                _ => return Err(AuthError::Token(message)),
            },
        }
    }
}

/// `access_token=x&token_type=bearer&scope=repo` — the shape GitHub returns by default.
fn form_token(body: &str) -> Result<TokenSet, AuthError> {
    let parsed = url::form_urlencoded::parse(body.as_bytes());
    let mut set = TokenSet {
        access_token: String::new(),
        refresh_token: None,
        expires_in: None,
        scope: None,
        token_type: None,
    };
    let (mut error, mut description) = (None, None);
    for (key, value) in parsed {
        match key.as_ref() {
            "access_token" => set.access_token = value.into_owned(),
            "refresh_token" => set.refresh_token = Some(value.into_owned()),
            "expires_in" => set.expires_in = value.parse().ok(),
            "scope" => set.scope = Some(value.into_owned()),
            "token_type" => set.token_type = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            // The description is the half worth showing, and it may arrive after the code.
            "error_description" => description = Some(value.into_owned()),
            _ => {}
        }
    }
    if set.access_token.is_empty() {
        return Err(AuthError::Token(
            description
                .or(error)
                .unwrap_or_else(|| "the response carried no token".to_owned()),
        ));
    }
    Ok(set)
}

/// The first free port of the fixed set (03 §7). All five busy means something else is using
/// them; failing here is better than redirecting somewhere unregistered.
async fn bind() -> Result<TcpListener, AuthError> {
    for port in PORTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            return Ok(listener);
        }
    }
    Err(AuthError::NoPort)
}

/// Serves exactly one request and returns its query parameters.
async fn accept(
    listener: &TcpListener,
) -> Result<std::collections::HashMap<String, String>, AuthError> {
    loop {
        let (mut socket, _) = listener.accept().await?;
        let mut buf = vec![0_u8; 8192];
        let read = socket.read(&mut buf).await?;
        let request = String::from_utf8_lossy(&buf[..read]).to_string();
        let Some(target) = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
        else {
            continue;
        };
        // A browser asks for the icon too; only the callback carries the answer.
        if target.starts_with("/favicon") {
            let _ = socket
                .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
                .await;
            continue;
        }
        let url = Url::parse(&format!("http://127.0.0.1{target}"))
            .map_err(|e| AuthError::Denied(e.to_string()))?;
        let query: std::collections::HashMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        let _ = socket.write_all(PAGE.as_bytes()).await;
        let _ = socket.shutdown().await;
        return Ok(query);
    }
}

/// What the browser shows before the user switches back. Deliberately plain: it is served over
/// http on loopback and belongs to no theme.
const PAGE: &str = concat!(
    "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\nconnection: close\r\n\r\n",
    "<!doctype html><meta charset=utf-8><title>Gantry</title>",
    "<style>body{font:15px/1.5 system-ui,sans-serif;display:grid;place-items:center;",
    "height:100vh;margin:0;color:#1a1a1a;background:#fafaf9}p{color:#666}</style>",
    "<div><h1>Connected</h1><p>You can close this tab and go back to Gantry.</p></div>",
);

/// URL-safe random text of `bytes` entropy.
fn random(bytes: usize) -> String {
    let mut buf = vec![0_u8; bytes];
    getrandom::fill(&mut buf).expect("the system random source");
    URL_SAFE_NO_PAD.encode(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_encoded_token_is_read() {
        let set = form_token("access_token=gho_x&scope=repo%2Cread%3Aorg&token_type=bearer")
            .expect("a token");
        assert_eq!(set.access_token, "gho_x");
        assert_eq!(set.scope.as_deref(), Some("repo,read:org"));
        assert_eq!(set.header(), "Bearer gho_x");
    }

    #[test]
    fn an_error_in_a_form_response_is_an_error() {
        let err = form_token("error=bad_verification_code&error_description=expired")
            .expect_err("an error");
        assert!(err.to_string().contains("expired"), "{err}");
    }

    #[test]
    fn a_verifier_is_long_and_url_safe() {
        let v = random(64);
        assert!(v.len() >= 43 && v.len() <= 128, "{}", v.len());
        assert!(
            v.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }
}
