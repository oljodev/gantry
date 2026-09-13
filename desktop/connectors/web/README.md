# Web

Search the web and read pages as text. No key, no account, nothing to pay for.

First-party, native (Rust) connector. Design: `docs/plan/03-connector-system.md` §5, and
`docs/connectors/web.md` for where search is going.

## The rule

**Free and local, always.** This connector asks the user for no key, holds no account, and has
no quota anybody can exhaust or buy their way out of. That is rule 1 of `docs/connectors/web.md`
§1 and it is not negotiable against convenience.

An earlier build of this connector took a Brave, Tavily or Exa key through `user_config`. It was
removed: a key field is precisely the thing this connector may not have, whoever pays for it.
`tests` asserts the manifest asks for nothing, so it cannot come back by accident.

## Tools

| Tool | Input → output | Tier |
|------|----------------|------|
| `fetch_url` | `{ url, offset?, max_chars?, format?: markdown\|text\|html }` → `{ url, title, content, status, chars, first_char, total_chars, more, next_offset, redirects }` | read (internet) |
| `search` | `{ query, source?, max_results? }` → `{ results: { title, url, snippet, source }[], searched, count }` | read (internet) |

## Search without an account

Four indexes, all keyless, each better at its own subject than a general engine would be:

| Index | For |
|-------|-----|
| Wikipedia | Facts, people, places, events — and the backstop for anything with no other signal |
| Stack Overflow | Programming questions and error messages |
| crates.io | Rust packages |
| npm | JavaScript packages |

`route` in `src/search/mod.rs` picks from the words in the query; every chosen index is asked
concurrently and the hits are interleaved, so a model reading top-down sees each index near the
top rather than eight crates before the first answer. Each hit carries the index it came from.

Two rules the live APIs taught, both now tests:

- **A sentence is not a package lookup.** A registry matches names and descriptions, so a
  question run through one returns whatever crate shares a word with it — `cannot borrow as
  mutable more than once rust` came back with `fp-bench`. A registry is asked only when the
  query names one outright or is short enough to be a name.
- **The word that routed the query must not be searched for.** crates.io searched for `serde
  crate` ranks `serde_core` and `serde-big-array` above `serde`, because the literal word
  matches no package and dilutes the word that does. `registry_query` strips it.
- **Searching a registry is not enough, and re-ranking cannot save it.** Both registries match
  every word against name *and* description, so `tokio async runtime` does not return `tokio`
  anywhere in thirty results — tokio's own description never says "runtime". The package
  everybody meant is not in the list to re-rank. Every search is therefore paired with an exact
  by-name lookup of up to three words from the query (`/api/v1/crates/{name}`,
  `registry.npmjs.org/{name}/latest`), which 404s for a word that is not a package and costs
  nothing when it does.
- **Not every namesake deserves a slot.** `runtime` is a real crate at 0.0.0 with 100 thousand
  downloads; `tokio` has 962 million. `dominant` keeps an exact match only if it is within a
  thousandth of the most-used one — relative, because "popular" is not a number that holds
  across ecosystems. `serde` and `json` are comparable and both stay; `runtime` and
  `serialization` go. A lone match is always kept.

The known weakness is the router itself: it is keyword matching, so it will always miss
something. `realistic_queries_reach_the_index_that_can_answer_them` pins down a table of hand-
checked cases, and the tool's `source` argument is the escape hatch when the words alone would
route a query wrongly. Expect to add keywords as real use finds the holes — two were already
found this way, `segmentation fault` reaching only an encyclopedia and a bare `tokio` reaching
nothing useful at all.

**A question no index covers is an error, not an empty list** (`docs/connectors/web.md` §6.8).
There is no general engine here; `[]` reads to a model as "this does not exist", and it will
answer from memory and cite nothing. The message names the four indexes and says to use
`fetch_url` or the model's own search instead.

Still to build, in `docs/connectors/web.md` §6 order: the user's own SearXNG (§6.3), a rationed
DuckDuckGo Lite with a circuit breaker (§6.4), and mwmbl/Wiby as the tier below that (§6.5).
§6.7 is the honest ceiling — one search and several reads per question works forever and free;
an agent wanting five searches while reasoning will feel the ration once §6.4 exists.

## What it may reach

