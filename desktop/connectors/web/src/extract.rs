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

/// Containers that *declare* themselves to be the article.
///
/// These are not guesses, so they are not scored against the rest of the page: an author who
/// wrote `<article>` has said where the content is, and the one thing that can outrank that is
/// another semantic container.
const SEMANTIC: &[&str] = &["article", "main", "[role=main]", "[itemprop=articleBody]"];

/// Containers that are *probably* the article, in decreasing order of how strongly they say so.
/// These are guesses, so the best-scoring one wins. `body` is last and is the floor.
const HEURISTIC: &[&str] = &[
    ".post-content",
    ".entry-content",
    ".article-body",
    ".article-content",
    ".markdown-body",
    "#content",
    ".content",
    "body",
];

/// How deeply raw markup may nest before it is not parsed at all.
///
/// Both the HTML parse and the candidate scoring are quadratic in nesting depth — 20 000 levels
/// is a 180 KB page and fifteen seconds of CPU, and the 5 MB cap allows far worse. Neither cost
/// is recoverable once parsing has started, so the only place to stop it is here, on the bytes.
///
/// Counted generously: real documents nest tens of levels, not hundreds.
const MAX_RAW_NESTING: usize = 500;

/// Elements that never close themselves and never close each other, so a scan of the raw bytes
/// can count them and be about right.
///
/// `p`, `li`, `td`, `tr`, `dt`, `dd` and `option` are deliberately absent. The parser closes
/// those implicitly — a page with five thousand unclosed `<p>` is one level deep, not five
/// thousand — and counting them would refuse ordinary pages.
const NESTING_TAGS: &[&str] = &[
    "div",
    "span",
    "ul",
    "ol",
    "dl",
    "table",
    "blockquote",
    "section",
    "article",
    "aside",
    "nav",
    "header",
    "footer",
    "main",
    "form",
    "figure",
    "center",
    "font",
    "a",
    "b",
    "i",
    "em",
    "strong",
    "small",
    "pre",
    "label",
    "fieldset",
    "details",
    "picture",
    "video",
    "audio",
];

/// Whether raw markup nests past [`MAX_RAW_NESTING`], scanned without parsing it.
///
/// Approximate on purpose: it is a bound on work, not a parse. It counts opening and closing
/// tags of the elements in [`NESTING_TAGS`] and answers as soon as the count runs away.
#[must_use]
pub fn nests_too_deep(html: &str) -> bool {
    let bytes = html.as_bytes();
    let mut depth: usize = 0;
    let mut i = 0;
    while let Some(at) = bytes[i..].iter().position(|b| *b == b'<') {
        i += at + 1;
        let closing = bytes.get(i) == Some(&b'/');
        if closing {
            i += 1;
        }
        let start = i;
        while bytes
            .get(i)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'-')
        {
            i += 1;
        }
        if start == i {
            continue; // `<!doctype`, `<!--`, `</>` and other things that are not a tag
        }
        let name = html[start..i].to_ascii_lowercase();
        if !NESTING_TAGS.contains(&name.as_str()) {
            continue;
        }
        if closing {
            depth = depth.saturating_sub(1);
        } else {
            // A self-closing `<div/>` opens nothing, and is rare enough to check for only here.
            let self_closing = bytes[i..]
                .iter()
                .take_while(|b| **b != b'>')
                .last()
                .is_some_and(|b| *b == b'/');
            if !self_closing {
                depth += 1;
                if depth > MAX_RAW_NESTING {
                    return true;
                }
            }
        }
    }
    false
}

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
    // A semantic container wins outright, and is never scored against `body`. Scoring it there
    // could not work: `body` contains it, so `body` always scores at least as much, and any
    // furniture that survived the strip is enough to tip it — which is how a page's promo rail
    // ends up quoted back as part of the article. Length is not the test either; a short note
    // in an `<article>` is still the thing somebody asked to read.
    //
    // The exception is a page that is a *list* of articles: teaser cards on an index, where no
    // one of them is the page, so that falls back to scoring and `body` returns the lot.
    //
    // Counting `<article>` elements is not enough to tell the two apart. A news page routinely
    // marks up its own story and three "more like this" cards the same way, and treating that as
    // an index throws the story away. What separates them is whether one of them *is* the page:
    // on a real article the main text dwarfs the teasers; on an index the cards are siblings of
    // roughly one size.
    let listing = !dominant_article(doc);
    let chosen = match best_of(doc, SEMANTIC) {
        Some((_, selection)) if !listing => Some(selection),
        semantic => match (semantic, best_of(doc, HEURISTIC)) {
            (Some((a, x)), Some((b, y))) => Some(if a >= b { x } else { y }),
            (Some((_, x)), None) => Some(x),
            (None, Some((_, y))) => Some(y),
            (None, None) => None,
        },
    };

    let Some(selection) = chosen else {
        // Nothing scored: a page that is one `<div>` of text, or not really a document at all.
        // Its whole text is a better answer than an empty string — but taken from `<body>`, so
        // the `<title>` does not arrive a second time as the first line of the content.
        let body = doc.select("body");
        let whole = if body.exists() {
            body
        } else {
            doc.select(":root")
        };
        return match format {
            Format::Html => whole.html().to_string(),
            Format::Markdown => whole
                .nodes()
                .first()
                .map(|n| markdown(n, &whole))
                .unwrap_or_default(),
            Format::Text => whole.formatted_text().to_string(),
        };
    };
    match format {
        Format::Markdown => selection
            .nodes()
            .first()
            .map(|n| markdown(n, &selection))
            .unwrap_or_default(),
        Format::Text => selection.formatted_text().to_string(),
        Format::Html => selection.html().to_string(),
    }
}

