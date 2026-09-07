//! Exact-match text replacement (docs/plan/03 §5, 13 §2): `old_string` must occur exactly
//! once unless `replace_all` is set. The code editor connector reuses this in M6.

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Edit {
    pub old_string: String,
    pub new_string: String,
    #[serde(default)]
    pub replace_all: bool,
}

/// Applies the edits in order and returns the new text, or a message the model can act on.
pub fn apply_edits(content: &str, edits: &[Edit]) -> Result<String, String> {
    if edits.is_empty() {
        return Err("edits is empty; give at least one { old_string, new_string }".into());
    }
    let mut text = content.to_owned();
    for (i, e) in edits.iter().enumerate() {
        if e.old_string.is_empty() {
            return Err(format!("edit {}: old_string is empty", i + 1));
        }
        let count = text.matches(&e.old_string).count();
        match count {
            0 => {
                return Err(format!(
                    "edit {}: old_string was not found. Read the artifact first and copy the \
                     text exactly, including whitespace.",
                    i + 1
                ));
            }
            1 => text = text.replacen(&e.old_string, &e.new_string, 1),
            n if e.replace_all => {
                let _ = n;
                text = text.replace(&e.old_string, &e.new_string);
            }
            n => {
                return Err(format!(
                    "edit {}: old_string occurs {n} times; include more surrounding text so it \
                     matches once, or set replace_all",
                    i + 1
                ));
            }
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(old: &str, new: &str, all: bool) -> Edit {
        Edit {
            old_string: old.into(),
            new_string: new.into(),
            replace_all: all,
        }
    }

    #[test]
    fn unique_matches_replace_and_ambiguous_ones_refuse() {
        assert_eq!(
            apply_edits("a b a", &[edit("b", "c", false)]).unwrap(),
            "a c a"
        );
        let err = apply_edits("a b a", &[edit("a", "c", false)]).unwrap_err();
        assert!(err.contains("occurs 2 times"));
        assert_eq!(
            apply_edits("a b a", &[edit("a", "c", true)]).unwrap(),
            "c b c"
        );
        assert!(
            apply_edits("x", &[edit("y", "z", false)])
                .unwrap_err()
                .contains("not found")
        );
        assert!(apply_edits("x", &[]).is_err());
        assert!(apply_edits("x", &[edit("", "z", false)]).is_err());
    }

    #[test]
    fn edits_apply_in_order() {
        let out = apply_edits(
            "one two",
            &[edit("one", "1", false), edit("1 two", "12", false)],
        );
        assert_eq!(out.unwrap(), "12");
    }
}
