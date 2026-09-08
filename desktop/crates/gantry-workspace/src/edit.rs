//! The changes a file can be asked to undergo, and the diff every change leaves behind.
//!
//! The shape that works for a model editing code is exact-text replacement with a uniqueness
//! requirement (`docs/connectors/code-editor.md` §1). Whitespace is significant and is never
//! normalised: a model that indents wrongly must be told so, not silently accommodated — but
//! being told so is only useful if the message says what the file actually has, which is what
//! the near-miss search below is for.

use serde::{Deserialize, Serialize};

/// How many characters of a failed needle are quoted back. Enough to recognise, short enough
/// that a refusal never costs more than the edit would have.
const PREVIEW: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Replace {
        old: String,
        new: String,
        count: usize,
    },
    Insert {
        text: String,
        anchor: Anchor,
    },
    Patch {
        patch: String,
    },
    /// Whole-text restore, which is what `undo` and **Revert** replay through.
    Restore {
        text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    After(String),
    /// 1-based; `0` prepends.
    AtLine(usize),
}

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Ambiguous(String),
    #[error("{0}")]
    OutOfRange(String),
    #[error("{0}")]
    Patch(String),
    #[error("the change would leave the file exactly as it is")]
    NoChange,
}

/// What one change did, in the terms the journal and the feed both need.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    /// The hunk as a unified diff, prefixes included.
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diff {
    pub hunks: Vec<Hunk>,
    pub added: usize,
    pub removed: usize,
}

/// Applies a change to text, or explains what the file has instead.
pub fn apply(before: &str, change: &Change) -> Result<String, EditError> {
    let after = match change {
        Change::Replace { old, new, count } => replace(before, old, new, *count)?,
        Change::Insert { text, anchor } => insert(before, text, anchor)?,
        Change::Patch { patch } => patch_text(before, patch)?,
        Change::Restore { text } => text.clone(),
    };
    if after == before {
        return Err(EditError::NoChange);
    }
    Ok(after)
}

fn replace(before: &str, old: &str, new: &str, count: usize) -> Result<String, EditError> {
    if old.is_empty() {
        return Err(EditError::NotFound(
            "`old` is empty; use insert to add text that replaces nothing".to_owned(),
        ));
    }
    let hits = occurrences(before, old);
    if hits.is_empty() {
        return Err(EditError::NotFound(near_miss(before, old)));
    }
    if hits.len() != count {
        let lines: Vec<String> = hits
            .iter()
            .map(|o| line_of(before, *o).to_string())
            .collect();
        return Err(EditError::Ambiguous(format!(
            "found {} occurrences of that text (lines {}), not {count}. Widen `old` with \
             surrounding lines so it names exactly the one you mean, or set `count` to {}.",
            hits.len(),
            lines.join(", "),
            hits.len()
        )));
    }
    Ok(before.replace(old, new))
}

fn insert(before: &str, text: &str, anchor: &Anchor) -> Result<String, EditError> {
    match anchor {
        Anchor::After(needle) => {
            if needle.is_empty() {
                return Err(EditError::NotFound(
                    "`after` is empty; give the text to insert after, or use `at_line`".to_owned(),
                ));
            }
            let hits = occurrences(before, needle);
            match hits.len() {
                0 => Err(EditError::NotFound(near_miss(before, needle))),
                1 => {
                    let at = hits[0] + needle.len();
                    let mut out = String::with_capacity(before.len() + text.len() + 1);
                    out.push_str(&before[..at]);
                    if !before[..at].ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str(text);
                    if !text.ends_with('\n') && !before[at..].is_empty() {
                        out.push('\n');
                    }
                    out.push_str(before[at..].trim_start_matches('\n'));
                    Ok(out)
                }
                n => {
                    let lines: Vec<String> = hits
                        .iter()
                        .map(|o| line_of(before, *o).to_string())
                        .collect();
                    Err(EditError::Ambiguous(format!(
                        "`after` matches {n} places (lines {}); widen it so it names one.",
                        lines.join(", ")
                    )))
                }
            }
        }
        Anchor::AtLine(at) => {
            let lines: Vec<&str> = before.split_inclusive('\n').collect();
            if *at > lines.len() {
                return Err(EditError::OutOfRange(format!(
                    "the file has {} lines; `at_line` {at} is past the end. Insert after the \
                     last line's text instead.",
                    lines.len()
                )));
            }
            let mut out: String = lines[..*at].concat();
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(text);
            if !text.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&lines[*at..].concat());
            Ok(out)
        }
    }
}

