//! Embeds every skill under `desktop/skills/` into the binary (docs/plan/12 §A3: a bundled
//! skill is not a file on the user's disk, it ships inside the release).
//!
//! Generates one table of `(name, SKILL.md, &[(reference file, text)])`. The text arrives
//! through `include_str!` rather than a raw string literal, because a Markdown body may hold
//! any sequence of characters and a literal would have to guess how many hashes to use.
//!
//! What is checked here is only what makes the table wrong: a folder with no `SKILL.md`, a
//! missing frontmatter block, and a `name:` that disagrees with the folder. The full rules of
//! 12 §A2 are `cargo xtask validate-skills`, which can report all of them at once instead of
//! stopping the build at the first.

use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills")
        .canonicalize()
        .expect("desktop/skills exists");
    println!("cargo:rerun-if-changed={}", root.display());

    let mut dirs: Vec<_> = fs::read_dir(&root)
        .expect("read desktop/skills")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();

    let mut out = String::from(
        "/// One bundled skill: its name, its `SKILL.md`, and its reference files.\n\
         pub type BundledSkill = (\n\
         \x20   &'static str,\n\
         \x20   &'static str,\n\
         \x20   &'static [(&'static str, &'static str)],\n\
         );\n\n\
         /// The bundled skills, embedded at build time (12 §A3).\n\
         pub static BUNDLED: &[BundledSkill] = &[\n",
    );

    for dir in dirs {
        let file = dir.join("SKILL.md");
        if !file.exists() {
            continue;
        }
        println!("cargo:rerun-if-changed={}", file.display());
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .expect("a skill folder name")
            .to_owned();
        let text =
            fs::read_to_string(&file).unwrap_or_else(|e| panic!("reading {}: {e}", file.display()));
        assert!(
            text.starts_with("---\n") || text.starts_with("---\r\n"),
            "{name}/SKILL.md does not start with a `---` frontmatter block"
        );
        let declared = text
            .lines()
            .skip(1)
            .take_while(|l| l.trim_end() != "---")
            .find_map(|l| l.strip_prefix("name:"))
            .map(|v| v.trim().trim_matches(['"', '\'']).to_owned())
            .unwrap_or_default();
        assert_eq!(
            declared, name,
            "{name}/SKILL.md declares the name {declared:?}; the folder name is the name"
        );

        let mut refs: Vec<(String, PathBuf)> = Vec::new();
        let ref_dir = dir.join("references");
        if ref_dir.is_dir() {
            println!("cargo:rerun-if-changed={}", ref_dir.display());
            let mut entries: Vec<_> = fs::read_dir(&ref_dir)
                .expect("read a references folder")
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "md" || e == "txt"))
                .collect();
            entries.sort();
            for path in entries {
                println!("cargo:rerun-if-changed={}", path.display());
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .expect("a reference file name")
                    .to_owned();
                refs.push((file_name, path));
            }
        }

        out.push_str(&format!("    ({name:?}, include_str!({:?}), &[\n", file));
        for (file_name, path) in &refs {
            out.push_str(&format!(
                "        ({file_name:?}, include_str!({:?})),\n",
                path
            ));
        }
        out.push_str("    ]),\n");
    }
    out.push_str("];\n");

    let dest = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("bundled_skills.rs");
    fs::write(&dest, out).expect("writing the bundled skills");
}
