//! `cargo xtask validate-skills` (docs/plan/12 §A2): the rules of a bundled skill, all of them,
//! reported together.
//!
//! `gantry-agent`'s `build.rs` already refuses a folder with no `SKILL.md` or a name that
//! disagrees with its folder, because a build cannot embed what it cannot read. This is the
//! rest — the limits, the reference files, the shape of a description — and it reports every
//! problem at once instead of stopping at the first, which is what makes it worth running.

use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, bail};
use gantry_agent::skills::format;
use gantry_core::skill;

pub fn validate(root: &Path) -> anyhow::Result<()> {
    let dir = root.join("desktop/skills");
    let mut problems: Vec<String> = Vec::new();
    let mut names: BTreeSet<String> = BTreeSet::new();

    let mut folders: Vec<_> = fs::read_dir(&dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();

    for folder in &folders {
        let name = folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        let file = folder.join("SKILL.md");
        if !file.exists() {
            problems.push(format!("{name}: no SKILL.md"));
            continue;
        }
        if let Some(problem) = skill::name_problem(&name) {
            problems.push(format!("{name}: {problem}"));
        }
        let text = fs::read_to_string(&file).with_context(|| format!("reading {name}/SKILL.md"))?;
        let parsed = match format::parse(&text) {
            Ok(p) => p,
            Err(err) => {
                problems.push(format!("{name}: {err}"));
                continue;
            }
        };
        if parsed.front.name != name {
            problems.push(format!(
                "{name}: the frontmatter says `{}`; the folder name is the name",
                parsed.front.name
            ));
        }
        for line in &parsed.unread {
            problems.push(format!(
                "{name}: `{line}` is not a field Gantry reads. A bundled skill should use only \
                 the documented fields."
            ));
        }
        names.insert(name.clone());

        let mut references = Vec::new();
        let ref_dir = folder.join("references");
        if ref_dir.is_dir() {
            let mut files: Vec<_> = fs::read_dir(&ref_dir)
                .with_context(|| format!("reading {name}/references"))?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .collect();
            files.sort();
            for path in files {
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_owned();
                if !path.is_file() || !path.extension().is_some_and(|e| e == "md" || e == "txt") {
                    problems.push(format!(
                        "{name}: `references/{file_name}` is not text. A skill is text and \
                         nothing else (12 §A2)."
                    ));
                    continue;
                }
                let text = fs::read_to_string(&path)?;
                references.push(gantry_core::SkillReference {
                    file: file_name,
                    text,
                });
            }
        }
        // Anything beside SKILL.md and references/ has no way of being used and no way of being
        // installed, so it is a mistake rather than an extra.
        for entry in fs::read_dir(folder)?.filter_map(Result::ok) {
            let entry_name = entry.file_name().to_string_lossy().into_owned();
            if entry_name != "SKILL.md" && entry_name != "references" {
                problems.push(format!(
                    "{name}: `{entry_name}` is neither SKILL.md nor references/; nothing would \
                     install it."
                ));
            }
        }

        let input = format::to_input(&parsed, references);
        for problem in skill::validate(&input) {
            problems.push(format!("{name}: {problem}"));
        }
        // A description that does not say *when* to use the skill is a description the matcher
        // cannot work with (12 §A4). A weak heuristic, but it catches the common omission.
        if !input.description.to_lowercase().contains("use when")
            && !input.description.to_lowercase().contains("use it when")
        {
            problems.push(format!(
                "{name}: the description never says when to use the skill, which is the matcher's \
                 main signal. Write \"… Use when …\"."
            ));
        }
        if input.triggers.is_empty() {
            problems.push(format!(
                "{name}: no `metadata.gantry-triggers`. A bundled skill should name the words \
                 that mean it."
            ));
        }
    }

    if folders.is_empty() {
        problems.push("desktop/skills holds no skills at all".to_owned());
    }

    if problems.is_empty() {
        println!("validate-skills: {} skills, all good", names.len());
        return Ok(());
    }
    for p in &problems {
        eprintln!("  {p}");
    }
    bail!("{} problem(s) in desktop/skills", problems.len())
}