/// A unified diff applied with fuzz zero: context must match. A rejected hunk fails the whole
/// call, because a half-applied patch is worse than none.
fn patch_text(before: &str, patch: &str) -> Result<String, EditError> {
    let parsed = diffy::Patch::from_str(patch)
        .map_err(|err| EditError::Patch(format!("that is not a unified diff: {err}")))?;
    diffy::apply(before, &parsed).map_err(|_| EditError::Patch(rejected_hunk(before, &parsed)))
}

/// Which hunk did not apply, and what the file has where its context was expected.
fn rejected_hunk(before: &str, patch: &diffy::Patch<'_, str>) -> String {
    let lines: Vec<&str> = before.lines().collect();
    for (n, hunk) in patch.hunks().iter().enumerate() {
        let expected: Vec<&str> = hunk
            .lines()
            .iter()
            .filter_map(|line| match line {
                diffy::Line::Context(text) | diffy::Line::Delete(text) => Some(text.trim_end()),
                diffy::Line::Insert(_) => None,
            })
            .collect();
        let start = hunk.old_range().start().saturating_sub(1);
        let found: Vec<&str> = lines
            .iter()
            .skip(start)
            .take(expected.len())
            .map(|l| l.trim_end())
            .collect();
        if found == expected {
            continue;
        }
        return format!(
            "hunk {} did not apply. It expects, from line {}:\n{}\nThe file has:\n{}\nRead the \
             file again and patch what is there.",
            n + 1,
            start + 1,
            quote(&expected.join("\n")),
            quote(&found.join("\n"))
        );
    }
    "the patch did not apply to this file; read it again and patch what is there.".to_owned()
}

/// The lines that differ, for the journal, the feed and the diff drawer.
#[must_use]
pub fn diff(before: &str, after: &str) -> Diff {
    let patch = diffy::create_patch(before, after);
    let mut out = Diff::default();
    for hunk in patch.hunks() {
        let mut text = String::new();
        for line in hunk.lines() {
            match line {
                diffy::Line::Context(l) => {
                    text.push(' ');
                    text.push_str(l);
                }
                diffy::Line::Delete(l) => {
                    out.removed += 1;
                    text.push('-');
                    text.push_str(l);
                }
                diffy::Line::Insert(l) => {
                    out.added += 1;
                    text.push('+');
                    text.push_str(l);
                }
            }
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
        out.hunks.push(Hunk {
            old_start: hunk.old_range().start(),
            old_lines: hunk.old_range().len(),
            new_start: hunk.new_range().start(),
            new_lines: hunk.new_range().len(),
            text,
        });
    }
    out
}

/// Byte offsets of every non-overlapping occurrence.
fn occurrences(haystack: &str, needle: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = haystack[from..].find(needle) {
        let at = from + at;
        found.push(at);
        from = at + needle.len();
    }
    found
}

fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count() + 1
}

