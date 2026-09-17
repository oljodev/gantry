//! The one thing Linux needs said before the webview exists.
//!
//! WebKitGTK renders through DMA-BUF, and on the NVIDIA proprietary driver that path has never
//! worked reliably: the window comes up empty, or black, or shows one frame and then stops.
//! Every GTK/WebKit application on that driver hits it, and the answer everybody reaches for is
//! `WEBKIT_DISABLE_DMABUF_RENDERER=1`, which falls back to a shared-memory path.
//!
//! That fallback is **slower** where DMA-BUF works, which is why this is not set unconditionally
//! (docs/dev/performance.md): a machine with working acceleration keeps it. The variable is set
//! only where the proprietary NVIDIA driver is loaded and the user has not already answered the
//! question themselves — their value always wins, in either direction.
//!
//! The decision is a plain function, tested on every platform. A rule that exists only inside a
//! `cfg(target_os = "linux")` block on a machine that is not Linux is a comment.

/// The name of the variable WebKitGTK reads.
pub const DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";

/// What to set `WEBKIT_DISABLE_DMABUF_RENDERER` to, or `None` to leave it alone.
///
/// `set` is the value already in the environment; `nvidia` is whether the proprietary driver is
/// loaded.
#[must_use]
pub fn dmabuf_value(set: Option<&str>, nvidia: bool) -> Option<&'static str> {
    match set {
        // Answered already, by the user or by their desktop. Either answer is theirs.
        Some(_) => None,
        None if nvidia => Some("1"),
        None => None,
    }
}

/// Whether the NVIDIA proprietary driver is loaded. The open kernel module registers the same
/// two paths, and has the same DMA-BUF problem, so both are covered.
#[cfg(target_os = "linux")]
#[must_use]
pub fn nvidia_driver() -> bool {
    std::path::Path::new("/proc/driver/nvidia/version").exists()
        || std::path::Path::new("/sys/module/nvidia").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_users_own_answer_wins_in_either_direction() {
        assert_eq!(dmabuf_value(Some("0"), true), None);
        assert_eq!(dmabuf_value(Some("1"), false), None);
        assert_eq!(dmabuf_value(Some(""), true), None);
    }

    #[test]
    fn only_the_driver_that_needs_it_gets_it() {
        assert_eq!(dmabuf_value(None, true), Some("1"));
        // A machine whose acceleration works keeps the faster path.
        assert_eq!(dmabuf_value(None, false), None);
    }
}
