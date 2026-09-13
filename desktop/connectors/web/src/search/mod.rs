//! Searching the web without an account (`docs/connectors/web.md` §6).
//!
//! The rule this is built to is §1's first: free, always. No key field, no free tier to exhaust,
//! no upsell. That rules out every general search API worth having, so the architecture spends
//! its effort somewhere else — on *not needing one for most questions*.
//!
//! Gantry is a coding assistant. A large share of what it is really asked is a programming
//! question, a library lookup, or a fact. Each of those has a purpose-built index that is
//! keyless, unrationed in practice, and better at its own subject than any general engine:
//! Wikipedia, Stack Overflow, crates.io, the npm registry. `route` picks among them from the
//! words in the query, every chosen backend runs at once, and the hits come back labelled with
//! which index answered — because "this came from Stack Overflow" is information the model
//! should have before it quotes it.
//!
//! What is deliberately *not* here yet is a general engine for everything else. §6.1 measured
//! that part: DuckDuckGo's Lite endpoint works but blocks after five or six queries in two
//! minutes, and everything freely available is worse. It needs a ration, a circuit breaker and
//! a fallback to independent indexes, which is §6.4 and §6.5 and its own piece of work. Until
//! then a query that matches no index gets a sentence saying so. That is §6.8's rule and it is
//! the important one: an empty list reads to a model as "nothing exists", and it will answer
//! from memory and cite nothing.

mod registries;
mod stack;
mod wikipedia;

use std::fmt;

// Re-exported so the tests can drive each parser against a recorded reply without a socket.
// The request and the parsing are separate for that reason: what breaks in a backend is the
// shape of what comes back, and that is testable only if it can be fed in.
pub use registries::{lookups, parse_crate, parse_crates, parse_npm, parse_package};
pub use stack::parse as parse_stack;
pub use wikipedia::parse as parse_wikipedia;

/// One result, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: Source,
}

/// The indexes this connector can ask. Each is keyless and free; that is the whole membership
/// criterion, and it is why the good paid ones are absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    Wikipedia,
    StackOverflow,
    CratesIo,
    Npm,
}

impl Source {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Wikipedia => "Wikipedia",
            Self::StackOverflow => "Stack Overflow",
            Self::CratesIo => "crates.io",
            Self::Npm => "npm",
        }
    }

    /// The `source` argument's spelling, for a model that wants to choose.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "wikipedia" => Some(Self::Wikipedia),
            "stackoverflow" | "stack overflow" => Some(Self::StackOverflow),
            "crates" | "crates.io" => Some(Self::CratesIo),
            "npm" => Some(Self::Npm),
            _ => None,
        }
    }
}

/// Why a search produced nothing, in the words the model is given.
///
/// Never an empty list (§6.8): "no backend had anything" and "there is no backend for this kind
/// of question" lead to different next moves, and a model that is handed `[]` will conclude the
/// thing does not exist and answer from memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// Nothing in the query said which index could answer it, and the general engine that would
    /// take everything else is not built.
    NoIndex,
    /// Backends were asked and all of them came back empty.
    NothingFound(Vec<Source>),
    /// Every backend asked failed to answer at all.
    Unreachable(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoIndex => write!(
                f,
                "this connector has no index that covers that question. It can search Wikipedia, \
                 Stack Overflow, crates.io and npm; general web search is not built yet, so a \
                 question outside those has to be answered another way — fetch_url on a page you \
                 already know, or the model's own web search if this chat has it."
            ),
            Self::NothingFound(asked) => {
                let names: Vec<&str> = asked.iter().map(|s| s.label()).collect();
                write!(
                    f,
                    "nothing matched in {}. The words may be too specific, or this may be \
                     something those indexes do not cover.",
                    names.join(" or ")
                )
            }
            Self::Unreachable(why) => write!(f, "the search could not be run: {why}"),
        }
    }
}

/// The most results any one call returns.
pub const MAX_RESULTS: usize = 20;

/// Results returned when the model does not say.
pub const DEFAULT_RESULTS: usize = 8;

