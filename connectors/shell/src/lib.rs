//! First-party connector: Shell. See `docs/plan/03-connector-system.md` §5.
//!
//! Stub: the manifest is embedded and validated; tools arrive with the milestone that owns
//! this connector (`docs/plan/09-roadmap.md`).

#![forbid(unsafe_code)]

/// The connector manifest, embedded at build time (`docs/plan/03-connector-system.md` §3).
pub const MANIFEST: &str = include_str!("../manifest.json");

/// The connector id; equals the folder name and the tool namespace prefix.
pub const ID: &str = "shell";

#[cfg(test)]
mod tests {
    #[test]
    fn manifest_is_valid_json_with_the_right_id() {
        let manifest: serde_json::Value = serde_json::from_str(super::MANIFEST).unwrap();
        assert_eq!(manifest["manifest_version"], "1");
        assert_eq!(manifest["id"], super::ID);
        assert_eq!(manifest["runtime"]["kind"], "native");
        assert_eq!(manifest["runtime"]["crate"], env!("CARGO_PKG_NAME"));
    }
}