/// How deep a document may nest before the Markdown serializer is not asked to walk it.
///
/// `dom_query`'s Markdown writer is iterative for ordinary elements but recurses for lists,
/// blockquotes, code and tables — about three stack frames per level, on a tree whose shape came
/// off the network. `<ul><li>x` repeated five thousand times is a 45 KB page, well under the
/// 5 MB cap, and it overflows the stack and aborts the process: not a panic that a `catch_unwind`
/// or the turn loop's `select!` could contain, but the whole app going down because a page was
/// read. Real documents nest a few dozen levels; a limit here costs nothing and removes that.
const MAX_NESTING: usize = 200;

/// The Markdown of one node, or its text when the tree is too deep to serialize safely.
///
/// Falling back rather than refusing: the text extractor is fully iterative, so a page built to
/// overflow the stack still comes back readable, just without its headings and lists.
fn markdown(node: &dom_query::NodeRef<'_>, selection: &dom_query::Selection<'_>) -> String {
    if deeper_than(node, MAX_NESTING) {
        log::warn!("page nests deeper than {MAX_NESTING} levels; returning text, not markdown");
        return selection.formatted_text().to_string();
    }
    unescape_prose(&node.md(Some(&["script", "style", "meta", "head", "nav"])))
}

/// Whether any path below `node` goes deeper than `limit`.
///
/// Iterative, with its own stack, because a recursive depth check would be the bug it is
/// looking for. Stops at the first path that is too deep rather than measuring the whole tree.
fn deeper_than(node: &dom_query::NodeRef<'_>, limit: usize) -> bool {
    let mut stack = vec![(*node, 0usize)];
    while let Some((current, depth)) = stack.pop() {
        if depth > limit {
            return true;
        }
        let mut child = current.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            stack.push((next, depth + 1));
        }
    }
    false
}

/// Whether one `<article>` holds most of the prose in all of them.
///
/// One article is trivially dominant. Several are dominant only if the biggest outweighs the
/// rest put together, which is the shape of a story with teasers beside it and not the shape of
/// an index.
fn dominant_article(doc: &Document) -> bool {
    let mut scores: Vec<usize> = doc
        .select("article")
        .nodes()
        .iter()
        .map(|node| prose_length(&dom_query::Selection::from(*node)))
        .filter(|score| *score > 0)
        .collect();
    if scores.len() <= 1 {
        return true;
    }
    scores.sort_unstable_by(|a, b| b.cmp(a));
    let rest: usize = scores[1..].iter().sum();
    scores[0] > rest
}

