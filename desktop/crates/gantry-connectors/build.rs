//! Embeds every manifest under `desktop/connectors/` into the binary (docs/plan/03 §11: the
//! curated catalog ships inside each release; there is no endpoint to fetch it from).
//!
//! Generates one `&[(&str, &str)]` of (id, manifest JSON). A malformed manifest fails the
//! build rather than disappearing quietly from the catalog.

use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../connectors")
        .canonicalize()
        .expect("desktop/connectors exists");
    println!("cargo:rerun-if-changed={}", root.display());

    let mut entries: Vec<(String, String)> = Vec::new();
    let mut dirs: Vec<_> = fs::read_dir(&root)
        .expect("read desktop/connectors")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    for dir in dirs {
        let manifest = dir.join("manifest.json");
        if !manifest.exists() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", manifest.display());
        let json = fs::read_to_string(&manifest)
            .unwrap_or_else(|e| panic!("reading {}: {e}", manifest.display()));
        let id = dir
            .file_name()
            .and_then(|n| n.to_str())
            .expect("a connector folder name")
            .to_owned();
        // A minimal parse here catches a broken manifest at build time, where it is cheap.
        let value: serde_json::Value = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("{}/manifest.json is not JSON: {e}", id));
        let declared = value.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        assert_eq!(
            declared, id,
            "{id}/manifest.json declares the id {declared}; the folder name is the id"
        );
        entries.push((id, json));
    }

    let mut out = String::from("/// The catalog, embedded at build time (03 §11).\n");
    out.push_str("pub static EMBEDDED: &[(&str, &str)] = &[\n");
    for (id, json) in &entries {
        out.push_str(&format!("    ({id:?}, r#\"{json}\"#),\n"));
    }
    out.push_str("];\n");

    let dest = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("catalog.rs");
    fs::write(&dest, out).expect("writing the catalog");
}
