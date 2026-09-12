//! Bringing a skill in from somewhere else (docs/plan/12 §A5 flow 3).
//!
//! Every path — a `.md` file, a folder, a pasted URL — ends at the same review screen, and
//! **nothing is written before the user presses Install**. That is the whole safety model for
//! imported text: a skill cannot execute anything, so the risk is prose that steers the model
//! badly, and the answer to that risk is that a person read it first.

use std::path::Path;

use gantry_core::{SkillInput, SkillReference, skill};

use crate::skills::format;

/// What the review screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub input: SkillInput,
    /// The file as it arrived, installed byte for byte when the user says yes.
    pub text: String,
    /// Things the user should know before installing: what was dropped, what was repaired,
    /// what does not fit. Never silent.
    pub warnings: Vec<String>,
    /// What `skill::validate` refuses. A review with problems cannot be installed.
    pub problems: Vec<String>,
    /// The name this would take, after repair and after avoiding a collision.
    pub name: String,
}

/// The cap on a skill fetched from a URL (12 §A5).
pub const URL_MAX_BYTES: usize = 256 * 1024;

/// Reviews a single `SKILL.md`. `filename` is used only to suggest a name when the frontmatter
/// has none worth keeping.
#[must_use]
pub fn review_text(text: &str, filename: Option<&str>) -> Review {
    let mut warnings = Vec::new();
    let parsed = match format::parse(text) {
        Ok(p) => p,
        Err(err) => {
            return Review {
                input: SkillInput::default(),
                text: text.to_owned(),
                warnings,
                problems: vec![err],
                name: String::new(),
            };
        }
    };
    for line in &parsed.unread {
        warnings.push(format!(
            "`{line}` is not a field Gantry reads. It stays in the file and travels with it, but \
             nothing here acts on it."
        ));
    }
    let mut input = format::to_input(&parsed, Vec::new());

    // A name that breaks the rule is repaired rather than refused: the file is somebody else's,
    // and `Rust Idioms` meaning `rust-idioms` is not ambiguous.
    if skill::name_problem(&input.name).is_some() {
        let from_file = filename
            .and_then(|f| f.split('/').next_back())
            .map(|f| f.trim_end_matches(".skill.md").trim_end_matches(".md"))
            .unwrap_or_default();
        let repaired = skill::slugify(if input.name.trim().is_empty() {
            from_file
        } else {
            &input.name
        });
        if !repaired.is_empty() {
            warnings.push(format!(
                "The name `{}` is not a folder name; it will be installed as `{repaired}`.",
                input.name
            ));
            input.name = repaired;
        }
    }
    if input.description.chars().count() > skill::DESCRIPTION_MAX {
        warnings.push(format!(
            "The description is {} characters and is cut to {}.",
            input.description.chars().count(),
            skill::DESCRIPTION_MAX
        ));
        input.description = input
            .description
            .chars()
            .take(skill::DESCRIPTION_MAX)
            .collect();
    }

    let problems = skill::validate(&input);
    let name = input.name.clone();
    Review {
        input,
        text: text.to_owned(),
        warnings,
        problems,
        name,
    }
}

/// Reviews a skill folder: its `SKILL.md`, its `references/*.md`, and everything in it that a
/// text-only skill may not carry.
pub fn review_folder(dir: &Path) -> Result<Review, String> {
    let file = dir.join("SKILL.md");
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("{} has no readable SKILL.md: {e}", dir.display()))?;
    let folder_name = dir.file_name().and_then(|n| n.to_str());
    let mut review = review_text(&text, folder_name);

    let mut references = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name == "SKILL.md" {
                continue;
            }
            if path.is_dir() && name == "references" {
                let (found, warn) = read_references(&path);
                references = found;
                review.warnings.extend(warn);
            } else {
                // `scripts/`, `assets/`, a binary, a notebook: a skill is text (12 §A2).
                dropped.push(name.to_owned());
            }
        }
    }
    if !dropped.is_empty() {
        review.warnings.push(format!(
            "Not installed, because a skill is text and nothing else: {}.",
            dropped.join(", ")
        ));
    }
    review.input.references = references;
    review.problems = skill::validate(&review.input);
    Ok(review)
}

fn read_references(dir: &Path) -> (Vec<SkillReference>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (out, warnings);
    };
    let mut paths: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        if !path.is_file() || !path.extension().is_some_and(|e| e == "md" || e == "txt") {
            warnings.push(format!(
                "`references/{name}` is not text and is left behind."
            ));
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) if text.len() > skill::REFERENCE_MAX_BYTES => warnings.push(format!(
                "`references/{name}` is {} KB, over the {} KB a reference may be, and is left \
                 behind.",
                text.len() / 1024,
                skill::REFERENCE_MAX_BYTES / 1024
            )),
            Ok(text) => {
                if out.len() < skill::REFERENCES_MAX {
                    out.push(SkillReference { file: name, text });
                } else {
                    warnings.push(format!(
                        "`references/{name}` is past the {} files a skill may carry and is left \
                         behind.",
                        skill::REFERENCES_MAX
                    ));
                }
            }
            Err(err) => warnings.push(format!("`references/{name}` could not be read: {err}")),
        }
    }
    (out, warnings)
}

/// What a skill is exported as (12 §A5 flow 2): the exact `SKILL.md`, under a name that is
/// recognizable in a downloads folder.
#[must_use]
pub fn export_filename(name: &str) -> String {
    format!("{name}.skill.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_is_not_a_folder_name_is_repaired_and_the_user_is_told() {
        let review = review_text(
            "---\nname: Rust Idioms\ndescription: About Rust.\n---\nBody.\n",
            None,
        );
        assert_eq!(review.name, "rust-idioms");
        assert!(review.problems.is_empty(), "{:?}", review.problems);
        assert!(
            review.warnings.iter().any(|w| w.contains("rust-idioms")),
            "{:?}",
            review.warnings
        );
    }

    #[test]
    fn a_file_that_is_not_a_skill_is_a_problem_not_a_guess() {
        let review = review_text("# Just some notes\n", Some("notes.md"));
        assert_eq!(review.problems.len(), 1);
        assert!(review.problems[0].contains("frontmatter"));
    }

    #[test]
    fn a_folder_keeps_its_references_and_leaves_its_scripts_behind() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("helper");
        std::fs::create_dir_all(skill.join("references")).unwrap();
        std::fs::create_dir_all(skill.join("scripts")).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            "---\nname: helper\ndescription: Helps.\n---\nBody.\n",
        )
        .unwrap();
        std::fs::write(skill.join("references/one.md"), "Reference one.").unwrap();
        std::fs::write(skill.join("scripts/run.sh"), "rm -rf /").unwrap();

        let review = review_folder(&skill).unwrap();
        assert!(review.problems.is_empty(), "{:?}", review.problems);
        assert_eq!(review.input.references.len(), 1);
        assert_eq!(review.input.references[0].file, "one.md");
        assert!(
            review.warnings.iter().any(|w| w.contains("scripts")),
            "the dropped folder is named, not silently skipped: {:?}",
            review.warnings
        );
    }
}