/// Words that say "this is a package lookup" rather than a question, and which are therefore
/// also words a registry must not be asked to match on. See `registry_query`.
const REGISTRY_WORDS: &[&str] = &[
    "crate", "crates", "cargo", "npm", "package", "packages", "registry", "yarn", "pnpm", "lib",
    "library",
];

/// The query as a registry should receive it.
///
/// crates.io and npm search names and descriptions for the words given, so the words that
/// routed the query there are actively harmful in it: searching crates.io for "serde crate"
/// ranks `serde_core` and `serde-big-array` above `serde`, because the literal word "crate"
/// matches neither and the rest is diluted. Measured against the live API, not guessed — the
/// first live run of this backend returned exactly that.
///
/// Falls back to the original when stripping would leave nothing, so "npm" alone still searches
/// for something.
#[must_use]
pub fn registry_query(query: &str) -> String {
    let kept: Vec<&str> = query
        .split_whitespace()
        .filter(|token| {
            let bare =
                token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
            let lower = bare.to_ascii_lowercase();
            !REGISTRY_WORDS.contains(&lower.as_str())
                && !matches!(lower.as_str(), "the" | "a" | "an" | "for" | "in")
        })
        .collect();
    if kept.is_empty() {
        return query.trim().to_owned();
    }
    kept.join(" ")
}

/// Which indexes a query is worth asking, best first.
///
/// Keyword and pattern matching, not a model call (§6.2), and it errs towards asking one index
/// too many rather than one too few: every backend here is free and they run concurrently, so a
/// wasted question costs milliseconds, while a missed one costs the answer. Wikipedia is the
/// backstop for anything that reads like a question about the world, which is the closest thing
/// to a general engine available without an account.
#[must_use]
pub fn route(query: &str) -> Vec<Source> {
    let q = query.to_ascii_lowercase();
    let word = |w: &str| {
        q.split(|c: char| {
            !c.is_ascii_alphanumeric() && c != '.' && c != '-' && c != '+' && c != '#'
        })
        .any(|token| token == w)
    };
    let any = |ws: &[&str]| ws.iter().any(|w| word(w));

    let mut picked = Vec::new();

    // A named ecosystem is the strongest signal there is: "the tokio crate" is not a question
    // about the world and Wikipedia will answer it badly.
    let rust = any(&["rust", "cargo", "crate", "crates", "rustc", "clippy"]);
    let js = any(&[
        "npm",
        "node",
        "nodejs",
        "javascript",
        "typescript",
        "react",
        "yarn",
        "pnpm",
        "package",
    ]);

    // Naming the ecosystem is not enough on its own. A registry search is a *name* lookup, and
    // a sentence run through one returns whatever crate happens to share a word with it —
    // "cannot borrow as mutable more than once rust" came back with `fp-bench`, which is noise
    // in the two result slots it took. So a registry is asked only when the query looks like a
    // lookup: it says which registry outright, or it is short enough to be a name.
    let package_query = REGISTRY_WORDS.iter().any(|w| word(w)) || q.split_whitespace().count() <= 3;
    if rust && package_query {
        picked.push(Source::CratesIo);
    }
    if js && package_query {
        picked.push(Source::Npm);
    }

    // Programming questions, which is most of what a coding assistant is asked. Error text is
    // the clearest tell of all: nobody types a compiler message into an encyclopedia.
    let programming = rust
        || js
        || any(&[
            "error",
            "exception",
            "panic",
            "traceback",
            "stacktrace",
            "compile",
            "compiler",
            "segfault",
            "undefined",
            "null",
            "async",
            "await",
            "thread",
            "mutex",
            "borrow",
            "lifetime",
            "trait",
            "generic",
            "regex",
            "runtime",
            "segmentation",
            "fault",
            "crash",
            "crashes",
            "malloc",
            "heap",
            "stack",
            "buffer",
            "overflow",
            "syntax",
            "parse",
            "parser",
            "install",
            "config",
            "server",
            "client",
            "socket",
            "port",
            "cache",
            "unicode",
            "encoding",
            "sql",
            "query",
            "api",
            "http",
            "json",
            "docker",
            "kubernetes",
            "git",
            "python",
            "java",
            "kotlin",
            "swift",
            "golang",
            "c++",
            "c#",
            "css",
            "html",
            "linux",
            "bash",
            "shell",
            "function",
            "method",
            "class",
            "struct",
            "enum",
            "array",
            "string",
            "pointer",
            "memory",
            "leak",
            "deadlock",
            "build",
            "linker",
            "import",
            "module",
            "dependency",
            "version",
            "deprecated",
        ]);
    // A short query naming a registry outright — "serde crate", "npm react" — is a lookup and
    // not a question. Everything else routed there is noise: Stack Overflow will return
    // tangents and Wikipedia an unrelated entry that happens to share the word, and an
    // unrelated result beside a correct one is worse than one result. The registry word has to
    // be there: "rust async runtime" names a language, not a package, and is a real question.
    let registry_word = REGISTRY_WORDS.iter().any(|w| word(w));
    if registry_word && q.split_whitespace().count() <= 3 && !picked.is_empty() {
        return picked;
    }

    // One word, and nothing said which world it belongs to. "tokio" and "axum" are the queries
    // this is for: a coding assistant is asked them constantly and an encyclopedia has nothing
    // useful for either. Both registries and the encyclopedia, and the model picks — an honest
    // answer to an ambiguous question is one result from each index rather than a guess at
    // which was meant.
    if picked.is_empty() && !programming && q.split_whitespace().count() == 1 {
        return vec![Source::CratesIo, Source::Npm, Source::Wikipedia];
    }

    if programming {
        picked.push(Source::StackOverflow);
    }
    // Wikipedia last in the order and first in breadth: it is the closest thing to a general
    // engine that needs no account, so it is asked for anything that reads like a question
    // about the world.
    picked.push(Source::Wikipedia);

    picked
}

