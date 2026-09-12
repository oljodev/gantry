//! `cargo xtask probe-connectors` (docs/plan/17 §5): ask every catalogued server what it is, and
//! check the manifest against the answer.
//!
//! A catalogue entry is a set of claims about somebody else's server — this is its auth shape,
//! these are its tools, this one is destructive. Claims about a system nobody here controls go
//! stale, and the way they go stale is silent: a vendor retires an endpoint, adds a tool that
//! deletes things, or changes how it wants to be signed into, and the first person to find out is
//! a user whose connector stopped working or, worse, worked too well. This asks instead.
//!
//! Three ways it runs, and the middle one is the point:
//!
//! - plain, while writing a batch: probe the network, rewrite the fixtures, print what is wrong;
//! - `--offline`, in CI on every push: no network at all, so it checks each manifest against the
//!   fixture that was committed with it. Every assertion below still runs, which makes a manifest
//!   edit that contradicts the recorded server a failing build rather than a discovery;
//! - `--spawn`, to start a local server in a scratch directory and record its tools the same way.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, bail};
use serde_json::{Value, json};

use gantry_connectors::manifest::{Auth, Manifest, Runtime};

/// What one server answered, as it is committed beside the manifest.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Fixture {
    /// The protocol version the server negotiated, when it let us in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol_version: Option<String>,
    /// What `initialize` answered without a credential: `200`, `401`, and so on.
    pub status: u16,
    /// Present when the server asked us to sign in: what its metadata offers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthShape>,
    #[serde(default)]
    pub tools: Vec<Tool>,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AuthShape {
    pub issuer: String,
    /// The server runs a registration endpoint: a client can be created on the spot.
    pub dynamic_registration: bool,
    /// The server accepts a client-id metadata document instead of a registration.
    pub client_id_metadata_document: bool,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
}

pub fn run(root: &Path, offline: bool, spawn: bool) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting a runtime for the probe")?;
    runtime.block_on(probe_all(root, offline, spawn))
}

async fn probe_all(root: &Path, offline: bool, spawn: bool) -> anyhow::Result<()> {
    let dir = root.join("desktop/connectors");
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("gantry-probe")
        .build()?;
    let mut folders: Vec<PathBuf> = fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.join("manifest.json").is_file())
        .collect();
    folders.sort();

    let mut problems: Vec<String> = Vec::new();
    let mut probed = 0usize;
    for folder in &folders {
        let manifest = Manifest::parse(&fs::read_to_string(folder.join("manifest.json"))?)?;
        let id = manifest.id.clone();
        let fixture_path = folder.join("fixtures/tools.json");

        let fixture = match (&manifest.runtime, offline) {
            // A native connector is this repository's own code; its tools are in a Rust file and
            // a probe would be asking ourselves.
            (Runtime::Native { .. }, _) => continue,
            (_, true) => match fs::read_to_string(&fixture_path) {
                Ok(text) => serde_json::from_str(&text)
                    .with_context(|| format!("{id}: reading the recorded fixture"))?,
                Err(_) => {
                    problems.push(format!(
                        "{id}: no fixtures/tools.json — run `cargo xtask probe-connectors` \
                         with the network and commit what it records"
                    ));
                    continue;
                }
            },
            (Runtime::McpRemote { url, headers }, false) => {
                probed += 1;
                println!("probing {id} at {url}");
                // One vendor being down is not a reason to learn nothing about the other fifty,
                // and with a catalogue this size there is usually one. It is still a problem —
                // the run fails at the end — but every other fixture is written first.
                match remote(&http, url, headers).await {
                    Ok(fixture) => fixture,
                    Err(err) => {
                        problems.push(format!("{id}: {err:#}"));
                        continue;
                    }
                }
            }
            (Runtime::McpStdio { .. }, false) => {
                probed += 1;
                println!("probing {id} (local process)");
                match stdio(&http, &manifest, folder, spawn).await {
                    Ok(fixture) => fixture,
                    Err(err) => {
                        problems.push(format!("{id}: {err}"));
                        continue;
                    }
                }
            }
        };

        problems.extend(check(&manifest, &fixture));
        for line in suggestions(&fixture, &manifest) {
            println!("  {id}: {line}");
        }
        if !offline {
            fs::create_dir_all(folder.join("fixtures"))?;
            let mut text = serde_json::to_string_pretty(&fixture)?;
            text.push('\n');
            fs::write(&fixture_path, text)
                .with_context(|| format!("writing {}", fixture_path.display()))?;
        }
    }

    if problems.is_empty() {
        println!(
            "{} connectors checked{}",
            folders.len(),
            if offline {
                " against their recorded fixtures".to_owned()
            } else {
                format!(", {probed} probed")
            }
        );
        return Ok(());
    }
    for problem in &problems {
        eprintln!("  {problem}");
    }
    bail!("{} problem(s)", problems.len())
}