/// The highest-scoring element any of `selectors` matches, with ties going to the earlier
/// selector — which is what makes the order of `HEURISTIC` mean something.
fn best_of<'a>(doc: &'a Document, selectors: &[&str]) -> Option<(usize, dom_query::Selection<'a>)> {
    let mut best: Option<(usize, dom_query::Selection<'a>)> = None;
    for selector in selectors {
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
    best
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
/// `a denial of service you wrote yourself\.` with a backslash before every full stop. That is
/// correct Markdown and it is bad reading: the model pays for each backslash by the token.
///
/// The trap is that a backslash in the output is not necessarily one the serializer put there.
/// It never escapes a backslash, so a `\` that was in the page arrives as itself, and stripping
/// backslashes indiscriminately turns `C:\Users\me` into `C:Usersme` and the regex `\d+\.\d+`
/// into `d+.d+` — on exactly the documentation pages this tool exists to read. Three rules keep
/// them apart:
///
/// 1. **Only a character the serializer would have escaped can be carrying an added escape.**
///    `\t` and `\d` are not in its set, so that backslash came from the page and stays. This is
///    what saves Windows paths and regexes.
/// 2. **Inside code, nothing is unescaped at all.** The serializer turns escaping off for code
///    spans and fences, so every backslash in one is literal.
/// 3. **An escape that is doing something stays.** Inline that is the emphasis and link
///    characters; at the start of a line it is also `#`, `+` and `>`, which would otherwise turn
///    a paragraph into a heading, a list or a quote; and a full stop after a leading number,
///    because `1974. The year` is an ordered list where `1974\. The year` is a sentence.
///
/// What is left ambiguous is a literal backslash in prose before one of the seventeen — `\(` in
/// running text. That reads as an added escape and loses its backslash. It is the rarest of the
/// cases and the only one the output cannot distinguish.
fn unescape_prose(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let opener = line.trim_start();
        if opener.starts_with("```") || opener.starts_with("~~~") {
            fenced = !fenced;
            out.push_str(line);
        } else if fenced {
            out.push_str(line);
        } else {
            unescape_line(line, &mut out);
        }
    }
    out
}

/// Characters the Markdown serializer escapes. A backslash before anything else was in the page.
const ESCAPABLE: &[char] = &[
    '`', '*', '_', '{', '}', '[', ']', '<', '>', '(', ')', '#', '+', '.', '!', '|', '"',
];
/// Escapes that change an inline rendering wherever they appear.
///
/// `|` is here because the serializer emits tables: inside a row a bare pipe ends the cell, so
/// `\|` is the only way a cell can contain one, and taking that backslash away splits the row
/// into the wrong number of columns.
const INLINE: &[char] = &['`', '*', '_', '[', ']', '|'];
/// Escapes that only matter as the first thing on a line.
const BLOCK: &[char] = &['#', '+', '-', '>'];

fn unescape_line(line: &str, out: &mut String) {
    let mut chars = line.chars().peekable();
    // Inside a code span the serializer added nothing, so nothing may be taken away.
    let mut code = false;
    // Whether only whitespace has been seen, and whether the line so far is a bare number —
    // which is how "at the start of a line" and "after a number" are answered without looking
    // backwards through `out`.
    let mut at_start = true;
    let mut digits_only = true;

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            if ch == '`' {
                code = !code;
            }
            if !ch.is_whitespace() {
                digits_only = digits_only && ch.is_ascii_digit();
                at_start = false;
            }
            out.push(ch);
            continue;
        }
        // A trailing lone backslash is not an escape at all.
        let Some(next) = chars.next() else {
            out.push('\\');
            break;
        };
        let keep = code
            || !ESCAPABLE.contains(&next)
            || INLINE.contains(&next)
            || (at_start && BLOCK.contains(&next))
            || (next == '.' && digits_only && !at_start);
        if keep {
            out.push('\\');
        }
        // Pushed here rather than left to the next turn of the loop, so an escaped backtick
        // cannot open a code span that was never there.
        out.push(next);
        at_start = false;
        digits_only = false;
    }
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

/// One window of a document: what was returned, and where in the whole it came from.
///
/// Positions are characters, counted from 0, and they are metadata — never written into the
/// text (D8). `last` is where the *next* window starts, so a model paging through a document
/// passes it straight back as `offset` and loses nothing between the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub text: String,
    /// Character this window starts at.
    pub first: usize,
    /// Character the next window starts at; `total` when this was the last one.
    pub last: usize,
    /// Characters in the whole document.
    pub total: usize,
}

impl Window {
    /// Whether there is anything after this window.
    #[must_use]
    pub fn more(&self) -> bool {
        self.last < self.total
    }
}

