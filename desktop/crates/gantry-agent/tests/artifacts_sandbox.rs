//! The automated sandbox conformance test (docs/plan/13 §5, M13): one test per rule the
//! sandbox claims to enforce, each one a hostile artifact that tries the thing and is denied.
//!
//! §5 is enforced in two different ways, so this file tests it in two different ways.
//!
//! **What the engine enforces.** The CSP and the iframe's `sandbox` attribute are browser
//! rules. The engine keeps them; what can quietly stop working is the declaration — a
//! directive dropped while editing the string, a flag added to make one artifact work. So
//! `desktop/artifact-runtime/scripts/sandbox-conformance.py` mounts each hostile artifact from
//! `desktop/artifact-runtime/src/conformance/cases.json` on its own, in a real WebKitGTK view,
//! behind the very CSP and sandbox attribute the app ships — read out of the frontend source by
//! this test and handed to the harness — and reports whether the rule held. Every `engine_*`
//! test below reads one case's verdict out of that run.
//!
//! WebKitGTK is the engine the Linux app renders artifacts in; Chromium and WKWebView are still
//! covered by running the app. Where no engine is available the `engine_*` tests say so and
//! pass; set `GANTRY_SANDBOX_ENGINE=1` to make a missing engine a failure instead, which is
//! what a Linux CI job should do. `GANTRY_SANDBOX_ENGINE=0` skips the run entirely.
//!
//! **What Gantry enforces.** The bridge — the nonce, the source and origin checks, the message
//! allowlist, the console budget, the `https:` gate on `open_url`, the `unsupported` answer to
//! the reserved namespaces, the loop guard — is Gantry's own code. Those rules are held to
//! `gantry_agent::artifacts::sandbox`, which states §5 once in Rust; the `declares_*` tests
//! check that every shipped declaration still says what it says. They need no engine and run
//! everywhere.
//!
//! Two of §5's claims do not hold yet. Each has a test, each test is `#[ignore]`d, and each
//! says what it proves: `an_html_artifacts_inline_script_is_loop_guarded` (only `react` is
//! compiled, so an `html` artifact's scripts are never instrumented) and
//! `the_toolbar_can_stop_a_running_artifact` (hang risk 2's Stop was never built). Remove the
//! `#[ignore]` when the rule is made true; the test is already written.

use std::{
    collections::{BTreeSet, HashMap},
    env, fs,
    path::PathBuf,
    process::Command,
    sync::OnceLock,
};

use gantry_agent::artifacts::sandbox;
use serde_json::{Value, json};

// ---------------------------------------------------------------- the shipped declarations

fn desktop() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = desktop().join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The text between `open` and the next `close`, starting the search at `anchor`.
fn between(text: &str, anchor: &str, open: &str, close: &str) -> String {
    let from = text
        .find(anchor)
        .unwrap_or_else(|| panic!("`{anchor}` is gone from the source this test reads"));
    let rest = &text[from + anchor.len()..];
    let start = rest
        .find(open)
        .unwrap_or_else(|| panic!("no `{open}` after `{anchor}`"))
        + open.len();
    let end = rest[start..]
        .find(close)
        .unwrap_or_else(|| panic!("no closing `{close}` after `{anchor}`"));
    rest[start..start + end].to_owned()
}

fn frontend_bridge() -> String {
    read("frontend/src/features/artifacts/bridge.ts")
}

fn sandbox_host() -> String {
    read("frontend/src/features/artifacts/renderers/SandboxHost.tsx")
}

fn runtime_index() -> String {
    read("artifact-runtime/index.html")
}

fn app_index() -> String {
    read("frontend/index.html")
}

fn runtime_bridge() -> String {
    read("artifact-runtime/src/bridge.ts")
}

/// The policy the app injects into an `html` artifact.
fn shipped_csp() -> String {
    between(&frontend_bridge(), "export const CSP =", "\"", "\"")
}

/// The policy the runtime document carries.
fn runtime_csp() -> String {
    between(
        &runtime_index(),
        "http-equiv=\"Content-Security-Policy\"",
        "content=\"",
        "\"",
    )
}