/// The manifest's claims, against what the server said. These run in both modes, which is what
/// makes the fixture worth committing: a manifest edited on a train still meets the real server.
fn check(manifest: &Manifest, fixture: &Fixture) -> Vec<String> {
    let id = &manifest.id;
    let mut problems = Vec::new();

    match (&manifest.auth, fixture.status, &fixture.auth) {
        (Auth::None, 401, _) => problems.push(format!(
            "{id}: the manifest says no account is needed, and the server answered 401"
        )),
        (Auth::Oauth2 { .. }, 200, None) => problems.push(format!(
            "{id}: the manifest says OAuth, and the server let us in without a credential"
        )),
        (Auth::Oauth2 { registration, .. }, _, Some(shape)) => {
            for mode in registration {
                // The four the schema allows (03 §7). The last two need nothing from the
                // server — a client id the user pastes in, or one shipped with the manifest —
                // so there is nothing to disagree with.
                let offered = match mode.as_str() {
                    "dcr" => shape.dynamic_registration,
                    "cimd" => shape.client_id_metadata_document,
                    _ => true,
                };
                if !offered {
                    problems.push(format!(
                        "{id}: auth.registration lists `{mode}`, which {} does not offer",
                        shape.issuer
                    ));
                }
            }
        }
        _ => {}
    }

    // An override for a tool that is not there is either a typo or a tool the server removed,
    // and both mean the tier it was protecting is no longer applied to anything.
    let names: BTreeSet<&str> = fixture.tools.iter().map(|t| t.name.as_str()).collect();
    if !fixture.tools.is_empty() {
        for name in manifest.tool_overrides.keys() {
            if !names.contains(name.as_str()) {
                problems.push(format!(
                    "{id}: tool_overrides names `{name}`, which the server does not offer"
                ));
            }
        }
    }
    problems
}

/// 17 §6, printed rather than applied: where the name says what a tool does and the manifest
/// does not already say otherwise, say which tier the rules would pick. The reviewer decides —
/// the last row of that table is a judgement no regex makes — so this never edits a manifest.
fn suggestions(fixture: &Fixture, manifest: &Manifest) -> Vec<String> {
    const READ: [&str; 6] = ["list_", "get_", "search_", "read_", "describe_", "fetch_"];
    const WRITE: [&str; 9] = [
        "create_", "update_", "add_", "set_", "post_", "send_", "merge_", "deploy_", "publish_",
    ];
    const DESTRUCTIVE: [&str; 8] = [
        "delete_",
        "remove_",
        "drop_",
        "revoke_",
        "cancel_",
        "refund_",
        "transfer_",
        "pay_",
    ];
    let mut lines = Vec::new();
    if let (Auth::Oauth2 { registration, .. }, Some(shape)) = (&manifest.auth, &fixture.auth) {
        for (mode, offered) in [
            ("cimd", shape.client_id_metadata_document),
            ("dcr", shape.dynamic_registration),
        ] {
            if offered && !registration.iter().any(|m| m == mode) {
                lines.push(format!(
                    "{} offers `{mode}`, which auth.registration does not list",
                    shape.issuer
                ));
            }
        }
    }
    for tool in &fixture.tools {
        if manifest.tool_overrides.contains_key(&tool.name) {
            continue;
        }
        let name = tool.name.to_ascii_lowercase();
        // A name that reads as a read, and a name that reads as nothing in particular, both
        // keep `risk.default_tool_tier`; only the two that raise the bar are worth printing.
        let _ = READ;
        let tier = if DESTRUCTIVE.iter().any(|p| name.starts_with(p)) {
            Some("destructive, always_confirm")
        } else if WRITE.iter().any(|p| name.starts_with(p)) {
            Some("write_external")
        } else {
            None
        };
        if let Some(tier) = tier {
            lines.push(format!("`{}` reads as {tier}", tool.name));
        }
    }
    lines
}

