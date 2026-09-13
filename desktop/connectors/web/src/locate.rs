//! Finding your way around a page without reading all of it.
//!
//! `fetch_url` returns a window and says where the next one starts, which makes a long document
//! *reachable* but still sequential: a 287 000-character article is seven reads at the default
//! budget, and six of them are thrown away if the answer is in the last section. This is the
//! index to that document. Ask for its headings, or for where a pattern occurs, and the answer
//! is a list of character offsets to hand straight back to `fetch_url`.
//!
//! Everything here is a pure function of text that has already been fetched, so it costs no
//! request — the document is the one in `cache` — and it is all testable without a socket.
//!
//! Offsets are per rendering. The same page as Markdown and as plain text are different strings
//! of different lengths, so a position found in one is meaningless in the other. Both this and
//! `fetch_url` take the same `format`, and the result says which one it is measured in.

/// Headings returned for one page. A long specification can have hundreds; past this the list
/// is no longer an index anybody can read, and the pattern search is the better tool.
pub const MAX_HEADINGS: usize = 200;

/// Matches returned for one pattern.
pub const MAX_MATCHES: usize = 50;

/// How much of a matching line comes back, so a result list stays a list.
const CONTEXT_CHARS: usize = 200;

/// One entry in a page's table of contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// 1 for `#`, 2 for `##`, and so on.
    pub level: usize,
    pub title: String,
    /// Character offset of the heading line itself, so fetching from here starts at the
    /// heading rather than after it.
    pub offset: usize,
}

/// One place a pattern occurs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub offset: usize,
    /// The line the match is on, shortened.
    pub line: String,
    /// The nearest heading above it, which is the section the match is in. `None` before the
    /// first heading, or in a page that has none.
    pub section: Option<String>,
}

/// The headings of a Markdown rendering, in document order.
///
/// ATX only (`#` through `######`), which is what the extractor emits. A `#` inside a fenced
/// code block is not a heading — shell comments and C preprocessor directives both start with
/// one, and a page of either would otherwise produce an outline made entirely of code.
#[must_use]
pub fn outline(body: &str) -> (Vec<Heading>, bool) {
    let mut headings = Vec::new();
    let mut fenced = false;
    for (offset, line) in lines(body) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let hashes = line.chars().take_while(|c| *c == '#').count();
        // `#hashtag` is not a heading; the space is what makes it one.
        if (1..=6).contains(&hashes) && line.chars().nth(hashes) == Some(' ') {
            let title = line[hashes + 1..].trim().trim_end_matches('#').trim();
            if !title.is_empty() {
                if headings.len() == MAX_HEADINGS {
                    return (headings, true);
                }
                headings.push(Heading {
                    level: hashes,
                    title: title.to_owned(),
                    offset,
                });
            }
        }
    }
    (headings, false)
}

/// Where a pattern occurs, with the section each occurrence is in.
///
/// One match per line: a model is deciding where to read, and ten offsets on the same line are
/// one place. The regular expression is the caller's, built the way `filesystem.grep` builds
/// one, so a pattern that works in one works in the other.
#[must_use]
pub fn find(body: &str, pattern: &regex::Regex, limit: usize) -> (Vec<Found>, bool) {
    let limit = limit.min(MAX_MATCHES);
    let mut found = Vec::new();
    let mut section: Option<String> = None;
    let mut fenced = false;
    for (offset, line) in lines(body) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
        } else if !fenced {
            let hashes = line.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&hashes) && line.chars().nth(hashes) == Some(' ') {
                let title = line[hashes + 1..].trim().trim_end_matches('#').trim();
                if !title.is_empty() {
                    section = Some(title.to_owned());
                }
            }
        }
        if pattern.is_match(line) {
            if found.len() == limit {
                return (found, true);
            }
            found.push(Found {
                offset,
                line: shorten(line),
                section: section.clone(),
            });
        }
    }
    (found, false)
}

/// Lines with the character offset each one starts at.
///
/// Characters rather than bytes, because that is what `fetch_url` counts in and the whole
/// purpose of an offset here is to be handed to it.
fn lines(body: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    body.split('\n').map(move |line| {
        let start = at;
        // The newline the split consumed is a character too, or every offset after the first
        // line would be short by the number of lines before it.
        at += line.chars().count() + 1;
        (start, line.trim_end_matches('\r'))
    })
}