/// The policy the app document itself carries, which is the one that governs where an
/// artifact's frame may go.
fn shipped_embedder_csp() -> String {
    between(
        &app_index(),
        "http-equiv=\"Content-Security-Policy\"",
        "content=\"",
        "\"",
    )
}

fn shipped_sandbox_flags() -> String {
    between(&frontend_bridge(), "export const SANDBOX_FLAGS =", "'", "'")
}

fn shipped_referrer_policy() -> String {
    between(&sandbox_host(), "referrerPolicy=", "\"", "\"")
}

/// The source with its comments taken out, so that naming a flag in prose — which
/// `SandboxHost.tsx` does, to say why it is absent — is not read as granting it. Block
/// comments and whole-line `//` comments are where every such mention lives; a grant never is.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("/*") {
        out.push_str(&rest[..at]);
        rest = match rest[at..].find("*/") {
            Some(end) => &rest[at + end + 2..],
            None => "",
        };
    }
    out.push_str(rest);
    let mut html = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(at) = rest.find("<!--") {
        html.push_str(&rest[..at]);
        rest = match rest[at..].find("-->") {
            Some(end) => &rest[at + end + 3..],
            None => "",
        };
    }
    html.push_str(rest);
    html.lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with('*')
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `16384` as TypeScript writes it in the runtime, with the numeric separator.
fn grouped(n: usize) -> String {
    let plain = n.to_string();
    let mut out = String::new();
    for (i, c) in plain.chars().enumerate() {
        if i > 0 && (plain.len() - i) % 3 == 0 {
            out.push('_');
        }
        out.push(c);
    }
    out
}

// -------------------------------------------------------------------- the real-engine run

struct Verdict {
    rule: String,
    verdict: String,
    detail: String,
}

enum Engine {
    Ran(HashMap<String, Verdict>),
    Unavailable(String),
    Skipped,
}

enum Requirement {
    Auto,
    Require,
    Skip,
}

fn requirement() -> Requirement {
    match env::var("GANTRY_SANDBOX_ENGINE")
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "auto" => Requirement::Auto,
        "0" | "off" | "skip" | "no" => Requirement::Skip,
        _ => Requirement::Require,
    }
}

fn harness_script() -> PathBuf {
    desktop().join("artifact-runtime/scripts/sandbox-conformance.py")
}

/// The configuration the harness runs under: the strings the app actually ships, so a
/// weakened declaration is a weakened run.
fn shipped_config() -> Value {
    json!({
        "csp": shipped_csp(),
        "sandbox_flags": shipped_sandbox_flags(),
        "referrer_policy": shipped_referrer_policy(),
        "parent_csp": shipped_embedder_csp(),
    })
}

fn run_harness(config: &Value, extra: &[&str]) -> Result<HashMap<String, Verdict>, String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let path = dir.path().join("conformance.json");
    fs::write(
        &path,
        serde_json::to_vec_pretty(config).expect("config is JSON"),
    )
    .map_err(|e| e.to_string())?;
    let output = Command::new("python3")
        .arg(harness_script())
        .arg("--config")
        .arg(&path)
        .arg("--quiet")
        .args(extra)
        .output()
        .map_err(|e| format!("python3 could not be started: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.code() == Some(2) {
        return Err(stderr.trim().to_owned());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Exit code 1 only means some case was open, which is what the tests are here to say.
    let line = stdout
        .lines()
        .find_map(|l| l.strip_prefix("GANTRY_CONFORMANCE "))
        .ok_or_else(|| {
            format!("the harness printed no report.\nstdout:\n{stdout}\nstderr:\n{stderr}")
        })?;
    let report: Value =
        serde_json::from_str(line).map_err(|e| format!("report is not JSON: {e}"))?;
    let cases = report["cases"]
        .as_array()
        .ok_or_else(|| "the report has no cases".to_owned())?;
    Ok(cases
        .iter()
        .map(|c| {
            let id = c["id"].as_str().unwrap_or_default().to_owned();
            let verdict = Verdict {
                rule: c["rule"].as_str().unwrap_or_default().to_owned(),
                verdict: c["verdict"].as_str().unwrap_or_default().to_owned(),
                detail: c["detail"].as_str().unwrap_or_default().to_owned(),
            };
            (id, verdict)
        })
        .collect())
}

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| {
        if matches!(requirement(), Requirement::Skip) {
            return Engine::Skipped;
        }
        let probe = Command::new("python3")
            .arg(harness_script())
            .arg("--probe-engine")
            .output();
        match probe {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                return Engine::Unavailable(
                    String::from_utf8_lossy(&o.stderr)
                        .trim()
                        .trim_start_matches("GANTRY_CONFORMANCE_UNAVAILABLE ")
                        .to_owned(),
                );
            }
            Err(e) => return Engine::Unavailable(format!("python3 could not be started: {e}")),
        }
        match run_harness(&shipped_config(), &[]) {
            Ok(cases) => Engine::Ran(cases),
            Err(why) => Engine::Unavailable(why),
        }
    })
}