// ---------------------------------------------------------------------------------------------
// Remote servers
// ---------------------------------------------------------------------------------------------

const PROTOCOL: &str = "2026-07-28";

/// The modern revision is stateless: there is no handshake, and one `tools/list` is the whole
/// conversation — but every request has to carry an envelope, and the server checks that the
/// headers and the body agree. Cloudflare's documentation server says so in as many words when
/// they do not, which is how this was written.
fn envelope(method: &str, id: u32) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": {
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": PROTOCOL,
                "io.modelcontextprotocol/clientCapabilities": {},
            },
        },
    })
}

/// Ask for the tool list, the modern way first and the legacy handshake after. A server that
/// wants a credential says so on the first request either way, which is the answer the probe is
/// really after: what shape of sign-in does this server ask for.
async fn remote(
    http: &reqwest::Client,
    url: &str,
    headers: &BTreeMap<String, String>,
) -> anyhow::Result<Fixture> {
    let post = |method: &'static str, modern: bool, body: Value, session: Option<String>| {
        let mut request = http
            .post(url)
            .header("Accept", "application/json, text/event-stream");
        if modern {
            request = request
                .header("MCP-Protocol-Version", PROTOCOL)
                .header("Mcp-Method", method);
        }
        if let Some(id) = session {
            request = request.header("Mcp-Session-Id", id);
        }
        for (name, value) in headers {
            // A header whose value is a placeholder for a credential is exactly what a probe
            // has not got; sending the placeholder text is worse than sending nothing.
            if !value.contains("${") {
                request = request.header(name, value);
            }
        }
        request.json(&body).send()
    };

    let response = post("tools/list", true, envelope("tools/list", 1), None)
        .await
        .with_context(|| format!("POST {url}"))?;
    let status = response.status().as_u16();
    if status == 401 || status == 403 {
        let challenge = response
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        return Ok(Fixture {
            status,
            auth: auth_shape(http, url, challenge.as_deref()).await,
            ..Fixture::default()
        });
    }
    let answer = rpc_body(response).await?;
    if let Some(tools) = tools_of(&answer) {
        return Ok(Fixture {
            protocol_version: Some(PROTOCOL.to_owned()),
            status,
            // Listing tools to anyone is not the same as letting anyone call them: Railway and
            // BigQuery hand out the whole list unauthenticated and refuse every call until you
            // sign in. The protected-resource document is what says so, and recording it is what
            // stops the manifest's `oauth2` reading as a contradiction of a 200.
            auth: auth_shape(http, url, None).await,
            tools,
        });
    }

    // Older revisions want the handshake, and reject the modern version header outright, so the
    // fallback sends neither it nor the envelope.
    let response = post(
        "initialize",
        false,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "gantry-probe", "version": env!("CARGO_PKG_VERSION") },
            },
        }),
        None,
    )
    .await?;
    let status = response.status().as_u16();
    if status == 401 || status == 403 {
        let challenge = response
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        return Ok(Fixture {
            status,
            auth: auth_shape(http, url, challenge.as_deref()).await,
            ..Fixture::default()
        });
    }
    let session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let handshake = rpc_body(response).await?;
    let protocol_version = handshake
        .pointer("/result/protocolVersion")
        .and_then(Value::as_str)
        .map(str::to_owned);
    // The notification is what tells a spec-following server the handshake is finished; some
    // refuse `tools/list` without it, and none mind receiving it.
    let _ = post(
        "notifications/initialized",
        false,
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        session.clone(),
    )
    .await;
    let listed = post(
        "tools/list",
        false,
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        session,
    )
    .await?;
    let listed = rpc_body(listed).await?;
    Ok(Fixture {
        protocol_version,
        status,
        auth: auth_shape(http, url, None).await,
        tools: tools_of(&listed).unwrap_or_default(),
    })
}

/// The tools out of a `tools/list` answer, or nothing when the answer was an error — which is
/// how the modern attempt says "try the handshake instead".
fn tools_of(body: &Value) -> Option<Vec<Tool>> {
    let list = body.pointer("/result/tools")?.as_array()?;
    let mut tools: Vec<Tool> = list
        .iter()
        .map(|t| Tool {
            name: t["name"].as_str().unwrap_or_default().to_owned(),
            description: t["description"].as_str().unwrap_or_default().to_owned(),
            input_schema: t.get("inputSchema").cloned(),
        })
        .collect();
    // Recorded in a fixed order, so a re-probe that found nothing new is an empty diff.
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    Some(tools)
}