Unlike the other first-party connectors this one touches no disk, so it has no roots to enforce.
Its boundary is what it may *reach*, and `src/guard.rs` is the whole of it:

- **http and https only.** `file:`, `data:`, `ftp:` and the rest are refused.
- **The public internet only.** Loopback, the private ranges, carrier-grade NAT, link-local
  (169.254.169.254 among it), unique-local IPv6 and IPv4-mapped forms of any of them are refused.
- **Checked again on every redirect.** Redirects are followed by hand rather than by reqwest, so
  a public URL cannot bounce the fetch to `http://127.0.0.1:6379/`.
- **5 MB, 5 redirects, 20 seconds, no cookies.**

IPv6 transition formats are decoded before they are judged: `2002:7f00:1::` is 6to4 for
127.0.0.1 and `64:ff9b::a00:1` is NAT64 for 10.0.0.1, and neither is reachable through this tool.

Not covered: DNS rebinding. The name is resolved for the check and resolved again by the
connection, and a record with a one-second TTL can differ between the two. Closing that means
connecting to the address that was checked rather than to the name, which reqwest does not
expose.

## What a hostile page cannot do

A page is untrusted input that arrives as bytes and leaves as text in a prompt, so the cost of
reading one is bounded on both sides:

- **Nesting.** `<ul><li>x` repeated five thousand times is a 45 KB page that used to abort the
  process — `dom_query`'s Markdown writer recurses once per level. Raw markup is scanned for
  nesting depth before it is parsed at all (both the parse and the scoring are quadratic in
  depth), and the serializer gets a second depth check behind that.
- **Blocking.** Parsing happens on a blocking thread. Nothing in it awaits, so on an async
  worker it would hold a runtime thread for its whole duration and the turn's cancellation could
  never interrupt it.
- **Size.** 5 MB on the wire, and `max_chars` on what reaches one reply. Only the first is a
  loss: past `max_chars` the page is paged rather than cut, and the two are reported separately
  so the model knows which it is looking at.

## Reading a long page

`fetch_url` returns a window, not a truncation. `offset` says where to start; the result says
where the window sits (`first_char`, `total_chars`, `more`) and, when there is more, the
`next_offset` to pass back. The names match `filesystem.read_file`'s rather than the MCP fetch
server's `start_index`/`max_length`, so a model holding both tools meets one convention.

`src/cache.rs` keeps the extracted document for five minutes, bounded at 8 pages and 4 M
characters. That is what makes paging worth doing — reading on is neither a second download nor
a second parse, and the offsets cannot go stale between two calls — and it also means a chat
that reads one URL twice pays for it once. What is cached is public content fetched anonymously
with no cookies, so one cache across the connector leaks nothing between chats; a result served
from it says `cached` and how old it is.

## Upgrading an old install

An instance installed from an earlier release — when this manifest declared no tools — catches
up on the next start: `rebuild` reconciles a native connector's recorded tool list with what the
build offers, so the Connectors page stops saying "No tools yet" without anyone pressing
**Refresh tools**.

## Tests

`tests/` is offline. Pages are recorded in `tests/fixtures/`, and `cargo test` reaches no host
on the internet — verified under `strace`, which shows loopback and nothing else. The refusal tests in `tests/tools.rs` in particular prove their point only because
no request is made. The one socket the suite does open is a local resolver lookup for
`localhost`, which is why that test accepts either refusal: on a machine whose hosts file lacks
the name, it is refused as unresolvable rather than as private, and both are correct.

Search fixtures are real. Every one in `tests/fixtures/*.json` was captured from the live API
on 2026-09-13 and saved verbatim — `wikipedia.json` from `en.wikipedia.org/w/api.php`,
`stackexchange.json` from `api.stackexchange.com`, `crates.json` from `crates.io/api/v1`,
`npm.json` from `registry.npmjs.org`. The connector's previous search was built against
hand-written fixtures and never met a real response; its parsers were fiction that compiled.

The one live test is `#[ignore]`d and needs no key, which is the point:

```
cargo test -p gantry-connector-web --test search -- --ignored --nocapture
```

It is worth running when an API might have moved. Both routing bugs above were found by it and
by nothing else — a recorded response cannot tell you that you sent the wrong query.