/// Ask every routed index at once and merge what comes back.
///
/// Concurrent rather than sequential because they are independent and free: the slowest backend
/// sets the latency instead of the sum of them. A backend that fails is dropped rather than
/// failing the search — three good answers and one timeout is a result, not an error — and the
/// call only fails when there is nothing at all to show.
pub async fn run(
    http: &reqwest::Client,
    query: &str,
    limit: usize,
    only: Option<Source>,
) -> Result<Vec<Hit>, SearchError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(SearchError::NoIndex);
    }
    let sources = match only {
        Some(one) => vec![one],
        None => route(query),
    };
    if sources.is_empty() {
        return Err(SearchError::NoIndex);
    }
    let limit = limit.clamp(1, MAX_RESULTS);

    // Per backend, so one index cannot fill the answer on its own when several were asked.
    let each = if sources.len() == 1 {
        limit
    } else {
        limit.div_ceil(sources.len()).max(3)
    };

    let answers = futures_util::future::join_all(sources.iter().map(|source| {
        let source = *source;
        async move {
            let hits = match source {
                Source::Wikipedia => wikipedia::search(http, query, each).await,
                Source::StackOverflow => stack::search(http, query, each).await,
                Source::CratesIo => registries::crates_io(http, &registry_query(query), each).await,
                Source::Npm => registries::npm(http, &registry_query(query), each).await,
            };
            (source, hits)
        }
    }))
    .await;

    let mut hits = Vec::new();
    let mut failures = Vec::new();
    for (source, answer) in answers {
        match answer {
            Ok(found) => hits.extend(found),
            Err(why) => failures.push(format!("{} ({why})", source.label())),
        }
    }
    if !hits.is_empty() {
        // Interleaved rather than concatenated: the first result of each index beats the third
        // of any one of them, and a model reading top-down should not have to get past eight
        // crates to reach the Stack Overflow answer.
        return Ok(interleave(hits, &sources, limit));
    }
    if failures.len() == sources.len() {
        return Err(SearchError::Unreachable(failures.join("; ")));
    }
    Err(SearchError::NothingFound(sources))
}