/// The heart of the suite: the named hostile artifact was denied by a real engine.
fn denied(case: &str) {
    match engine() {
        Engine::Ran(cases) => {
            let v = cases.get(case).unwrap_or_else(|| {
                panic!(
                    "the conformance run has no case `{case}`; cases.json and this test disagree"
                )
            });
            assert_eq!(
                v.verdict, "blocked",
                "\n\nThe sandbox did NOT enforce `{}`.\nThe hostile artifact `{case}` got through: {}\n",
                v.rule, v.detail
            );
        }
        Engine::Unavailable(why) => match requirement() {
            Requirement::Require => panic!(
                "GANTRY_SANDBOX_ENGINE is set and no engine could be used for `{case}`: {why}"
            ),
            _ => eprintln!(
                "note: `{case}` needs a WebKitGTK view and none is available here ({why})"
            ),
        },
        Engine::Skipped => eprintln!("note: `{case}` skipped, GANTRY_SANDBOX_ENGINE is off"),
    }
}

// ============================================================ one test per rule, in the engine

#[test]
fn engine_an_artifact_cannot_reach_tauris_ipc_internals() {
    denied("tauri_internals");
}

#[test]
fn engine_an_artifact_cannot_reach_the_tauri_global() {
    denied("tauri_global");
}

#[test]
fn engine_an_artifact_cannot_reach_the_ipc_transport() {
    denied("ipc_localhost");
}

#[test]
fn engine_an_artifact_cannot_fetch() {
    denied("fetch");
}

#[test]
fn engine_an_artifact_cannot_use_xhr() {
    denied("xhr");
}

#[test]
fn engine_an_artifact_cannot_open_a_websocket() {
    denied("websocket");
}

#[test]
fn engine_an_artifact_cannot_open_an_event_stream() {
    denied("eventsource");
}

#[test]
fn engine_an_artifact_cannot_send_a_beacon() {
    denied("send_beacon");
}

#[test]
fn engine_an_artifact_cannot_load_a_script_from_the_network() {
    denied("remote_script");
}

#[test]
fn engine_an_artifact_cannot_load_a_stylesheet_from_the_network() {
    denied("remote_stylesheet");
}

#[test]
fn engine_an_artifact_cannot_load_an_image_from_the_network() {
    denied("remote_image");
}

#[test]
fn engine_an_artifact_cannot_load_a_font_from_the_network() {
    denied("remote_font");
}

#[test]
fn engine_an_artifact_cannot_load_media_from_the_network() {
    denied("remote_media");
}

#[test]
fn engine_an_artifact_cannot_open_a_frame_of_its_own() {
    denied("nested_frame");
}

#[test]
fn engine_an_artifact_cannot_embed_an_object() {
    denied("object_embed");
}

#[test]
fn engine_an_artifact_cannot_submit_a_form() {
    denied("form_submit");
}

#[test]
fn engine_an_artifact_cannot_rewrite_its_document_base() {
    denied("base_uri");
}

#[test]
fn engine_an_artifact_cannot_read_the_apps_document() {
    denied("parent_document");
}

#[test]
fn engine_an_artifact_cannot_read_the_top_document() {
    denied("top_document");
}

