//! Reading and writing `SKILL.md` (docs/plan/12 §A2).
//!
//! The file is an Agent Skills file: a `---` frontmatter block, then Markdown. The frontmatter
//! is YAML, but only a corner of YAML — flat `key: value` pairs, one nested `metadata:` block,
//! and the occasional block list — so this is a parser for that corner rather than a YAML
//! dependency. A file that uses more of YAML than this parses as far as it can and the fields
//! it could not read are reported to the user on the import screen, which is the same answer a
//! stricter parser would give and a friendlier one than refusing the file.
//!
//! Nothing here writes a file it did not render: an **imported** skill is stored byte for byte
//! as the author wrote it, so an unknown field (`allowed-tools`, somebody else's extension)
//! survives the trip. Only a skill Gantry authored or edited is rendered from the fields.

use std::collections::BTreeMap;

use gantry_core::{SkillInput, skill};

/// The frontmatter of a `SKILL.md`, as far as Gantry reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    /// Everything under `metadata:`, including the `gantry-` keys.
    pub metadata: BTreeMap<String, String>,
}

impl Frontmatter {
    /// `metadata.gantry-triggers`, split on commas and trimmed.
    #[must_use]
    pub fn triggers(&self) -> Vec<String> {
        self.metadata
            .get("gantry-triggers")
            .map(|v| {
                v.split(',')
                    .map(|t| t.trim().to_lowercase())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[must_use]
    pub fn always(&self) -> bool {
        self.metadata
            .get("gantry-always")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }

    #[must_use]
    pub fn version(&self) -> u32 {
        self.metadata
            .get("gantry-version")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1)
    }

    #[must_use]
    pub fn author(&self) -> Option<String> {
        self.metadata.get("author").cloned()
    }
}

/// What a file turned into, and anything about it worth saying out loud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub front: Frontmatter,
    pub body: String,
    /// Lines of frontmatter this parser did not understand, by key. Shown on the import screen
    /// rather than swallowed, because a dropped field is the sort of thing a user finds out
    /// about weeks later.
    pub unread: Vec<String>,
}

/// Splits a `SKILL.md` into its frontmatter and its body.
///
/// # Errors
/// When the file has no `---` frontmatter block at all, or no `name`.
pub fn parse(text: &str) -> Result<Parsed, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = text
        .strip_prefix("---\r\n")
        .or_else(|| text.strip_prefix("---\n"))
        .ok_or_else(|| {
            "This file has no frontmatter. A skill starts with a `---` block holding at least a \
             name and a description."
                .to_owned()
        })?;
    let end = rest
        .split_inclusive('\n')
        .scan(0usize, |at, line| {
            let start = *at;
            *at += line.len();
            Some((start, line))
        })
        .find(|(_, line)| line.trim_end() == "---")
        .map(|(start, _)| start)
        .ok_or_else(|| "The frontmatter block is never closed with a second `---`.".to_owned())?;
    let (front_text, after) = rest.split_at(end);
    let body = after
        .split_once('\n')
        .map_or("", |(_, b)| b)
        .trim_start_matches('\n')
        .to_owned();

    let (front, unread) = parse_frontmatter(front_text);
    if front.name.is_empty() {
        return Err("The frontmatter has no `name`, which is also the folder name.".to_owned());
    }
    Ok(Parsed {
        front,
        body,
        unread,
    })
}

fn parse_frontmatter(text: &str) -> (Frontmatter, Vec<String>) {
    let mut front = Frontmatter::default();
    let mut unread = Vec::new();
    // The key a block list or a nested block currently belongs to, and whether we are inside
    // `metadata:`.
    let mut list_key: Option<String> = None;
    let mut list_items: Vec<String> = Vec::new();
    let mut in_metadata = false;

    let flush = |key: &mut Option<String>,
                 items: &mut Vec<String>,
                 front: &mut Frontmatter,
                 in_metadata: bool| {
        if let Some(k) = key.take() {
            let joined = std::mem::take(items).join(", ");
            set(front, &k, &joined, in_metadata);
        }
    };

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();

        if let Some(item) = trimmed.strip_prefix("- ") {
            if list_key.is_some() {
                list_items.push(unquote(item).to_owned());
            } else {
                unread.push(trimmed.to_owned());
            }
            continue;
        }
        flush(&mut list_key, &mut list_items, &mut front, in_metadata);

        let Some((key, value)) = trimmed.split_once(':') else {
            unread.push(trimmed.to_owned());
            continue;
        };
        let key = key.trim().to_lowercase();
        let value = unquote(value.trim());

        if indent == 0 {
            in_metadata = key == "metadata";
            if in_metadata {
                continue;
            }
        }
        if value.is_empty() {
            // `key:` on its own opens a block list or a nested block; a nested block other
            // than `metadata` is more YAML than this parser reads.
            list_key = Some(key);
            continue;
        }
        if indent == 0
            && !matches!(
                key.as_str(),
                "name" | "description" | "license" | "compatibility"
            )
        {
            unread.push(trimmed.to_owned());
            continue;
        }
        set(&mut front, &key, value, in_metadata && indent > 0);
    }
    flush(&mut list_key, &mut list_items, &mut front, in_metadata);
    (front, unread)
}