/// Round-robin the sources so every index that answered is represented near the top.
fn interleave(hits: Vec<Hit>, order: &[Source], limit: usize) -> Vec<Hit> {
    let mut queues: Vec<Vec<Hit>> = order
        .iter()
        .map(|source| {
            hits.iter()
                .filter(|hit| hit.source == *source)
                .cloned()
                .collect()
        })
        .collect();
    let mut out = Vec::with_capacity(limit);
    let mut round = 0;
    while out.len() < limit {
        let mut took = false;
        for queue in &mut queues {
            if round < queue.len() {
                out.push(queue[round].clone());
                took = true;
                if out.len() == limit {
                    return out;
                }
            }
        }
        if !took {
            break;
        }
        round += 1;
    }
    out
}

/// Which indexes answered, each named once, in the order they first appear.
///
/// Not `dedup`: the hits are interleaved on purpose, so the labels arrive as Wikipedia,
/// crates.io, Wikipedia, crates.io and only *consecutive* repeats would go. Not a sort either —
/// the order is the routing order, which is the order of confidence.
#[must_use]
pub fn sources_of(hits: &[Hit]) -> Vec<&'static str> {
    let mut seen: Vec<&'static str> = Vec::new();
    for hit in hits {
        let label = hit.source.label();
        if !seen.contains(&label) {
            seen.push(label);
        }
    }
    seen
}

/// Markup out of a snippet. Search APIs return highlighted HTML, and a model quoting a result
/// should not be quoting `<span class="searchmatch">`.
pub(crate) fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('<') {
        let after = &rest[at + 1..];
        let name = after.strip_prefix('/').unwrap_or(after);
        let tag = name.starts_with(|c: char| c.is_ascii_alphabetic())
            && after.find('>').is_some_and(|end| end <= 64);
        if !tag {
            out.push_str(&rest[..=at]);
            rest = after;
            continue;
        }
        out.push_str(&rest[..at]);
        // `find` succeeded in the check above, so the tag has an end.
        let end = after.find('>').unwrap_or(after.len() - 1);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    decode_entities(&out)
}

pub(crate) fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
        // Last, so that `&amp;lt;` decodes to `&lt;` and not to `<`.
        .replace("&amp;", "&")
}

/// A snippet cut to something a result list can hold, on a word boundary.
pub(crate) fn snippet(text: &str, max_chars: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let cut: String = flat.chars().take(max_chars).collect();
    let end = cut.rfind(' ').unwrap_or(cut.len());
    format!("{}…", cut[..end].trim_end())
}

/// One backend's request, run and parsed.
///
/// Every backend is a fixed host this connector chose, so `guard` has nothing to do here: there
/// is no model-supplied address to refuse. What the model supplies is the query, and it reaches
/// the URL through `Url::parse_with_params`, which is what keeps a query containing `&` from
/// becoming a second parameter.
pub(crate) async fn get_json(
    http: &reqwest::Client,
    url: url::Url,
) -> Result<serde_json::Value, String> {
    let response = http
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|err| brief(&err.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("answered {}", status.as_u16()));
    }
    response.json().await.map_err(|err| {
        format!(
            "sent something that is not JSON: {}",
            brief(&err.to_string())
        )
    })
}

