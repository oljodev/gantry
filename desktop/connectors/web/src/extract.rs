//! HTML in, readable text out.
//!
//! A page is mostly not the page: navigation, cookie banners, share buttons, related-article
//! rails and three newsletter prompts, around the few paragraphs somebody asked for. Handing all
//! of it to a model costs context and buries the answer, so this does what a reader mode does —
//! strip the furniture, pick the block that holds the prose, and serialize that.
//!
//! It is deliberately a small readability rather than a port of one. The scoring is the part of
//! the original algorithm that earns its keep: paragraph text length per candidate container,
//! with the obvious furniture removed first. What is dropped is the iterative re-scoring, which
//! costs a lot of code to change the answer on very few pages.

use dom_query::Document;

/// What a page turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Article {
    pub title: Option<String>,
    pub body: String,
}

/// How the body comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Headings, links and lists survive as Markdown. The default: it is the cheapest form that
    /// keeps the structure a model needs to quote a page accurately.
    #[default]
    Markdown,
    /// Prose only.
    Text,
    /// The extracted subtree as HTML, for when the markup itself is the question.
    Html,
}

impl Format {
    /// The manifest's `format` argument. Anything unrecognised is `None`, so the tool can say so
    /// rather than silently returning something else.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "markdown" => Some(Self::Markdown),
            "text" => Some(Self::Text),
            "html" => Some(Self::Html),
            _ => None,
        }
    }
}

/// Elements that are never content, removed before anything is scored.
const FURNITURE: &[&str] = &[
    "script",
    "style",
    "noscript",
    "template",
    "svg",
    "iframe",
    "object",
    "embed",
    "canvas",
    "form",
    "button",
    "input",
    "select",
    "textarea",
    "nav",
    "aside",
    "footer",
    "dialog",
    "[aria-hidden=true]",
    "[hidden]",
    "[role=navigation]",
    "[role=banner]",
    "[role=complementary]",
    "[role=search]",
    "[role=dialog]",
];

/// Words that mark a container as furniture wherever they appear in its id or class. Matched as
/// substrings, which is what makes `site-footer-nav` and `RelatedPostsWrapper` both go.
const FURNITURE_WORDS: &[&str] = &[
    "nav",
    "menu",
    "sidebar",
    "side-bar",
    "footer",
    "header-bar",
    "masthead",
    "breadcrumb",
    "pagination",
    "comment",
    "disqus",
    "share",
    "social",
    "subscribe",
    "newsletter",
    "signup",
    "sign-up",
    "promo",
    "advert",
    "-ad-",
    "banner",
    "cookie",
    "consent",
    "gdpr",
    "popup",
    "modal",
    "overlay",
    "related",
    "recirc",
    "recommend",
    "trending",
    "skip-link",
    "screen-reader",
    "sr-only",
    "visually-hidden",
];

/// Containers that usually *are* the article, tried in order of how strongly they say so. A
/// match is still scored: a site that wraps its whole page in `<main>` gets no benefit from it.
const CANDIDATES: &[&str] = &[
    "article",
    "main",
    "[role=main]",
    "[itemprop=articleBody]",
    ".post-content",
    ".entry-content",
    ".article-body",
    ".article-content",
    ".markdown-body",
    "#content",
    ".content",
    "body",
];

/// Pull the title and the readable body out of a page.
#[must_use]
pub fn article(html: &str, format: Format) -> Article {
    let doc = Document::from(html);
    let title = title(&doc);
    strip_furniture(&doc);
    let body = best_body(&doc, format);
    Article {
        title,
        body: tidy(&body),
    }
}