fn set(front: &mut Frontmatter, key: &str, value: &str, in_metadata: bool) {
    if in_metadata {
        front.metadata.insert(key.to_owned(), value.to_owned());
        return;
    }
    match key {
        "name" => front.name = value.to_owned(),
        "description" => front.description = value.to_owned(),
        "license" => front.license = Some(value.to_owned()),
        "compatibility" => front.compatibility = Some(value.to_owned()),
        _ => {}
    }
}

fn unquote(v: &str) -> &str {
    let v = v.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return &v[1..v.len() - 1];
        }
    }
    v
}

/// Renders the canonical `SKILL.md` for a skill Gantry authored or edited.
#[must_use]
pub fn render(input: &SkillInput, version: u32) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("name: {}\n", input.name));
    out.push_str(&format!("description: {}\n", one_line(&input.description)));
    if let Some(license) = input.license.as_deref().filter(|l| !l.trim().is_empty()) {
        out.push_str(&format!("license: {license}\n"));
    }
    out.push_str("metadata:\n");
    if !input.triggers.is_empty() {
        out.push_str(&format!(
            "  gantry-triggers: \"{}\"\n",
            input.triggers.join(", ").replace('"', "'")
        ));
    }
    out.push_str(&format!("  gantry-always: \"{}\"\n", input.always_include));
    out.push_str(&format!("  gantry-version: \"{version}\"\n"));
    if let Some(author) = input.author.as_deref().filter(|a| !a.trim().is_empty()) {
        out.push_str(&format!("  author: {author}\n"));
    }
    out.push_str("---\n\n");
    out.push_str(input.body.trim_end());
    out.push('\n');
    out
}

/// A description is one line of frontmatter, so a newline in it would end the field.
fn one_line(text: &str) -> String {
    let flat: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(skill::DESCRIPTION_MAX)
        .collect();
    flat
}

/// The `SkillInput` a parsed file stands for.
#[must_use]
pub fn to_input(parsed: &Parsed, references: Vec<gantry_core::SkillReference>) -> SkillInput {
    SkillInput {
        name: parsed.front.name.clone(),
        description: parsed.front.description.clone(),
        triggers: parsed.front.triggers(),
        always_include: parsed.front.always(),
        author: parsed.front.author(),
        license: parsed.front.license.clone(),
        body: parsed.body.clone(),
        references,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "---\nname: rust-idioms\ndescription: \"Idiomatic Rust: errors, ownership.\"\nlicense: MIT\nmetadata:\n  gantry-triggers: \"rust, borrow checker, lifetime\"\n  gantry-always: \"true\"\n  gantry-version: \"3\"\n  author: olav\n---\n\n# Rust idioms\n\nBody text.\n";

    #[test]
    fn a_skill_file_parses_into_its_fields_and_its_body() {
        let p = parse(FILE).unwrap();
        assert_eq!(p.front.name, "rust-idioms");
        assert_eq!(p.front.description, "Idiomatic Rust: errors, ownership.");
        assert_eq!(p.front.license.as_deref(), Some("MIT"));
        assert_eq!(p.front.triggers(), ["rust", "borrow checker", "lifetime"]);
        assert!(p.front.always());
        assert_eq!(p.front.version(), 3);
        assert_eq!(p.front.author().as_deref(), Some("olav"));
        assert_eq!(p.body, "# Rust idioms\n\nBody text.\n");
        assert!(p.unread.is_empty(), "{:?}", p.unread);
    }

    #[test]
    fn a_block_list_is_read_and_a_foreign_field_is_reported_rather_than_dropped() {
        let file = "---\nname: x\ndescription: d\nallowed-tools: Read, Bash\nmetadata:\n  gantry-triggers:\n    - one\n    - \"two words\"\n---\nbody\n";
        let p = parse(file).unwrap();
        assert_eq!(p.front.triggers(), ["one", "two words"]);
        assert_eq!(p.unread, ["allowed-tools: Read, Bash"]);
    }

    #[test]
    fn a_file_without_frontmatter_says_so_instead_of_guessing() {
        assert!(parse("# Just markdown\n").is_err());
        assert!(parse("---\nname: x\n").is_err(), "unclosed block");
        assert!(parse("---\ndescription: d\n---\nbody").is_err(), "no name");
    }

    #[test]
    fn what_we_render_is_what_we_parse_back() {
        let input = to_input(&parse(FILE).unwrap(), Vec::new());
        let rendered = render(&input, 4);
        let again = parse(&rendered).unwrap();
        assert_eq!(again.front.name, "rust-idioms");
        assert_eq!(again.front.description, input.description);
        assert_eq!(again.front.triggers(), input.triggers);
        assert!(again.front.always());
        assert_eq!(again.front.version(), 4);
        assert_eq!(again.body.trim(), "# Rust idioms\n\nBody text.".trim());
    }

    #[test]
    fn a_description_written_across_lines_still_ends_up_on_one() {
        let input = SkillInput {
            name: "x".into(),
            description: "first line\nsecond line".into(),
            body: "b".into(),
            ..SkillInput::default()
        };
        let parsed = parse(&render(&input, 1)).unwrap();
        assert_eq!(parsed.front.description, "first line second line");
    }
}