/// Transport errors read as a stack trace by the time reqwest is done with them; one clause is
/// what the model can act on.
fn brief(message: &str) -> String {
    message
        .split(':')
        .next()
        .unwrap_or(message)
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sentence_is_a_question_and_not_a_package_lookup() {
        // Measured against the live registries: a sentence run through crates.io comes back
        // with whatever crate shares a word with it — "cannot borrow as mutable more than once
        // rust" returned `fp-bench` — and every junk hit costs a slot a real answer wanted.
        assert_eq!(
            route("how do I share a tokio runtime between rust threads"),
            [Source::StackOverflow, Source::Wikipedia]
        );
        assert_eq!(
            route("cannot borrow as mutable more than once rust"),
            [Source::StackOverflow, Source::Wikipedia]
        );
        // Short enough to be a name, so the registry is worth asking.
        assert_eq!(
            route("tokio runtime"),
            [Source::StackOverflow, Source::Wikipedia],
            "no ecosystem named, so no registry"
        );
        assert_eq!(
            route("rust tokio runtime"),
            [Source::CratesIo, Source::StackOverflow, Source::Wikipedia]
        );
    }

    #[test]
    fn a_registry_is_not_asked_to_match_the_word_that_routed_the_query() {
        // The first live run of this backend searched crates.io for "serde crate" and ranked
        // `serde_core` and `serde-big-array` above `serde`. The literal routing word matches no
        // package and dilutes everything that would.
        assert_eq!(registry_query("serde crate"), "serde");
        assert_eq!(registry_query("npm react"), "react");
        assert_eq!(registry_query("the tokio crate for async"), "tokio async");
        // Stripping everything would leave the registry nothing to search.
        assert_eq!(registry_query("npm"), "npm");
        assert_eq!(registry_query("crate"), "crate");
    }

    #[test]
    fn a_bare_package_lookup_does_not_ask_an_encyclopedia() {
        // "the serde crate" in Wikipedia is either nothing or something unrelated, and an
        // unrelated encyclopedia entry beside a correct crate is worse than one result.
        assert_eq!(route("serde crate"), [Source::CratesIo]);
        assert_eq!(route("npm react"), [Source::Npm]);
        // Naming the language is not naming a package: this one is a real question and keeps
        // the question indexes.
        assert_eq!(
            route("rust async runtime"),
            [Source::CratesIo, Source::StackOverflow, Source::Wikipedia]
        );
        // Given a sentence rather than a name, the encyclopedia is worth asking again.
        assert!(
            route("why did the react team rewrite the reconciler").contains(&Source::Wikipedia)
        );
    }

    #[test]
    fn an_error_message_is_a_programming_question() {
        let hits = route("cannot borrow `x` as mutable more than once at a time");
        assert!(hits.contains(&Source::StackOverflow), "{hits:?}");
    }

    #[test]
    fn a_question_about_the_world_gets_the_encyclopedia() {
        assert_eq!(route("who is magnus carlsen"), [Source::Wikipedia]);
        assert_eq!(route("norwegian chess history"), [Source::Wikipedia]);
    }

    #[test]
    fn a_word_is_matched_whole_and_not_inside_another() {
        // "carlsen" contains no keyword; "apinomics" must not match `api`, or every query
        // containing a common substring would be routed as a programming question.
        assert_eq!(route("apinomics of nullity"), [Source::Wikipedia]);
        assert!(route("rusty nails").contains(&Source::Wikipedia));
        assert!(
            !route("rusty nails").contains(&Source::CratesIo),
            "`rusty` is not `rust`"
        );
    }

    #[test]
    fn a_bare_name_asks_both_registries_and_the_encyclopedia() {
        // "tokio" and "axum" are among the most likely one-word queries a coding assistant
        // gets, and nothing in either says which ecosystem it is. Wikipedia alone answers them
        // with nothing useful. One result from each index is the honest answer to an ambiguous
        // question; guessing which was meant is not.
        for name in ["tokio", "axum", "express"] {
            assert_eq!(
                route(name),
                [Source::CratesIo, Source::Npm, Source::Wikipedia],
                "{name}"
            );
        }
        // A one-word query that is plainly a programming word keeps the question indexes.
        assert_eq!(
            route("deadlock"),
            [Source::StackOverflow, Source::Wikipedia]
        );
    }

    /// The router in one table, which is the only honest way to review it.
    ///
    /// It is keyword matching and it will always miss something; what this pins down is that it
    /// misses in the safe direction. Every row was checked by hand, and `source` on the tool is
    /// the escape hatch for the rows a future reader disagrees with.
    #[test]
    fn realistic_queries_reach_the_index_that_can_answer_them() {
        use Source::{CratesIo, Npm, StackOverflow, Wikipedia};
        let cases: &[(&str, &[Source])] = &[
            ("who won the 2024 world chess championship", &[Wikipedia]),
            ("what is the capital of norway", &[Wikipedia]),
            ("how does photosynthesis work", &[Wikipedia]),
            ("history of the norwegian language", &[Wikipedia]),
            ("rust lifetime elision rules", &[StackOverflow, Wikipedia]),
            (
                "TypeError: Cannot read properties of undefined",
                &[StackOverflow, Wikipedia],
            ),
            (
                "segmentation fault in C when freeing twice",
                &[StackOverflow, Wikipedia],
            ),
            (
                "best crate for parsing toml",
                &[CratesIo, StackOverflow, Wikipedia],
            ),
            ("react useEffect cleanup", &[Npm, StackOverflow, Wikipedia]),
            (
                "npm install fails with EACCES",
                &[Npm, StackOverflow, Wikipedia],
            ),
        ];
        for (query, expected) in cases {
            assert_eq!(&route(query)[..], *expected, "{query}");
        }
    }

    #[test]
    fn nothing_to_search_is_an_error_and_not_an_empty_list() {
        // §6.8: the one shape that must never happen is `[]`, which a model reads as proof the
        // thing does not exist.
        let no_index = SearchError::NoIndex.to_string();
        assert!(
            no_index.contains("general web search is not built yet"),
            "{no_index}"
        );
        assert!(no_index.contains("fetch_url"), "it says what to do instead");

        let empty =
            SearchError::NothingFound(vec![Source::Wikipedia, Source::CratesIo]).to_string();
        assert!(empty.contains("Wikipedia or crates.io"), "{empty}");
    }

    #[test]
    fn markup_is_not_part_of_a_snippet() {
        assert_eq!(
            strip_tags(r#"Sven <span class="searchmatch">Magnus</span> Øen Carlsen"#),
            "Sven Magnus Øen Carlsen"
        );
        assert_eq!(strip_tags("a &lt;T&gt; and &amp;str"), "a <T> and &str");
        // A stray angle bracket is text, not a tag, and the words around it survive.
        assert_eq!(strip_tags("if a < b and b > c"), "if a < b and b > c");
    }

    #[test]
    fn a_snippet_is_cut_on_a_word() {
        let long = "the quick brown fox jumps over the lazy dog and keeps going";
        let cut = snippet(long, 20);
        assert!(cut.ends_with('…'));
        assert!(cut.chars().count() <= 21, "{cut}");
        assert!(!cut.contains("  "));
        assert_eq!(snippet("short\n  text", 100), "short text");
    }

    #[test]
    fn the_indexes_that_answered_are_named_once_each() {
        let hit = |source: Source| Hit {
            title: String::new(),
            url: String::new(),
            snippet: String::new(),
            source,
        };
        // As `interleave` leaves them, which is where the obvious `dedup` goes wrong.
        let hits = [
            hit(Source::Wikipedia),
            hit(Source::CratesIo),
            hit(Source::Wikipedia),
            hit(Source::CratesIo),
        ];
        assert_eq!(sources_of(&hits), ["Wikipedia", "crates.io"]);
        assert!(sources_of(&[]).is_empty());
    }

    #[test]
    fn results_are_interleaved_so_every_index_is_visible() {
        let hit = |source: Source, n: usize| Hit {
            title: format!("{n}"),
            url: String::new(),
            snippet: String::new(),
            source,
        };
        let hits = vec![
            hit(Source::CratesIo, 1),
            hit(Source::CratesIo, 2),
            hit(Source::CratesIo, 3),
            hit(Source::StackOverflow, 1),
            hit(Source::StackOverflow, 2),
        ];
        let merged = interleave(hits, &[Source::CratesIo, Source::StackOverflow], 4);
        assert_eq!(
            merged.iter().map(|h| h.source).collect::<Vec<_>>(),
            [
                Source::CratesIo,
                Source::StackOverflow,
                Source::CratesIo,
                Source::StackOverflow
            ]
        );
    }
}