/// What to say when the text is not there. A bare "not found" teaches a model nothing; the
/// overwhelmingly common cause is indentation, so that case is named explicitly.
fn near_miss(before: &str, needle: &str) -> String {
    let quoted = quote(needle);
    if let Some(line) = indentation_match(before, needle) {
        return format!(
            "that text is not in the file, but the same text with different indentation is at \
             line {line}. Whitespace is significant: copy the line exactly as the file has it.\n\
             You asked for:\n{quoted}"
        );
    }
    if let Some(first) = needle.lines().find(|l| !l.trim().is_empty())
        && let Some(at) = before.find(first.trim())
    {
        let line = line_of(before, at);
        let actual = before.lines().nth(line - 1).unwrap_or_default();
        return format!(
            "that text is not in the file. Its first line appears at line {line}, where the \
             file has:\n{}\nRead the file again and copy the passage exactly.",
            quote(actual)
        );
    }
    format!("that text is not in the file:\n{quoted}\nRead the file again before editing it.")
}

/// The needle's lines, ignoring leading whitespace, found as a run in the file.
fn indentation_match(before: &str, needle: &str) -> Option<usize> {
    let needle: Vec<&str> = needle.lines().map(str::trim_start).collect();
    if needle.is_empty() {
        return None;
    }
    let lines: Vec<&str> = before.lines().map(str::trim_start).collect();
    lines
        .windows(needle.len())
        .position(|window| window == needle.as_slice())
        .map(|at| at + 1)
}

fn quote(text: &str) -> String {
    let mut preview: String = text.chars().take(PREVIEW).collect();
    if text.chars().count() > PREVIEW {
        preview.push('…');
    }
    preview
        .lines()
        .map(|l| format!("    {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n";

    #[test]
    fn a_unique_passage_is_replaced() {
        let out = apply(
            FILE,
            &Change::Replace {
                old: "let x = 1;".into(),
                new: "let x = 2;".into(),
                count: 1,
            },
        )
        .unwrap();
        assert!(out.contains("let x = 2;"));
    }

    #[test]
    fn wrong_indentation_is_named_as_such() {
        let err = apply(
            FILE,
            &Change::Replace {
                old: "let x = 1;\nprintln!(\"{x}\");".into(),
                new: "".into(),
                count: 1,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("different indentation"), "{err}");
        assert!(err.to_string().contains("line 2"), "{err}");
    }

    #[test]
    fn several_occurrences_report_their_lines_instead_of_guessing() {
        let text = "a\nx\nb\nx\n";
        let err = apply(
            text,
            &Change::Replace {
                old: "x\n".into(),
                new: "y\n".into(),
                count: 1,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("lines 2, 4"), "{err}");
    }

    #[test]
    fn insert_lands_after_its_anchor_and_before_a_line() {
        let after = apply(
            FILE,
            &Change::Insert {
                text: "    let y = 2;".into(),
                anchor: Anchor::After("let x = 1;".into()),
            },
        )
        .unwrap();
        assert_eq!(
            after,
            "fn main() {\n    let x = 1;\n    let y = 2;\n    println!(\"{x}\");\n}\n"
        );
        let top = apply(
            FILE,
            &Change::Insert {
                text: "// header".into(),
                anchor: Anchor::AtLine(0),
            },
        )
        .unwrap();
        assert!(top.starts_with("// header\nfn main()"));
    }

    #[test]
    fn a_patch_applies_or_says_which_hunk_did_not() {
        let patch = diffy::create_patch(
            FILE,
            "fn main() {\n    let x = 9;\n    println!(\"{x}\");\n}\n",
        )
        .to_string();
        let out = apply(
            FILE,
            &Change::Patch {
                patch: patch.clone(),
            },
        )
        .unwrap();
        assert!(out.contains("let x = 9;"));

        let other = "fn main() {\n    let z = 0;\n    println!(\"{z}\");\n}\n";
        let err = apply(other, &Change::Patch { patch }).unwrap_err();
        assert!(err.to_string().contains("hunk 1 did not apply"), "{err}");
        assert!(err.to_string().contains("The file has"), "{err}");
    }

    #[test]
    fn the_diff_counts_what_changed() {
        let d = diff("a\nb\nc\n", "a\nB\nc\nd\n");
        assert_eq!((d.added, d.removed), (2, 1));
        assert_eq!(d.hunks.len(), 1);
        assert!(d.hunks[0].text.contains("-b"));
    }
}