#[test]
fn engine_an_artifact_cannot_navigate_the_app_window() {
    denied("top_navigation");
}

#[test]
fn engine_an_artifact_cannot_navigate_its_parent() {
    denied("parent_navigation");
}

#[test]
fn engine_an_artifact_cannot_open_a_popup() {
    denied("window_open");
}

#[test]
fn engine_an_artifact_cannot_show_a_modal_dialog() {
    denied("modal_dialog");
}

#[test]
fn engine_an_artifact_cannot_start_a_download() {
    denied("download");
}

#[test]
fn engine_an_artifact_cannot_capture_the_pointer() {
    denied("pointer_lock");
}

#[test]
fn engine_an_artifact_cannot_use_local_storage() {
    denied("local_storage");
}

#[test]
fn engine_an_artifact_cannot_use_session_storage() {
    denied("session_storage");
}

#[test]
fn engine_an_artifact_cannot_use_indexed_db() {
    denied("indexed_db");
}

#[test]
fn engine_an_artifact_cannot_set_a_cookie() {
    denied("cookies");
}

#[test]
fn engine_an_artifact_cannot_use_cache_storage() {
    denied("cache_storage");
}

#[test]
fn engine_an_artifact_cannot_write_the_clipboard() {
    denied("clipboard");
}

#[test]
fn engine_an_artifact_cannot_register_a_service_worker() {
    denied("service_worker");
}

// ------------------------------------------------------------------------ the two that fail

/// 13 §5 says links are intercepted and forwarded to the parent as `open_url`, and that
/// nothing navigates. Interception covers clicks on `<a href>`; a script that assigns
/// `location.href` navigates the artifact's own frame, which needs no sandbox flag and which
/// no directive inside the document refuses — so until 2026-09-16 the request left, taking
/// with it whatever the artifact chose to write into the URL.
///
/// What stops it is a policy on the *other* document: `frame-src` is checked against the one
/// that embeds the frame, whoever started the navigation, and the app document now says
/// `'none'` ([`sandbox::EMBEDDER_CSP`]). The blocked navigation never becomes a request, and
/// `srcdoc` is not a fetch, so the artifact loads and keeps running.
#[test]
fn nothing_navigates_the_artifact_frame_itself() {
    denied("self_navigation");
}

/// And the declaration that does it, held to 13 §5 where the other three already are.
#[test]
fn declares_that_an_artifact_frame_may_go_nowhere() {
    assert_eq!(
        shipped_embedder_csp(),
        sandbox::EMBEDDER_CSP,
        "the app document's own policy has drifted from 13 §5; without `frame-src` an artifact \
         can navigate its frame to any URL it likes, which is egress"
    );
}

/// **Not enforced.** 13 §5, hang risk 1: "Inline scripts in `html` artifacts get the same pass
/// when they parse; scripts that do not parse run unmodified." The React path does run the
/// loop guard — `compile.ts` registers the plugin and every loop body gets the check — but
/// `htmlDocument()` in `features/artifacts/bridge.ts` only prepends the prelude to an `html`
/// artifact's own document. Its `<script>` bodies are never compiled, so no guard is injected.
///
/// This case is run on its own, with `--include-hangs`: on WebKit an artifact's frame shares
/// the web content process with the app document, so a runaway loop in one is the frozen
/// window §5 describes. The harness measures it from outside and the artifact reports having
/// run unguarded for eight seconds.
#[test]
#[ignore = "not enforced: inline scripts in an html artifact are never loop-guarded (13 §5 hang risk 1)"]
fn an_html_artifacts_inline_script_is_loop_guarded() {
    match engine() {
        Engine::Ran(_) => {}
        Engine::Unavailable(why) => {
            eprintln!("note: this case needs a WebKitGTK view and none is available here ({why})");
            return;
        }
        Engine::Skipped => return,
    }
    let cases = run_harness(
        &shipped_config(),
        &["--include-hangs", "--only", "html_loop_guard"],
    )
    .expect("the harness runs");
    let v = cases.get("html_loop_guard").expect("the case ran");
    assert_eq!(
        v.verdict, "blocked",
        "\n\nAn html artifact's inline script ran an unbounded loop: {}\n",
        v.detail
    );
}

