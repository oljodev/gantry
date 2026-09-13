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

mod general;
mod registries;
mod stack;
mod wikipedia;

use std::fmt;

// Re-exported so the tests can drive each parser against a recorded reply without a socket.
// The request and the parsing are separate for that reason: what breaks in a backend is the
// shape of what comes back, and that is testable only if it can be fed in.
pub use general::{COOLDOWN, GAP, Ration, Spent, parse_duckduckgo, parse_mwmbl};
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
    /// General web search, rationed. See `general`.
    DuckDuckGo,
    /// The independent index that answers when DuckDuckGo cannot be asked.
    Mwmbl,
}

impl Source {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Wikipedia => "Wikipedia",
            Self::StackOverflow => "Stack Overflow",
            Self::CratesIo => "crates.io",
            Self::Npm => "npm",
            Self::DuckDuckGo => "DuckDuckGo",
            Self::Mwmbl => "mwmbl",
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
            "web" | "duckduckgo" | "general" => Some(Self::DuckDuckGo),
            "mwmbl" => Some(Self::Mwmbl),
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
            Self::NoIndex => write!(f, "there is nothing to search for. Give a query."),
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

/// What a search produced: the hits, and any index that could not be reached.
///
/// The second half exists because a backend that fails is dropped rather than failing the
/// search — three good answers and one timeout is a result, not an error — and a loss nothing
/// reports is a loss nobody can act on (D9). A model that asked about Rust and got no Stack
/// Overflow results should be able to tell "nobody has asked that" from "Stack Overflow was
/// down".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub hits: Vec<Hit>,
    /// One line per index that was asked and could not answer, naming it and why.
    pub unavailable: Vec<String>,
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
        return vec![
            Source::CratesIo,
            Source::Npm,
            Source::Wikipedia,
            Source::DuckDuckGo,
        ];
    }

    if programming {
        picked.push(Source::StackOverflow);
    }
    // Wikipedia before the general engine: it is free, unrationed and better than a general
    // result for anything it covers.
    picked.push(Source::Wikipedia);
    // And the open web last, for everything the four curated indexes do not hold —
    // documentation, release notes, blog posts, an RFC, anything recent. It is rationed rather
    // than unlimited, and `general` decides whether this particular query gets the engine or
    // the independent index behind it.
    picked.push(Source::DuckDuckGo);

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
    ration: &Ration,
    query: &str,
    limit: usize,
    only: Option<Source>,
) -> Result<Answer, SearchError> {
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
                Source::DuckDuckGo => general::search(http, ration, query, each).await,
                Source::Mwmbl => general::mwmbl_only(http, query, each).await,
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
        return Ok(Answer {
            hits: interleave(hits, &sources, limit),
            unavailable: failures,
        });
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

    /// Every route ends at the open web, because the four curated indexes hold no documentation,
    /// no release notes, no blog posts and nothing recent. Whether that last step actually
    /// spends a general query is `general`'s decision under its ration, not the router's.
    #[test]
    fn realistic_queries_reach_the_index_that_can_answer_them() {
        use Source::{CratesIo, DuckDuckGo, Npm, StackOverflow, Wikipedia};
        let cases: &[(&str, &[Source])] = &[
            // Questions about the world: the encyclopedia, then the open web.
            ("who is magnus carlsen", &[Wikipedia, DuckDuckGo]),
            ("what is the capital of norway", &[Wikipedia, DuckDuckGo]),
            ("how does photosynthesis work", &[Wikipedia, DuckDuckGo]),
            (
                "history of the norwegian language",
                &[Wikipedia, DuckDuckGo],
            ),
            ("apinomics of nullity", &[Wikipedia, DuckDuckGo]),
            // Programming questions. A sentence is never run through a package registry: one
            // returns whatever crate shares a word with it, measured.
            (
                "rust lifetime elision rules",
                &[StackOverflow, Wikipedia, DuckDuckGo],
            ),
            (
                "TypeError: Cannot read properties of undefined",
                &[StackOverflow, Wikipedia, DuckDuckGo],
            ),
            (
                "segmentation fault in C when freeing twice",
                &[StackOverflow, Wikipedia, DuckDuckGo],
            ),
            (
                "cannot borrow as mutable more than once rust",
                &[StackOverflow, Wikipedia, DuckDuckGo],
            ),
            (
                "how do I share a tokio runtime between rust threads",
                &[StackOverflow, Wikipedia, DuckDuckGo],
            ),
            ("tokio runtime", &[StackOverflow, Wikipedia, DuckDuckGo]),
            // Short enough to be a name, with an ecosystem named: the registry joins in.
            (
                "rust tokio runtime",
                &[CratesIo, StackOverflow, Wikipedia, DuckDuckGo],
            ),
            (
                "best crate for parsing toml",
                &[CratesIo, StackOverflow, Wikipedia, DuckDuckGo],
            ),
            (
                "react useEffect cleanup",
                &[Npm, StackOverflow, Wikipedia, DuckDuckGo],
            ),
            (
                "npm install fails with EACCES",
                &[Npm, StackOverflow, Wikipedia, DuckDuckGo],
            ),
            // Naming a registry outright, briefly, is a lookup and stops there: an encyclopedia
            // entry for an unrelated word of the same name is worse than one result.
            ("serde crate", &[CratesIo]),
            ("npm react", &[Npm]),
            // One word and no ecosystem: ask everything that could hold it, and let the model
            // choose. An encyclopedia alone has nothing useful for "tokio" or "axum".
            ("tokio", &[CratesIo, Npm, Wikipedia, DuckDuckGo]),
            ("axum", &[CratesIo, Npm, Wikipedia, DuckDuckGo]),
            // ...unless the one word is plainly a programming word.
            ("deadlock", &[StackOverflow, Wikipedia, DuckDuckGo]),
        ];
        for (query, expected) in cases {
            assert_eq!(&route(query)[..], *expected, "{query}");
        }
    }

    #[test]
    fn a_word_is_matched_whole_and_not_inside_another() {
        // "apinomics" must not match `api`, or every query containing a common substring would
        // be routed as a programming question.
        assert!(!route("apinomics of nullity").contains(&Source::StackOverflow));
        assert!(
            !route("rusty nails").contains(&Source::CratesIo),
            "`rusty` is not `rust`"
        );
    }

    #[test]
    fn an_empty_query_is_an_error_and_a_miss_names_what_was_asked() {
        // §6.8: the one shape that must never happen is `[]`, which a model reads as proof the
        // thing does not exist.
        assert_eq!(
            SearchError::NoIndex.to_string(),
            "there is nothing to search for. Give a query."
        );
        let empty =
            SearchError::NothingFound(vec![Source::Wikipedia, Source::CratesIo]).to_string();
        assert!(empty.contains("Wikipedia or crates.io"), "{empty}");
    }

    #[test]
    fn every_index_has_a_name_that_can_be_asked_for_by_name() {
        for source in [
            Source::Wikipedia,
            Source::StackOverflow,
            Source::CratesIo,
            Source::Npm,
            Source::DuckDuckGo,
            Source::Mwmbl,
        ] {
            let label = source.label().to_ascii_lowercase();
            assert_eq!(
                Source::parse(&label),
                Some(source),
                "{label} is shown to the model but cannot be asked for"
            );
        }
        assert_eq!(Source::parse("google"), None);
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
}
