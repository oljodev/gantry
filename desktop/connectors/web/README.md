# Web

Fetch pages as readable text and search the web with your own search key.

First-party, native (Rust) connector. Design: `docs/plan/03-connector-system.md` §5.

## Tools

| Tool | Input → output | Tier |
|------|----------------|------|
| `fetch_url` | `{ url, format?: markdown\|text\|html, max_chars? }` → `{ url, title, content, status, chars, truncated, redirects }` | read (internet) |
| `search` | `{ query, max_results? }` → `{ results: { title, url, snippet }[] }` | read (internet) |

`search` is offered only when a search key is configured; see below.

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
- **Size.** 5 MB on the wire, and `max_chars` on what reaches the model, with both losses
  reported separately so the model knows which it is looking at.

## Search keys (BYOK)

Gantry has no search account and buys nobody's quota. Two `user_config` fields, both optional:

- `SEARCH_PROVIDER` — `brave`, `tavily` or `exa`. Deliberately without a default: a key pasted
  against a pre-filled provider name would be sent to a service that did not issue it.
- `SEARCH_API_KEY` — the user's own key, declared `sensitive`, so it goes to the vault as a
  `user_config_secret` credential and the config row keeps only the field name (03 §11 step 2,
  06 §3).

With no key the `search` tool is not in the tool list at all rather than failing when it is
called: a tool the model can see and cannot use costs a round to find out.

## Tests

`tests/` is offline. HTML and search responses are recorded in `tests/fixtures/`, and `cargo
test` reaches no host on the internet — verified under `strace`, which shows loopback and
nothing else. The refusal tests in `tests/tools.rs` in particular prove their point only because
no request is made. The one socket the suite does open is a local resolver lookup for
`localhost`, which is why that test accepts either refusal: on a machine whose hosts file lacks
the name, it is refused as unresolvable rather than as private, and both are correct.

The one live test is `#[ignore]`d and reads its key from the environment:

```
GANTRY_SEARCH_PROVIDER=brave GANTRY_SEARCH_KEY=… \
  cargo test -p gantry-connector-web --test search -- --ignored --nocapture
```