// -------------------------------------------------------------------------- testing the test

/// A conformance run that cannot tell a sandbox from no sandbox would pass for ever. This
/// grants the engine the flags §5 withholds, and a policy that allows everything, and requires
/// the cases to notice.
///
/// Two cases are left out on purpose. `window_open` cannot flip, because WebKit blocks a popup
/// with no user activation whatever the sandbox says; `service_worker` cannot, because the
/// harness's parent document is a `file:` URL and registration wants http or https. Both are
/// still worth running in the real configuration — they just prove less than the rest.
#[test]
fn the_conformance_run_notices_a_weakened_sandbox() {
    match engine() {
        Engine::Ran(_) => {}
        Engine::Unavailable(why) => {
            match requirement() {
                Requirement::Require => {
                    panic!("GANTRY_SANDBOX_ENGINE is set and no engine could be used: {why}")
                }
                _ => eprintln!("note: no WebKitGTK view is available here ({why})"),
            }
            return;
        }
        Engine::Skipped => return,
    }
    let weakened = json!({
        "csp": "default-src * 'unsafe-inline' 'unsafe-eval' data: blob:",
        "sandbox_flags": "allow-scripts allow-same-origin allow-popups allow-modals \
                          allow-downloads allow-forms allow-top-navigation allow-pointer-lock",
        "referrer_policy": "no-referrer",
        // The app document's policy is part of what is being weakened: without it a frame
        // navigates itself wherever it likes.
        "parent_csp": "",
        "only": [
            "fetch", "nested_frame", "parent_document", "top_document", "top_navigation",
            "modal_dialog", "download", "local_storage", "session_storage", "indexed_db",
            "cookies", "cache_storage", "self_navigation",
        ],
    });
    let cases = run_harness(&weakened, &[]).expect("the harness runs");
    assert_eq!(cases.len(), 13, "every selected case ran");
    let blind: Vec<&str> = cases
        .iter()
        .filter(|(_, v)| v.verdict == "blocked")
        .map(|(id, _)| id.as_str())
        .collect();
    assert!(
        blind.is_empty(),
        "\n\nThese cases reported the sandbox held with every flag granted and a policy that \
         allows everything, so they cannot detect the rule being weakened: {blind:?}\n"
    );
}

/// Every case in `cases.json` is read by a test in this file, and every case a test names is in
/// `cases.json`. Without this, adding a rule and forgetting its test looks exactly like passing.
#[test]
fn every_case_has_a_test_and_every_test_has_a_case() {
    let spec: Value = serde_json::from_str(&read("artifact-runtime/src/conformance/cases.json"))
        .expect("cases.json is JSON");
    let source = include_str!("artifacts_sandbox.rs");
    let ids: BTreeSet<&str> = spec["cases"]
        .as_array()
        .expect("cases is an array")
        .iter()
        .map(|c| c["id"].as_str().expect("every case has an id"))
        .collect();
    assert!(
        ids.len() >= 30,
        "cases.json is down to {} cases; §5 names more rules than that",
        ids.len()
    );
    let untested: Vec<&&str> = ids
        .iter()
        .filter(|id| !source.contains(&format!("\"{id}\"")))
        .collect();
    assert!(
        untested.is_empty(),
        "these conformance cases are never asserted by a test: {untested:?}"
    );
    // And the other way round: a test that names a case no longer in the file would otherwise
    // only fail when an engine is there to run it.
    let mut named = BTreeSet::new();
    for (at, _) in source.match_indices("denied(\"") {
        let rest = &source[at + "denied(\"".len()..];
        named.insert(&rest[..rest.find('"').expect("a closed string")]);
    }
    let missing: Vec<&&str> = named.iter().filter(|id| !ids.contains(*id)).collect();
    assert!(
        missing.is_empty(),
        "these tests name a case that is no longer in cases.json: {missing:?}"
    );
    // `html_loop_guard` runs on its own, with --include-hangs, so it is the one case no
    // `denied(...)` call names.
    assert_eq!(
        ids.len() - named.len(),
        1,
        "every case but html_loop_guard is asserted by a `denied(...)` call"
    );
}