/// Cut `max_chars` from `text`, starting at `offset`, on a line boundary where there is one
/// near the end — so the model is not handed half a word, and the next window does not begin
/// mid-sentence. Nothing is ever silently dropped: what was left is in `last` and `total`
/// (D9).
///
/// An `offset` past the end is not an error. It returns nothing, says so, and reports the real
/// length, which is what a model that guessed too far needs in order to correct itself.
///
/// A window always advances: `last` is greater than `first` whenever there is anything left, so
/// a caller that keeps passing `last` back in terminates. That is why a budget of zero is read
/// as one character rather than as nothing — a zero-width window with more after it is a loop
/// that never ends.
#[must_use]
pub fn window(text: &str, offset: usize, max_chars: usize) -> Window {
    let total = text.chars().count();
    let first = offset.min(total);
    let short = |last: usize, text: String| Window {
        text,
        first,
        last,
        total,
    };
    if first == total {
        return short(first, String::new());
    }

    let taken: String = text.chars().skip(first).take(max_chars.max(1)).collect();
    let end = first + taken.chars().count();
    if end == total {
        // The rest of the document, so there is no boundary to be tidy about.
        return short(end, taken.trim_end().to_owned());
    }

    // Back up to the last line break, but only if it is late enough that doing so does not
    // throw away most of the budget. `rfind` and `len` are both in bytes, which is what makes
    // them comparable; the ratio is the same either way.
    match taken.rfind('\n').filter(|at| *at * 4 > taken.len() * 3) {
        // The break itself is consumed as the separator, so the next window opens on the line
        // after it rather than on a newline.
        Some(at) => short(
            first + taken[..at].chars().count() + 1,
            taken[..at].trim_end().to_owned(),
        ),
        None => short(end, taken.trim_end().to_owned()),
    }
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
        // `|` is not here: see INLINE. A pipe in a table cell has to keep its backslash.
        assert_eq!(unescape_prose(r"\<not a tag\>"), "<not a tag>");
    }

    #[test]
    fn the_escapes_that_do_something_are_kept() {
        // Inline: dropping these would turn prose into emphasis, code or a link.
        assert_eq!(unescape_prose(r"a \*literal\* star"), r"a \*literal\* star");
        assert_eq!(unescape_prose(r"snake\_case\_name"), r"snake\_case\_name");
        assert_eq!(unescape_prose(r"a \`tick\`"), r"a \`tick\`");
        assert_eq!(unescape_prose(r"\[not a link\]"), r"\[not a link\]");
        // A pipe inside a table cell: without the backslash the row grows a column.
        assert_eq!(unescape_prose(r"| a \| b | c |"), r"| a \| b | c |");
    }

    #[test]
    fn a_line_that_would_become_a_heading_or_a_list_keeps_its_escape() {
        assert_eq!(unescape_prose(r"\# not a heading"), r"\# not a heading");
        assert_eq!(unescape_prose(r"\- not a bullet"), r"\- not a bullet");
        assert_eq!(unescape_prose(r"\> not a quote"), r"\> not a quote");
        assert_eq!(unescape_prose(r"  \+ indented"), r"  \+ indented");
        // `#` mid-sentence is just a character; `-` is never escaped by the serializer at all,
        // so a backslash before one came from the page and stays.
        assert_eq!(unescape_prose(r"a \# b"), "a # b");
        assert_eq!(unescape_prose(r"two \- three"), r"two \- three");
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
    fn a_backslash_the_serializer_could_not_have_added_is_left_alone() {
        // The serializer escapes seventeen characters and `\` is not among them, so a backslash
        // before anything outside that set came from the page. Stripping these turned Windows
        // paths and regexes into nonsense on the documentation pages this tool is for.
        assert_eq!(unescape_prose(r"C:\temp\out"), r"C:\temp\out");
        assert_eq!(unescape_prose(r"C:\Users\me"), r"C:\Users\me");
        assert_eq!(unescape_prose(r"match \d+\s\w+"), r"match \d+\s\w+");
        assert_eq!(unescape_prose(r"a newline is \n"), r"a newline is \n");
    }

    #[test]
    fn nothing_inside_code_is_unescaped() {
        // The serializer turns escaping off for code, so every backslash in a span or a fence
        // is literal.
        assert_eq!(
            unescape_prose(r"use `\d+\.\d+` to match"),
            r"use `\d+\.\d+` to match"
        );
        let fence = concat!("```\n", "let re = r\"\\s+\\.\\w+\";\n", "```");
        assert_eq!(unescape_prose(fence), fence);
        // A fence closes again, so prose after it is still tidied.
        let mixed = concat!("```\n", "\\.\n", "```\n", "and then a sentence\\.");
        let want = concat!("```\n", "\\.\n", "```\n", "and then a sentence.");
        assert_eq!(unescape_prose(mixed), want);
        // An escaped backtick does not open a code span that was never there.
        assert_eq!(
            unescape_prose(r"a \`tick\` then yourself\."),
            r"a \`tick\` then yourself."
        );
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