/// A Streamable HTTP server may answer JSON or a one-event SSE stream; both carry the same
/// JSON-RPC object, so this returns it either way.
async fn rpc_body(response: reqwest::Response) -> anyhow::Result<Value> {
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let text = response.text().await?;
    if !content_type.starts_with("text/event-stream") {
        return Ok(serde_json::from_str(&text).unwrap_or(Value::Null));
    }
    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data:")
            && let Ok(value) = serde_json::from_str::<Value>(data.trim())
            && value.get("result").is_some()
        {
            return Ok(value);
        }
    }
    Ok(Value::Null)
}

/// What the server's own metadata says about signing in — the two facts that decide whether a
/// user has to go and make a client id by hand (03 §7).
async fn auth_shape(
    http: &reqwest::Client,
    resource: &str,
    challenge: Option<&str>,
) -> Option<AuthShape> {
    use gantry_connectors::auth::discovery;
    let metadata_url = challenge
        .and_then(discovery::resource_metadata_url)
        .or_else(|| discovery::protected_resource_url(resource))?;
    // The same fallback the app uses: a server with no protected-resource document is assumed to
    // be its own issuer, so the fixture records the shape instead of recording nothing.
    let protected = match discovery::protected_resource(http, &metadata_url).await {
        Ok(document) if !document.authorization_servers.is_empty() => document,
        _ => discovery::resource_as_issuer(resource)?,
    };
    let issuer = protected.authorization_servers.first()?.clone();
    let server = discovery::auth_server(http, &issuer).await.ok()?;
    Some(AuthShape {
        dynamic_registration: server.registration_endpoint.is_some(),
        client_id_metadata_document: server.client_id_metadata_document_supported,
        scopes: server.scopes_supported.clone(),
        issuer,
    })
}

// ---------------------------------------------------------------------------------------------
// Local servers
// ---------------------------------------------------------------------------------------------

/// A local server is a package before it is a process, so the cheap half of the check is asking
/// the registry whether the pinned version is still published — which is the failure that
/// actually happens, and one that needs no runtime installed to find.
async fn stdio(
    http: &reqwest::Client,
    manifest: &Manifest,
    folder: &Path,
    spawn: bool,
) -> anyhow::Result<Fixture> {
    let Runtime::McpStdio { command, args, .. } = &manifest.runtime else {
        unreachable!("called for a stdio manifest")
    };
    if let Some((name, version)) = package(command, args) {
        let url = format!("https://registry.npmjs.org/{name}");
        let response = http.get(&url).send().await?;
        if !response.status().is_success() {
            bail!(
                "npm does not know the package `{name}` ({})",
                response.status()
            );
        }
        let body: Value = response.json().await?;
        if let Some(version) = version
            && body.pointer(&format!("/versions/{version}")).is_none()
        {
            bail!("npm has no `{name}@{version}` any more");
        }
    }
    if let Some((name, version)) = python_package(command, args) {
        let url = format!("https://pypi.org/pypi/{name}/json");
        let response = http.get(&url).send().await?;
        if !response.status().is_success() {
            bail!(
                "PyPI does not know the package `{name}` ({})",
                response.status()
            );
        }
        let body: Value = response.json().await?;
        if let Some(version) = version
            && body.pointer(&format!("/releases/{version}")).is_none()
        {
            bail!("PyPI has no `{name}=={version}` any more");
        }
    }
    if !spawn {
        // Without `--spawn` the tools are whatever the last spawn recorded; saying so beats
        // overwriting a real tool list with an empty one.
        let recorded = fs::read_to_string(folder.join("fixtures/tools.json")).unwrap_or_default();
        return Ok(serde_json::from_str(&recorded).unwrap_or(Fixture {
            status: 0,
            ..Fixture::default()
        }));
    }
    bail!("--spawn needs the runtime check of 03 §11 step 1, which lands with B6")
}