// ==================================================== the declarations Gantry itself enforces

#[test]
fn declares_one_policy_in_every_place_it_appears() {
    assert_eq!(
        shipped_csp(),
        sandbox::csp(),
        "the policy the app injects into an html artifact has drifted from 13 §5"
    );
    assert_eq!(
        runtime_csp(),
        sandbox::csp(),
        "the runtime document's policy has drifted from the one html artifacts get"
    );
}

#[test]
fn declares_every_directive_section_five_names_and_no_others() {
    let policy = shipped_csp();
    let shipped: Vec<&str> = policy.split("; ").map(str::trim).collect::<Vec<_>>();
    let expected: Vec<String> = sandbox::CSP_DIRECTIVES
        .iter()
        .map(|(name, value)| format!("{name} {value}"))
        .collect();
    for want in &expected {
        assert!(
            shipped.contains(&want.as_str()),
            "the policy no longer says `{want}`; it says {shipped:?}"
        );
    }
    assert_eq!(
        shipped.len(),
        expected.len(),
        "the policy has gained a directive 13 §5 does not name: {shipped:?}"
    );
}

#[test]
fn declares_a_frame_sandboxed_with_allow_scripts_and_nothing_else() {
    assert_eq!(shipped_sandbox_flags(), sandbox::SANDBOX_FLAGS);
    assert!(
        sandbox_host().contains("sandbox={SANDBOX_FLAGS}"),
        "the sandbox host no longer uses the one sandbox attribute"
    );
}

#[test]
fn declares_none_of_the_flags_section_five_withholds() {
    let sources = [
        ("bridge.ts", without_comments(&frontend_bridge())),
        ("SandboxHost.tsx", without_comments(&sandbox_host())),
        (
            "artifact-runtime/index.html",
            without_comments(&runtime_index()),
        ),
    ];
    for (flag, cost) in sandbox::FORBIDDEN_SANDBOX_FLAGS {
        for (name, text) in &sources {
            assert!(
                !text.contains(flag),
                "`{flag}` has appeared in {name}; granting it would open {cost}"
            );
        }
    }
}

#[test]
fn declares_a_srcdoc_frame_that_sends_no_referrer() {
    let host = sandbox_host();
    assert!(
        host.contains("srcDoc={srcdoc}"),
        "the frame is no longer given its document as srcdoc, which is what makes the origin opaque"
    );
    assert_eq!(shipped_referrer_policy(), sandbox::REFERRER_POLICY);
}

