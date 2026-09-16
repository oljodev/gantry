//! The sandbox policy (docs/plan/13 §5), stated once in Rust.
//!
//! The sandbox itself is three declarations in the frontend and the runtime document: the
//! iframe's attributes (`desktop/frontend/src/features/artifacts/renderers/SandboxHost.tsx`),
//! the Content-Security-Policy (`desktop/frontend/src/features/artifacts/bridge.ts` for `html`
//! artifacts, `desktop/artifact-runtime/index.html` for the runtime document), and the bridge
//! protocol both sides implement. A browser enforces the first two; nothing enforces that they
//! keep saying what §5 says they say.
//!
//! This module is that statement, and `tests/artifacts_sandbox.rs` holds the shipped
//! declarations to it — and then runs one hostile artifact per rule in a real engine to see
//! that the rule is enforced and not merely written down.

use std::time::Duration;

/// The policy every sandboxed artifact document carries, directive by directive (13 §5).
///
/// `'unsafe-eval'` is in `script-src` because compiled artifact code needs it; it is confined
/// to a document with an opaque origin, no network and no storage. `connect-src 'none'` closes
/// fetch, XHR, WebSocket and EventSource, and with them the `http://ipc.localhost` transport,
/// which is the second lock on Tauri's IPC.
pub const CSP_DIRECTIVES: &[(&str, &str)] = &[
    ("default-src", "'none'"),
    ("script-src", "'unsafe-inline' 'unsafe-eval' blob:"),
    ("style-src", "'unsafe-inline'"),
    ("img-src", "data: blob:"),
    ("font-src", "data:"),
    ("media-src", "data: blob:"),
    ("connect-src", "'none'"),
    ("frame-src", "'none'"),
    ("object-src", "'none'"),
    ("form-action", "'none'"),
    ("base-uri", "'none'"),
];

/// The directives above as the one string the `<meta>` element carries.
#[must_use]
pub fn csp() -> String {
    CSP_DIRECTIVES
        .iter()
        .map(|(name, value)| format!("{name} {value}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The iframe's `sandbox` attribute: scripts, and nothing else.
pub const SANDBOX_FLAGS: &str = "allow-scripts";

/// The flags deliberately absent (13 §5), each with what granting it would open.
pub const FORBIDDEN_SANDBOX_FLAGS: &[(&str, &str)] = &[
    (
        "allow-same-origin",
        "the document would be the app's origin: its DOM, its storage, its cookies, its IPC",
    ),
    (
        "allow-top-navigation",
        "an artifact could send the app window somewhere else",
    ),
    (
        "allow-top-navigation-by-user-activation",
        "the same, behind a click the artifact draws itself",
    ),
    (
        "allow-popups",
        "a new browsing context outside the panel and its policy",
    ),
    (
        "allow-forms",
        "form posts, which are egress and a navigation at once",
    ),
    (
        "allow-modals",
        "alert/confirm/print, which freeze the window they are in",
    ),
    (
        "allow-downloads",
        "writes to the disk; Download is parent-side (13 §4)",
    ),
    ("allow-pointer-lock", "capture of the pointer"),
    ("allow-presentation", "a second display"),
    ("allow-orientation-lock", "control of the screen"),
];

/// The iframe's `referrerpolicy`.
pub const REFERRER_POLICY: &str = "no-referrer";

/// Messages the parent sends into an artifact (13 §5).
pub const PARENT_TO_ARTIFACT: &[&str] = &["mount", "update", "theme"];

/// Messages an artifact sends out, plus the `loaded` handshake that precedes the nonce.
pub const ARTIFACT_TO_PARENT: &[&str] = &[
    "loaded", "ready", "error", "console", "resize", "open_url", "storage", "tools",
];

/// Namespaces reserved for §8 and for a future MCP route, answered and never implemented.
pub const RESERVED_NAMESPACES: &[&str] = &["storage", "tools"];

/// What a reserved namespace is answered with in v1.
pub const RESERVED_ANSWER: &str = "unsupported";

/// A console line is clipped here (13 §5).
pub const CONSOLE_MAX_BYTES: usize = 16_384;

/// And this many lines per second get through.
pub const CONSOLE_MAX_PER_SECOND: u32 = 50;

/// The only scheme the parent will open from an `open_url` message.
pub const OPEN_URL_SCHEME: &str = "https:";

/// A loop running longer than this throws `Artifact loop guard` (13 §5, hang risk 1).
pub const LOOP_BUDGET: Duration = Duration::from_secs(3);

/// What the guard's message says, so the panel and the model can recognise it.
pub const LOOP_GUARD_MESSAGE: &str = "Artifact loop guard";

/// A hidden artifact is unmounted after this long and remounted on demand (hang risk 2).
pub const HIDDEN_UNMOUNT_AFTER: Duration = Duration::from_secs(60);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_policy_reads_as_one_string() {
        let policy = csp();
        assert!(policy.starts_with("default-src 'none'; script-src "));
        assert!(policy.ends_with("base-uri 'none'"));
        assert!(policy.contains("connect-src 'none'"));
        // Every directive is named once and only once.
        for (name, _) in CSP_DIRECTIVES {
            assert_eq!(
                policy.matches(&format!("{name} ")).count(),
                1,
                "{name} appears more than once"
            );
        }
    }
}