/// The npm package a stdio command runs, with its pinned version: `npx -y pkg@1.2.3` is the
/// shape every catalogued local server uses.
fn package(command: &str, args: &[String]) -> Option<(String, Option<String>)> {
    if !matches!(command, "npx" | "npm" | "pnpm" | "bunx") {
        return None;
    }
    let spec = args.iter().find(|a| !a.starts_with('-'))?;
    // A scope keeps its leading `@`, so the version separator is the *last* `@` past position 0.
    match spec.rfind('@') {
        Some(at) if at > 0 => Some((spec[..at].to_owned(), Some(spec[at + 1..].to_owned()))),
        _ => Some((spec.clone(), None)),
    }
}

/// The PyPI distribution a `uvx` command runs, with its pinned version: `uvx name==1.2.3`, or
/// `uv tool run name`. The npm half of this asks npm; a Python server's package has to be asked
/// of PyPI, and a local server nobody can spawn here is otherwise checked against nothing at all.
fn python_package(command: &str, args: &[String]) -> Option<(String, Option<String>)> {
    if !matches!(command, "uvx" | "uv" | "pipx") {
        return None;
    }
    let spec = args
        .iter()
        .filter(|a| !a.starts_with('-'))
        // `uv tool run <name>` and `pipx run <name>`: the subcommands are not the package.
        .find(|a| !matches!(a.as_str(), "tool" | "run"))?;
    match spec.split_once("==") {
        Some((name, version)) => Some((name.to_owned(), Some(version.to_owned()))),
        None => Some((spec.clone(), None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_python_server_is_asked_of_pypi_however_uv_is_spelled() {
        assert_eq!(
            python_package("uvx", &["elevenlabs-mcp".into()]),
            Some(("elevenlabs-mcp".to_owned(), None))
        );
        assert_eq!(
            python_package("uv", &["tool".into(), "run".into(), "thing==1.2.3".into()]),
            Some(("thing".to_owned(), Some("1.2.3".to_owned())))
        );
        assert_eq!(python_package("npx", &["-y".into(), "thing".into()]), None);
    }

    #[test]
    fn a_scoped_package_keeps_its_scope_and_gives_up_its_version() {
        assert_eq!(
            package("npx", &["-y".into(), "@scope/server@1.2.3".into()]),
            Some(("@scope/server".to_owned(), Some("1.2.3".to_owned())))
        );
        assert_eq!(
            package("npx", &["server".into()]),
            Some(("server".to_owned(), None))
        );
        assert_eq!(package("uvx", &["thing".into()]), None);
    }

    #[test]
    fn an_sse_answer_is_read_like_a_json_one() {
        // Not a network test: the shape of what a Streamable HTTP server sends back.
        let text = ": ping\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}\n\n";
        let mut found = None;
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data:")
                && let Ok(value) = serde_json::from_str::<Value>(data.trim())
                && value.get("result").is_some()
            {
                found = Some(value);
            }
        }
        assert!(found.is_some());
    }

    #[test]
    fn a_manifest_that_promises_no_account_is_caught_by_a_401() {
        let manifest = Manifest::parse(
            r#"{"manifest_version":"1","id":"x","name":"X","description":"d","version":"1.0.0",
                "icon":"icon.svg","category":"web","publisher":{"name":"p"},
                "runtime":{"kind":"mcp-remote","url":"https://example.test/mcp"},
                "auth":{"type":"none"},"risk":{"network":"none","local_system":"none","default_tool_tier":"read"}}"#,
        )
        .unwrap();
        let fixture = Fixture {
            status: 401,
            ..Fixture::default()
        };
        assert_eq!(check(&manifest, &fixture).len(), 1);
    }

    #[test]
    fn an_override_for_a_tool_the_server_does_not_have_is_a_tier_protecting_nothing() {
        let manifest = Manifest::parse(
            r#"{"manifest_version":"1","id":"x","name":"X","description":"d","version":"1.0.0",
                "icon":"icon.svg","category":"web","publisher":{"name":"p"},
                "runtime":{"kind":"mcp-remote","url":"https://example.test/mcp"},
                "auth":{"type":"none"},"risk":{"network":"none","local_system":"none","default_tool_tier":"read"},
                "tool_overrides":{"delete_everything":{"risk":"destructive"}}}"#,
        )
        .unwrap();
        let fixture = Fixture {
            status: 200,
            tools: vec![Tool {
                name: "list_things".into(),
                ..Tool::default()
            }],
            ..Fixture::default()
        };
        let problems = check(&manifest, &fixture);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("delete_everything"));
    }
}
