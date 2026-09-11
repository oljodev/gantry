---
name: add-connector
description: Add one MCP server to Gantry's curated connector catalogue — probe it, write desktop/connectors/<id>/, add the website row, and verify. Use when asked to add a connector, ship a catalogue batch (B1–B11 in docs/plan/17), or when a connector request comes in.
---

# Adding a connector

A catalogue entry is a set of claims about somebody else's server: this is where it lives, this
is how it wants to be signed into, these tools are safe and this one deletes things. The point of
this recipe is that every claim is **checked against the server** before it ships, and recorded so
CI can keep checking it.

`docs/plan/17-connector-catalog.md` is which connectors, in what order, and why. This is how one
of them is written. `docs/plan/03-connector-system.md` §3 is the manifest, §6 the risk tiers, §7
the auth shapes.

## Before you start

Read the batch row in 17 §3. A batch groups by **auth shape**, and the shape decides the work: a
batch that reuses a proven shape is an afternoon, one that introduces a shape needs Olav to sign
in once. If the server's shape is new to the catalogue, say so up front — it changes what you can
finish alone.

Gantry speaks **streamable HTTP only**. A vendor who serves only `/sse` is out of the catalogue
until they ship it; do not add one hoping it works. A `401` at `/mcp` proves the path is served,
not that the transport is right — the challenge comes before any negotiation.

## 1. Probe it first

Write nothing until the server has answered. Ask for its tool list the way Gantry does:

```sh
curl -s -X POST <url> \
  -H 'Content-Type: application/json' \
  -H 'Accept: application/json, text/event-stream' \
  -H 'MCP-Protocol-Version: 2026-07-28' \
  -H 'Mcp-Method: tools/list' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{
        "io.modelcontextprotocol/protocolVersion":"2026-07-28",
        "io.modelcontextprotocol/clientCapabilities":{}}}}'
```

What the answers mean:

| Answer | What it is | `auth.type` |
|---|---|---|
| `200` with a tool list | open server | `none` |
| `401` with `WWW-Authenticate` | OAuth; follow it to the metadata to see whether registration is offered | `oauth2` |
| `400` "No valid session. Send initialize first." | older revision, session-based — fine, the client handles it | as the 401 says |
| `400` "Unsupported protocol version" | older revision — also fine | as the 401 says |
| connection refused, or only `/sse` works | not shippable | — |

A `400` about sessions or versions is **not** a failure. Gantry's client falls back to the
`initialize` handshake, and so does `cargo xtask probe-connectors`. Do not change the URL because
of one.

## 2. Write the folder

`desktop/connectors/<id>/` with three files. The id is lowercase, hyphenated, and **equals the
folder name** — `validate-connectors` enforces it.

- **`manifest.json`** — copy `desktop/connectors/cloudflare-docs/manifest.json` and work through
  it. `description` is one line for a tile; `long_description` is a short paragraph for the page,
  and it should say what the server can and cannot see. `keywords` and `catalog.suggest_for` are
  lowercase (enforced), and `suggest_for` is what a person would *type*, not what the vendor calls
  itself. `catalog.sort_weight` must not collide inside a category (enforced).
- **`icon.svg`** — a 24×24 stroke glyph, `fill="none" stroke="currentColor" stroke-width="1.8"`,
  matching the other first-party marks. Not the vendor's logo: the marks are one family, and
  trademark permission is a question nobody here has answered (17 §8). The website uses Simple
  Icons separately, which is a different question with a different answer.
- **`README.md`** — Runs / Needs / Can reach / Protocol, in that order, in plain sentences. "Can
  reach" is the one a user actually needs: say what the server sees of theirs.

## 3. Tiers are the judgement

Runtime-discovered tools take `risk.default_tool_tier`. Override the ones the rules in 17 §6 name,
and the ones no regex catches:

- `delete_`, `remove_`, `drop_`, `revoke_`, `cancel_`, `refund_`, `transfer_`, `pay_` →
  `destructive` **and `always_confirm: true`**.
- `create_`, `update_`, `send_`, `post_`, `deploy_`, `publish_` → `write_external`.
- **Anything that spends money, messages a human, or changes production is `destructive` with
  `always_confirm`, whatever it is called.** `create_order`, `post_message` and
  `create_deployment` are all "create" and all of them get confirmed every time.

`cargo xtask probe-connectors` prints the rules' suggestion. Where it and you disagree, you win,
and the override says why in the README.

## 4. The website row

`web/site/src/data/connectors.ts`. Every folder needs a row and every `available` row needs a
folder — `validate-connectors` fails the build otherwise, which is the only thing standing between
the site and a promise the app cannot keep. `status: 'available'` once the folder exists;
`soon` is for something 17 plans. Fill `capabilities` with real tool calls from the probe, not
invented ones.

## 5. Verify, then commit

```sh
cargo run -p xtask -- validate-connectors
cargo run -p xtask -- probe-connectors          # writes fixtures/tools.json — commit it
cargo test --workspace && pnpm test
```

The fixture is part of the diff. It is what lets CI check the manifest against the real server on
every push without a network, and what makes a vendor's change show up as a failing build rather
than as a user's bad afternoon.

**A batch is not done until its probe run is green and committed.** A connector nobody here can
sign in to still ships, with its README saying which parts were verified mechanically and which
were not — say it plainly rather than implying a sign-in happened.

## What to hand back

One commit per batch. In the message: which connectors, what each one's probe answered, and any
tier override with its reason. If a server needed a sign-in you could not do, end with the
checklist for Olav — which connector, what to click, what he should see.
