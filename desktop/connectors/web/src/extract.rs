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
                n.md(Some(&["script", "style", "meta", "head", "nav"]))
                    .to_string()
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