/// What the page calls itself. `og:title` first: it is the title an author wrote for humans,
/// where `<title>` is very often the same string with " | Site Name" welded on.
fn title(doc: &Document) -> Option<String> {
    let meta = |sel: &str| {
        let node = doc.select_single(sel);
        node.exists()
            .then(|| node.attr("content"))
            .flatten()
            .map(|v| v.to_string())
    };
    let candidates = [
        meta(r#"meta[property="og:title"]"#),
        meta(r#"meta[name="twitter:title"]"#),
        Some(doc.select_single("title").text().to_string()),
        Some(doc.select_single("h1").text().to_string()),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|t| collapse(&t))
        .find(|t| !t.is_empty())
}

fn strip_furniture(doc: &Document) {
    for selector in FURNITURE {
        doc.select(selector).remove();
    }
    // `<header>` is only furniture at the top of a page; inside an article it holds the
    // headline, so the ones that are descendants of a candidate are left alone.
    doc.select("body > header, body > div > header").remove();

    for node in doc.select("div, section, aside, ul, ol, span, p").nodes() {
        let selection = dom_query::Selection::from(*node);
        let id = selection.attr("id").unwrap_or_default().to_string();
        let class = selection.attr("class").unwrap_or_default().to_string();
        if id.is_empty() && class.is_empty() {
            continue;
        }
        let haystack = format!("{id} {class}").to_lowercase();
        // "-ad-" and friends are bracketed with spaces so that `download` and `header` do not
        // match `-ad-` and `header-bar` by accident.
        let padded = format!(" {} ", haystack.replace(['_', '.'], "-"));
        if FURNITURE_WORDS.iter().any(|word| padded.contains(word)) {
            selection.remove();
        }
    }
}

/// The candidate with the most prose in it.
///
/// Prose means the text inside `<p>`, `<li>` and headings rather than all text, which is the
/// whole trick: a navigation column has plenty of characters and almost no paragraphs, so it
/// loses to three real paragraphs even when it is longer.
fn best_body(doc: &Document, format: Format) -> String {
    let mut best: Option<(usize, dom_query::Selection<'_>)> = None;
    for selector in CANDIDATES {
        for node in doc.select(selector).nodes() {
            let selection = dom_query::Selection::from(*node);
            let score = prose_length(&selection);
            if score == 0 {
                continue;
            }
            if best.as_ref().is_none_or(|(top, _)| score > *top) {
                best = Some((score, selection));
            }
        }
    }
    let Some((_, selection)) = best else {
        // Nothing scored: a page that is one `<div>` of text, or not really a document at all.
        // Its whole text is a better answer than an empty string.
        return match format {
            Format::Html => doc.html().to_string(),
            _ => doc.formatted_text().to_string(),
        };
    };
    match format {
        Format::Markdown => selection
            .nodes()
            .first()
            .map(|n| {
                let md = n.md(Some(&["script", "style", "meta", "head", "nav"]));
                unescape_prose(&md)
            })
            .unwrap_or_default(),
        Format::Text => selection.formatted_text().to_string(),
        Format::Html => selection.html().to_string(),
    }
}

fn prose_length(selection: &dom_query::Selection<'_>) -> usize {
    selection
        .select("p, li, h1, h2, h3, h4, blockquote, pre, td")
        .iter()
        .map(|el| el.text().trim().chars().count())
        .sum()
}

/// Take back the Markdown escapes that were never needed.
///
/// The serializer escapes seventeen characters everywhere, so ordinary prose comes out as
/// `a denial of service you wrote yourself\.` with a backslash before every full stop, bracket
/// and quotation mark. That is correct Markdown and it is bad reading: the model pays for each
/// backslash by the token and has to see past all of them to quote a sentence back.
///
/// So the escape is kept only where it does something. Inline, that is the emphasis and link
/// characters — `` ` ``, `*`, `_`, `[`, `]` — which change how a line renders. At the start of a
/// line it is also `#`, `+`, `-` and `>`, which would otherwise turn a paragraph into a heading,
/// a list or a quote. And a full stop after a leading number stays escaped, because `1974. The
/// year` is an ordered list where `1974\. The year` is a sentence. Everything else loses the
/// backslash.
fn unescape_prose(text: &str) -> String {
    /// Escapes that change an inline rendering wherever they appear.
    const INLINE: &[char] = &['`', '*', '_', '[', ']'];
    /// Escapes that only matter as the first thing on a line.
    const BLOCK: &[char] = &['#', '+', '-', '>'];

    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    // What the line holds so far, which is how "at the start of a line" and "after a number"
    // are answered without looking backwards through `out`.
    let mut line_start = true;
    let mut digits_only = true;

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            if ch == '\n' {
                line_start = true;
                digits_only = true;
            } else if !ch.is_whitespace() {
                digits_only = digits_only && ch.is_ascii_digit();
                line_start = false;
            }
            out.push(ch);
            continue;
        }
        let Some(&next) = chars.peek() else {
            out.push(ch);
            continue;
        };
        // `\\` is a literal backslash: both characters stay, and the second must not then be
        // read as the start of another escape.
        let keep = next == '\\'
            || INLINE.contains(&next)
            || (line_start && BLOCK.contains(&next))
            || (next == '.' && digits_only && !line_start);
        if !keep {
            // Drop the backslash; the character itself is pushed by the next turn of the loop.
            continue;
        }
        out.push(ch);
        if next == '\\' {
            out.push(next);
            chars.next();
            line_start = false;
            digits_only = false;
        }
    }
    out
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Trailing spaces go, runs of blank lines collapse to one. A model pays for whitespace by the
/// token like everything else, and reader output is mostly whitespace before this runs.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blanks = 0;
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blanks += 1;
            if blanks > 1 {
                continue;
            }
            out.push('\n');
        } else {
            blanks = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_owned()
}

/// Cut to a character budget on a line boundary where there is one nearby, so the model is not
/// handed half a word. Returns whether anything was dropped, which the result reports (D9:
/// nothing is silently truncated).
#[must_use]
pub fn clamp(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_owned(), false);
    }
    let cut: String = text.chars().take(max_chars).collect();
    let on_a_line = cut
        .rfind('\n')
        .filter(|at| *at * 4 > cut.len() * 3)
        .map_or(cut.as_str(), |at| &cut[..at]);
    (on_a_line.trim_end().to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prose_loses_the_escapes_it_never_needed() {
        assert_eq!(
            unescape_prose(r"you wrote it yourself\."),
            "you wrote it yourself."
        );
        assert_eq!(unescape_prose(r"a claim \(mostly\)"), "a claim (mostly)");
        assert_eq!(unescape_prose(r#"he said \"no\""#), r#"he said "no""#);
        assert_eq!(unescape_prose(r"C\+\+ and C\#"), "C++ and C#");
        assert_eq!(unescape_prose(r"really\!"), "really!");
        assert_eq!(unescape_prose(r"a \| b"), "a | b");
        assert_eq!(unescape_prose(r"\<not a tag\>"), "<not a tag>");
    }

    #[test]
    fn the_escapes_that_do_something_are_kept() {
        // Inline: dropping these would turn prose into emphasis, code or a link.
        assert_eq!(unescape_prose(r"a \*literal\* star"), r"a \*literal\* star");
        assert_eq!(unescape_prose(r"snake\_case\_name"), r"snake\_case\_name");
        assert_eq!(unescape_prose(r"a \`tick\`"), r"a \`tick\`");
        assert_eq!(unescape_prose(r"\[not a link\]"), r"\[not a link\]");
    }

    #[test]
    fn a_line_that_would_become_a_heading_or_a_list_keeps_its_escape() {
        assert_eq!(unescape_prose(r"\# not a heading"), r"\# not a heading");
        assert_eq!(unescape_prose(r"\- not a bullet"), r"\- not a bullet");
        assert_eq!(unescape_prose(r"\> not a quote"), r"\> not a quote");
        assert_eq!(unescape_prose(r"  \+ indented"), r"  \+ indented");
        // The same characters mid-sentence are just characters.
        assert_eq!(unescape_prose(r"two \- three"), "two - three");
        assert_eq!(unescape_prose(r"a \# b"), "a # b");
    }

    #[test]
    fn a_year_at_the_start_of_a_line_stays_a_year() {
        // `1974. The year` would be an ordered list; the escape is what keeps it a sentence.
        assert_eq!(unescape_prose(r"1974\. The year"), r"1974\. The year");
        // Mid-sentence there is no list to be mistaken for.
        assert_eq!(unescape_prose(r"born in 1974\. He"), "born in 1974. He");
        // Nor is there when the line does not start with digits.
        assert_eq!(unescape_prose(r"Rust 1\.93"), "Rust 1.93");
    }

    #[test]
    fn a_literal_backslash_survives() {
        assert_eq!(unescape_prose(r"C:\\Users\\me"), r"C:\\Users\\me");
        // A trailing backslash with nothing after it is not an escape at all.
        assert_eq!(unescape_prose("ends with "), "ends with ");
    }

    #[test]
    fn text_that_never_had_an_escape_is_returned_as_it_was() {
        let plain = "Nothing here needs escaping at all.\n\nTwo paragraphs, in fact.";
        assert_eq!(unescape_prose(plain), plain);
    }
}
