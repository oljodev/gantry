# Web

Fetch pages as readable text. No key, no account, nothing to pay for.

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

Searching is not built. When it is, it will be keyless: a query router over purpose-built free
APIs first (Wikipedia, Stack Exchange, crates.io, OpenAlex, RFC Editor…), the user's own SearXNG
if they run one, a rationed general engine after that, and independent indexes when that is
spent. `docs/connectors/web.md` §6 has the measurements behind each tier.

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

No test here needs a key, because nothing here takes one. When search lands, its tests record
each backend's response the same way.