#[test]
fn declares_the_policy_and_the_bridge_prelude_on_every_html_artifact() {
    let bridge = frontend_bridge();
    assert!(
        bridge.contains(r#"<meta http-equiv="Content-Security-Policy" content="${CSP}">"#),
        "an html artifact's document no longer opens with the policy"
    );
    // The prelude goes in first, whichever of the three shapes the artifact's document has.
    assert!(bridge.contains("const prelude = pageStyle() + HTML_PRELUDE;"));
    assert!(bridge.contains("content.slice(0, at) + prelude + content.slice(at)"));
    assert!(bridge.contains("<head>${prelude}</head>"));
    // And it is the prelude that carries error capture, link interception and the budget.
    for piece in [
        "window.addEventListener('error'",
        "unhandledrejection",
        "kind: 'open_url'",
        "kind: 'console'",
    ] {
        assert!(
            bridge.contains(piece),
            "the html prelude no longer has `{piece}`"
        );
    }
}

#[test]
fn declares_a_runtime_document_with_nothing_to_load() {
    let index = runtime_index();
    assert!(
        index.contains(r#"<script type="module" src="/src/main.ts">"#),
        "the runtime's one entry point has moved"
    );
    let inline = read("artifact-runtime/scripts/inline.mjs");
    assert!(
        inline.contains(r#"`<script type="module">${safeJs}</script>`"#),
        "the bundle is no longer inlined as a module script; as a classic script it fails to \
         parse in WebKitGTK and the sandbox goes blank (13 §5)"
    );
    assert!(
        inline.contains("an external reference survived"),
        "the inliner no longer warns about a subresource the sandbox could not load"
    );
    // When the runtime has been built here, hold the built document to the same rule.
    let built = desktop().join("artifact-runtime/dist/runtime.html");
    if let Ok(html) = fs::read_to_string(&built) {
        let head = &html[..html.find("<script type=\"module\">").unwrap_or(html.len())];
        let head = head
            .split("<meta")
            .map(|s| s.split_once('>').map_or(s, |(_, rest)| rest))
            .collect::<String>();
        assert!(
            !head.contains(" src=\"") && !head.contains(" href=\""),
            "dist/runtime.html has an external reference the sandbox cannot load"
        );
    }
}

#[test]
fn declares_a_parent_that_checks_the_frame_the_origin_and_the_nonce() {
    let bridge = frontend_bridge();
    for check in [
        "e.source !== frame.contentWindow",
        "e.origin !== 'null'",
        "m.nonce !== nonce",
    ] {
        assert!(
            bridge.contains(check),
            "the parent no longer checks `{check}`, which is one of the three §5 names"
        );
    }
    assert!(
        bridge.contains("crypto.getRandomValues"),
        "the per-mount nonce is no longer unguessable"
    );
    assert!(
        sandbox_host().contains("key={nonce}"),
        "a new document no longer means a new frame, a new nonce and a new handshake"
    );
}

#[test]
fn declares_only_the_messages_section_five_lists() {
    // The union's own object types carry semicolons, so it is read to the blank line after it.
    let union = between(
        &frontend_bridge(),
        "export type SandboxMessage =",
        "",
        "\n\n",
    );
    let named: BTreeSet<String> = union
        .match_indices("kind: '")
        .map(|(at, _)| {
            let rest = &union[at + "kind: '".len()..];
            rest[..rest.find('\'').expect("a closed string")].to_owned()
        })
        .collect();
    let expected: BTreeSet<String> = sandbox::ARTIFACT_TO_PARENT
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(
        named, expected,
        "the artifact-to-parent side of the bridge is no longer the list in 13 §5"
    );

    let runtime = runtime_bridge();
    let inbound = between(&runtime, "export type ParentMessage", "=", ";");
    for kind in sandbox::PARENT_TO_ARTIFACT {
        let interface = format!("kind: '{kind}'");
        assert!(
            runtime.contains(&interface),
            "the parent can no longer send `{kind}`"
        );
    }
    assert_eq!(
        inbound.split('|').count(),
        sandbox::PARENT_TO_ARTIFACT.len(),
        "the parent-to-artifact side of the bridge is no longer the list in 13 §5: {inbound}"
    );
}

#[test]
fn declares_a_console_budget_on_both_sides_of_the_bridge() {
    let runtime = runtime_bridge();
    assert!(
        runtime.contains(&grouped(sandbox::CONSOLE_MAX_BYTES)),
        "the runtime no longer clips a console line at {} bytes",
        sandbox::CONSOLE_MAX_BYTES
    );
    assert!(
        runtime.contains(&format!(
            "consoleBudget = {}",
            sandbox::CONSOLE_MAX_PER_SECOND
        )),
        "the runtime no longer caps console output at {} lines a second",
        sandbox::CONSOLE_MAX_PER_SECOND
    );
    assert!(
        runtime.contains("[console output rate-limited]"),
        "dropping console output is no longer announced"
    );
    let prelude = frontend_bridge();
    assert!(
        prelude.contains(&sandbox::CONSOLE_MAX_BYTES.to_string()),
        "the html prelude no longer clips a console line"
    );
    assert!(
        prelude.contains(&format!("budget = {}", sandbox::CONSOLE_MAX_PER_SECOND)),
        "the html prelude no longer caps console output"
    );
}

#[test]
fn declares_that_only_an_https_url_is_offered_to_the_user() {
    assert!(
        sandbox_host().contains(r"/^https:\/\//i.test(m.url)"),
        "open_url no longer refuses every scheme but {}",
        sandbox::OPEN_URL_SCHEME
    );
    assert!(
        read("frontend/src/lib/clipboard.ts").contains("window.confirm("),
        "a URL from an artifact is no longer confirmed with the user before it is opened"
    );
}

#[test]
fn declares_the_reserved_namespaces_answered_and_unimplemented() {
    let host = sandbox_host();
    for namespace in sandbox::RESERVED_NAMESPACES {
        assert!(
            host.contains(&format!("case '{namespace}':")),
            "the reserved `{namespace}.*` namespace is no longer answered at all"
        );
    }
    assert!(
        host.contains(&format!("error: '{}'", sandbox::RESERVED_ANSWER)),
        "a reserved namespace is answered with something other than `{}`",
        sandbox::RESERVED_ANSWER
    );
}

#[test]
fn declares_a_height_the_parent_clamps() {
    assert!(
        sandbox_host().contains("Math.max(80, Math.min(20_000, Math.round(m.height)))"),
        "an artifact's requested height is no longer clamped by the parent"
    );
}

#[test]
fn declares_a_loop_guard_compiled_into_every_react_artifact() {
    let guard = read("artifact-runtime/src/react/loop-guard.ts");
    assert!(
        guard.contains(&format!(
            "LOOP_BUDGET_MS = {}",
            sandbox::LOOP_BUDGET.as_millis()
        )),
        "the loop budget is no longer {:?}",
        sandbox::LOOP_BUDGET
    );
    assert!(
        guard.contains(sandbox::LOOP_GUARD_MESSAGE),
        "a stopped loop no longer says `{}`, which is what the panel and the model recognise",
        sandbox::LOOP_GUARD_MESSAGE
    );
    for statement in [
        "ForStatement",
        "ForInStatement",
        "ForOfStatement",
        "WhileStatement",
        "DoWhileStatement",
    ] {
        assert!(
            guard.contains(statement),
            "`{statement}` is no longer guarded, so one loop shape can still freeze the window"
        );
    }
    let compile = read("artifact-runtime/src/react/compile.ts");
    assert!(
        compile.contains("Babel.registerPlugin('gantry-loop-guard'")
            && compile.contains("plugins: ['gantry-imports', 'gantry-loop-guard']"),
        "the loop guard is no longer in the compile pipeline"
    );
}

/// 13 §5, hang risk 2: an artifact the user is not looking at must not keep running. The pane
/// renders the active tab's content and nothing else, so React unmounts a hidden artifact's
/// frame the moment it stops being the active tab — sooner than the
/// [`sandbox::HIDDEN_UNMOUNT_AFTER`] the plan reserves for it, and by a mechanism that cannot
/// be forgotten. What this holds is that the pane keeps rendering one tab, not all of them.
#[test]
fn declares_that_only_the_artifact_in_view_is_mounted() {
    let pane = read("frontend/src/components/gantry/pane/RightPane.tsx");
    assert!(
        pane.contains("{active?.content}"),
        "the pane renders more than the active tab, so a hidden artifact now keeps running \
         (13 §5, hang risk 2; the plan allows it {:?} at most)",
        sandbox::HIDDEN_UNMOUNT_AFTER
    );
}

/// **Not enforced.** 13 §5, hang risk 2 opens with "The toolbar's Stop unmounts the iframe".
/// The panel's toolbar has Rendered/Source, the version stepper, Restore, Fix this, Copy and
/// the menu; there is no Stop, so an artifact that spins in the tab the user is looking at
/// cannot be stopped by hand.
///
/// It matters least of the three mitigations and most when the other two have failed: on
/// WebKit the artifact shares the main thread with the app, so by the time a person wants Stop
/// the window is already frozen and the click cannot land. That is an argument for the loop
/// guard covering `html` too (see `an_html_artifacts_inline_script_is_loop_guarded`), not for
/// leaving the escape hatch out.
#[test]
#[ignore = "not built: the artifact toolbar has no Stop (13 §5, hang risk 2)"]
fn the_toolbar_can_stop_a_running_artifact() {
    let panel = read("frontend/src/features/artifacts/ArtifactPanel.tsx");
    assert!(
        panel.contains("aria-label=\"Stop\"") || panel.contains(">Stop<"),
        "the artifact toolbar has no Stop to unmount a running frame"
    );
}