fn shorten(line: &str) -> String {
    let flat = line.trim();
    if flat.chars().count() <= CONTEXT_CHARS {
        return flat.to_owned();
    }
    let cut: String = flat.chars().take(CONTEXT_CHARS).collect();
    let end = cut.rfind(' ').unwrap_or(cut.len());
    format!("{}…", cut[..end].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = "# The Title\n\nIntro prose.\n\n## First section\n\nSome words about tokio.\n\n```rust\n# not a heading, a doc attribute\nlet x = 1;\n```\n\n### Deeper\n\nMore about tokio here.\n\n## Second section\n\nNothing to see.\n";

    #[test]
    fn the_outline_is_the_headings_with_where_they_start() {
        let (headings, more) = outline(PAGE);
        assert!(!more);
        assert_eq!(
            headings
                .iter()
                .map(|h| (h.level, h.title.as_str()))
                .collect::<Vec<_>>(),
            [
                (1, "The Title"),
                (2, "First section"),
                (3, "Deeper"),
                (2, "Second section")
            ]
        );
        // An offset points at the heading line itself, so reading from it starts at the
        // heading rather than after it.
        for heading in &headings {
            let from: String = PAGE.chars().skip(heading.offset).collect();
            assert!(
                from.starts_with(&"#".repeat(heading.level)),
                "offset {} is not the heading {:?}: {:?}",
                heading.offset,
                heading.title,
                from.chars().take(20).collect::<String>()
            );
        }
    }

    #[test]
    fn a_hash_inside_a_code_fence_is_not_a_heading() {
        // A page of shell or Rust doc attributes would otherwise outline as its own comments.
        let (headings, _) = outline(PAGE);
        assert!(
            !headings.iter().any(|h| h.title.contains("doc attribute")),
            "{headings:?}"
        );
        assert!(!outline("```\n# ls -la\n```\n").0.iter().any(|_| true));
        // And a hash with no space after it is a hashtag, not a heading.
        assert!(outline("#nottitle\n").0.is_empty());
        assert!(outline("####### too many\n").0.is_empty());
    }

    #[test]
    fn a_pattern_comes_back_with_the_section_it_is_in() {
        let pattern = regex::Regex::new("(?i)tokio").unwrap();
        let (found, more) = find(PAGE, &pattern, 50);
        assert!(!more);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].section.as_deref(), Some("First section"));
        assert_eq!(found[1].section.as_deref(), Some("Deeper"));
        assert_eq!(found[0].line, "Some words about tokio.");
        // The offset is the line, ready for fetch_url.
        let from: String = PAGE.chars().skip(found[1].offset).collect();
        assert!(from.starts_with("More about tokio"), "{from:?}");
    }

    #[test]
    fn an_offset_is_in_characters_and_not_in_bytes() {
        // The whole point of these offsets is that fetch_url takes them, and fetch_url counts
        // characters. A page with an em dash before the match would be off by one per dash.
        let page = "# Título — en español\n\nmás texto — con guiones\nel objetivo\n";
        let pattern = regex::Regex::new("objetivo").unwrap();
        let (found, _) = find(page, &pattern, 10);
        assert_eq!(found.len(), 1);
        let from: String = page.chars().skip(found[0].offset).collect();
        assert!(from.starts_with("el objetivo"), "{from:?}");
    }

    #[test]
    fn one_match_per_line_however_often_it_occurs_on_it() {
        let pattern = regex::Regex::new("x").unwrap();
        let (found, _) = find("x x x x x\ny\nx\n", &pattern, 50);
        assert_eq!(found.len(), 2, "a line is one place to read from");
    }

    #[test]
    fn a_page_with_more_than_the_cap_says_so() {
        let many = (0..MAX_MATCHES + 10)
            .map(|n| format!("hit {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let pattern = regex::Regex::new("hit").unwrap();
        let (found, more) = find(&many, &pattern, MAX_MATCHES);
        assert_eq!(found.len(), MAX_MATCHES);
        assert!(more, "nothing is silently dropped");

        let headings = (0..MAX_HEADINGS + 5)
            .map(|n| format!("# heading {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (listed, more) = outline(&headings);
        assert_eq!(listed.len(), MAX_HEADINGS);
        assert!(more);
    }

    #[test]
    fn a_page_with_no_headings_is_an_empty_outline_and_not_an_error() {
        let (headings, more) = outline("just prose\n\nand more prose\n");
        assert!(headings.is_empty());
        assert!(!more);
    }
}
