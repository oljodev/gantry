//! `cargo xtask validate-connectors` (docs/plan/17 §5): what the schema cannot express and what
//! spans more than one file.
//!
//! The JSON Schema itself is checked by ajv in `desktop/frontend/tests/schemas.test.ts`, which
//! runs on every `pnpm test`; duplicating it in Rust would be a second copy of one rule to keep
//! in step. What is left is everything a per-file schema cannot see: that an id matches its
//! folder and no other connector's, that the files a manifest names are actually there, that two
//! entries in one category do not claim the same place in the list, and — the one invariant 17 §7
//! states and nothing enforced — that the website and the catalogue describe the same set of
//! connectors. A site that promises a connector the app does not ship is a promise nobody in the
//! app can keep.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, bail};

/// One connector folder, as far as this check needs to understand it.
struct Entry {
    id: String,
    category: String,
    sort_weight: f64,
}

pub fn validate(root: &Path) -> anyhow::Result<()> {
    let dir = root.join("desktop/connectors");
    let mut problems: Vec<String> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();

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
        let path = folder.join("manifest.json");
        if !path.exists() {
            problems.push(format!("{name}: no manifest.json"));
            continue;
        }
        let text =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        // The real parse, not a lenient one: this is what the app does with the same bytes, so a
        // manifest that passes here is one the app can install.
        let manifest = match gantry_connectors::manifest::Manifest::parse(&text) {
            Ok(manifest) => manifest,
            Err(err) => {
                problems.push(format!("{name}: {err}"));
                continue;
            }
        };
        if manifest.id != name {
            problems.push(format!(
                "{name}: the manifest declares the id `{}`; the folder name is the id",
                manifest.id
            ));
        }
        let icon = manifest
            .icon
            .clone()
            .unwrap_or_else(|| "icon.svg".to_owned());
        for file in [icon.as_str(), "README.md"] {
            if !folder.join(file).exists() {
                problems.push(format!("{name}: {file} is named but missing"));
            }
        }
        for term in &manifest.catalog.suggest_for {
            if term != &term.to_lowercase() {
                problems.push(format!(
                    "{name}: suggest_for `{term}` is matched lowercase, so it can never fire"
                ));
            }
        }
        entries.push(Entry {
            id: manifest.id.clone(),
            category: manifest.category.clone(),
            sort_weight: manifest.catalog.sort_weight,
        });
    }

    // An id twice over is two connectors the user cannot tell apart and one instance table that
    // cannot hold both.
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for entry in &entries {
        if !seen.insert(entry.id.as_str()) {
            problems.push(format!("{}: the id is used by another folder", entry.id));
        }
    }

    // Inside one category the weight is the order; two entries claiming one place sort by name,
    // which is not what whoever wrote the second weight meant to happen.
    let mut weights: BTreeMap<(String, String), Vec<&str>> = BTreeMap::new();
    for entry in &entries {
        // Zero is "unweighted", which every entry that never thought about it shares.
        if entry.sort_weight != 0.0 {
            weights
                .entry((entry.category.clone(), entry.sort_weight.to_string()))
                .or_default()
                .push(&entry.id);
        }
    }
    for ((category, weight), ids) in &weights {
        if ids.len() > 1 {
            problems.push(format!(
                "{category}: {} both claim sort_weight {weight}",
                ids.join(" and ")
            ));
        }
    }

    problems.extend(site_parity(root, &entries)?);

    if problems.is_empty() {
        println!("{} connectors, all valid", entries.len());
        return Ok(());
    }
    for problem in &problems {
        eprintln!("  {problem}");
    }
    bail!("{} problem(s) in desktop/connectors", problems.len())
}

/// 17 §7, in its own words: every `available` row has a folder in `desktop/connectors/`, and
/// every folder has a row.
///
/// Two different failures, and the asymmetry is deliberate. A site that says `available` for a
/// connector no release contains is a promise nobody in the app can keep, so that is checked
/// against the folders. A folder with no row at all is the other way round — something shipped
/// and the catalogue page never heard about it — so that is checked against every row whatever
/// its status. A folder whose row says `soon` is neither: `web` has a manifest and a crate and
/// no tools yet, and `soon` is the true thing to say about it.
fn site_parity(root: &Path, entries: &[Entry]) -> anyhow::Result<Vec<String>> {
    let path = root.join("web/site/src/data/connectors.ts");
    let Ok(source) = fs::read_to_string(&path) else {
        // The site is a separate package and may legitimately be absent from a sparse checkout.
        return Ok(Vec::new());
    };
    let (available, listed) = site_slugs(&source);
    let folders: BTreeSet<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    let mut problems = Vec::new();
    for slug in &available {
        if !folders.contains(slug.as_str()) {
            problems.push(format!(
                "the site lists `{slug}` as available, but desktop/connectors/{slug} does not exist"
            ));
        }
    }
    for id in &folders {
        if !listed.contains(*id) {
            problems.push(format!(
                "desktop/connectors/{id} ships, but connectors.ts has no row for it"
            ));
        }
    }
    Ok(problems)
}

/// Every slug in the file, and the subset whose entry says `status: 'available'`.
///
/// Read with a scanner rather than a parser: the file is TypeScript, the entries are object
/// literals, and running `tsc` from a Rust developer task to learn two fields would be a build
/// dependency in exchange for nothing. Each `slug:` is paired with the `status:` that follows it
/// before the next slug, which is that entry's own — whether the literal is written on one line
/// or on six.
fn site_slugs(source: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut available, mut listed) = (BTreeSet::new(), BTreeSet::new());
    let mut rest = source;
    while let Some(at) = rest.find("slug:") {
        let after = &rest[at + "slug:".len()..];
        let Some(slug) = quoted(after) else { break };
        let end = after.find("slug:").unwrap_or(after.len());
        if let Some(status_at) = after[..end].find("status:")
            && quoted(&after[status_at + "status:".len()..]).as_deref() == Some("available")
        {
            available.insert(slug.clone());
        }
        listed.insert(slug);
        rest = after;
    }
    (available, listed)
}

/// The first single-quoted or double-quoted run in a fragment of TypeScript.
fn quoted(rest: &str) -> Option<String> {
    let chars: Vec<char> = rest.chars().collect();
    let open = chars.iter().position(|c| *c == '\'' || *c == '"')?;
    let quote = chars[open];
    let close = chars[open + 1..].iter().position(|c| *c == quote)? + open + 1;
    Some(chars[open + 1..close].iter().collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_slug_counts_only_when_its_entry_says_available() {
        // Both layouts the file actually uses: the fields on one line, and spread over several.
        let source = r#"
          { slug: 'filesystem', name: 'Files', status: 'available', category: 'local',
            does: 'Read files' },
          { slug: 'linear', name: 'Linear', status: 'soon' },
          {
            slug: "github",
            status: "available",
          },
        "#;
        let (available, listed) = super::site_slugs(source);
        assert!(available.contains("filesystem") && available.contains("github"));
        assert!(
            !available.contains("linear"),
            "`soon` is a plan, not a promise"
        );
        assert!(
            listed.contains("linear"),
            "but it is still a row, and a folder needs one"
        );
    }
}
