# 17 — Growing the connector catalog

**Written 2026-09-08.** Document 03 settles what a connector *is* and how one is installed. This
document settles the other half: which third-party servers Gantry ships manifests for, in what
order, and — the part Olav asked for — how a batch of five connectors can be shipped without him
signing in to five services to find out whether they work.

Everything in §2 was probed against the live servers on **2026-09-08** from this machine: one
`initialize` call per endpoint, plus the npm and PyPI registries and the vendors' own
repositories. Nothing below is from memory. §5 turns that probe into a repeatable command so the
table stays true.

## 1. Why this can be safe without you testing each one

A manifest-only connector adds **no code**. Its whole surface is one JSON file, an icon and a
README, so a mistake can only be in one of four places:

| Where a mistake can hide | Who catches it |
|---|---|
| The endpoint or the command (wrong URL, wrong package, server retired) | The live probe (§5) — mechanical, no account needed |
| The auth shape (says OAuth, actually wants a header; claims dynamic registration it does not do) | The live probe: `WWW-Authenticate`, the protected-resource document and a registration attempt say which of the four shapes it is |
| The risk tiers (a delete tool arriving as `read`) | The recorded tool list (§5) plus the tier rules in §6; reviewed once per connector, diffed forever after |
| The prose (description, README, setup steps) | Review, like any other copy |

The blast radius of getting one wrong is also small, and this is the real argument: an install
that fails, fails **visibly and alone**. `ConnectorRegistry` isolates every instance (03 §6),
nothing is installed without an explicit action (D11), and a manifest that points at a dead URL
produces a failed install dialog with the server's own error — not a broken app, not a broken
chat, not a lost credential.

So the testing rule is **per auth shape, not per connector**:

| Shape | Proven by | Status |
|---|---|---|
| Remote, no auth | `cloudflare-docs` | proven, shipped |
| Remote, OAuth with dynamic client registration | `cloudflare-bindings` | proven, shipped |
| Remote, OAuth with a client id you supply | `github` | proven, shipped |
| Remote, API key in a header | first of B5 | one sign-in from Olav |
| Local process on Node (`npx`) | first of B6 | one install from Olav |
| Local process on Python (`uvx`) | first of B7 | one install from Olav |
| Remote, host you supply (self-hosted or tenant) | first of B10 | one sign-in from Olav |

Four manual tests remain for the whole catalogue, each on a service Olav already uses. Every
connector after the first of its shape is data on a proven path, and the probe is what says the
data is right.

Where a connector's tool list cannot be recorded because nobody here has an account, its README
says so in the "Verified" line, in the form `github/README.md` already uses ("checked against the
live server on 2026-09-07"). Shipping an unexercised manifest is acceptable; pretending it was
exercised is not.

## 2. What actually exists, probed 2026-09-08

Verdicts: **official** = published by the vendor, on their domain or in their organization.
**vendor-hosted** means Gantry needs no runtime; **local** means a child process on Node or
Python (D3).

### A. Official remote servers — one URL, nothing to install

`init` is the HTTP status of an unauthenticated `initialize`; `401 + PRM` means it answered with
`WWW-Authenticate` and a protected-resource document, which is the shape Gantry's OAuth path
expects (03 §7).

| Service | Endpoint | init | Auth |
|---|---|---|---|
| Airtable | `https://mcp.airtable.com/mcp` | 401 | bearer / OAuth |
| Apify | `https://mcp.apify.com` (root, not `/mcp`) | 401 | OAuth |
| Asana | `https://mcp.asana.com/sse` | 401 + PRM | OAuth |
| Atlassian (Jira, Confluence) | `https://mcp.atlassian.com/v1/sse` | 401 | OAuth |
| Axiom | `https://mcp.axiom.co/mcp` | 401 + PRM | OAuth |
| Browserbase (Stagehand) | `https://mcp.browserbase.com/mcp` | 200 | key in header |
| Cal.com | `https://mcp.cal.com/mcp` | 401 + PRM | OAuth |
| Canva | `https://mcp.canva.com/mcp` | 401 + PRM | OAuth |
| Cardboard | `https://cardboard.inc/mcp/` (URL from their docs; not guessable) | — | OAuth |
| Cloudflare — Workers/bindings, docs, observability, radar, browser, and more | `https://<product>.mcp.cloudflare.com/mcp` | 401 + PRM (docs: open) | OAuth (DCR) |
| Cloudinary | `https://asset-management.mcp.cloudinary.com/mcp` | 401 + PRM | OAuth |
| Datadog | `https://mcp.datadoghq.com/api/unstable/mcp-server/mcp` | 401 | OAuth |
| Exa | `https://mcp.exa.ai/mcp` | 200 | key in query |
| Figma | `https://mcp.figma.com/mcp` | 401 + PRM | OAuth |
| Firecrawl | `https://mcp.firecrawl.dev/mcp` | 200 | key in URL or header |
| GitHub | `https://api.githubcopilot.com/mcp/` | 401 | OAuth (client id you supply) — **shipped** |
| GitLab | `https://gitlab.com/api/v4/mcp` | 401 + PRM | OAuth |
| Google Cloud & Workspace (BigQuery, Firestore, Cloud Run, Compute, Developer Knowledge, Drive, …) | `https://<service>.googleapis.com/mcp` | 200 | Google OAuth, client from your own project |
| Grafana Cloud | `https://mcp.grafana.com/mcp` | 401 + PRM | OAuth |
| Granola | `https://mcp.granola.ai/mcp` | 401 | OAuth |
| Hugging Face | `https://huggingface.co/mcp` | 200 | optional token |
| HubSpot | `https://mcp.hubspot.com/anthropic` | 401 + PRM | OAuth |
| incident.io | `https://mcp.incident.io/mcp` | 401 + PRM | OAuth |
| Intercom | `https://mcp.intercom.com/mcp` | 401 | OAuth |
| Jotform | `https://mcp.jotform.com/mcp` | 401 | OAuth |
| Kagi | `https://mcp.kagi.com/mcp` | 401 | bearer |
| LangSmith | `https://api.smith.langchain.com/mcp` | 401 + PRM | OAuth |
| Linear | `https://mcp.linear.app/mcp` | 401 + PRM | OAuth |
| Lovable | `https://mcp.lovable.dev/mcp` | 401 + PRM | OAuth |
| MailerLite | `https://mcp.mailerlite.com/mcp` | 401 + PRM | OAuth |
| Microsoft Fabric | `https://api.fabric.microsoft.com/v1/mcp` | 401 + PRM | Entra OAuth |
| Microsoft Learn | `https://learn.microsoft.com/api/mcp` | 200 | none |
| Neon | `https://mcp.neon.tech/mcp` | 401 | OAuth |
| Netlify | `https://mcp.netlify.com/mcp` | 401 + PRM | OAuth |
| Notion | `https://mcp.notion.com/mcp` | 401 + PRM | OAuth |
| PayPal | `https://mcp.paypal.com/mcp` | 401 + PRM | OAuth |
| Perplexity | `https://api.perplexity.ai/mcp` | 401 + PRM | OAuth |
| Plaid | `https://api.dashboard.plaid.com/mcp/sse` | 401 | OAuth |
| PostHog | `https://mcp.posthog.com/mcp` | 401 + PRM | OAuth |
| Postman | `https://mcp.postman.com/mcp` | 401 + PRM | OAuth |
| Railway | `https://mcp.railway.com/mcp` | 401 + PRM | OAuth |
| Ramp | `https://mcp.ramp.com/mcp` | 401 + PRM | OAuth |
| Replicate | `https://mcp.replicate.com/mcp` | 401 | OAuth |
| Resend | `https://mcp.resend.com/mcp` | 401 + PRM | OAuth |
| Sentry | `https://mcp.sentry.dev/mcp` | 401 + PRM | OAuth |
| Slack | `https://mcp.slack.com/mcp` | 401 + PRM | OAuth, admin-approved (GA Feb 2026) |
| Socket | `https://mcp.socket.dev/` | 200 | none for reads |
| Square | `https://mcp.squareup.com/sse` | 401 + PRM | OAuth |
| Stripe | `https://mcp.stripe.com` | 401 + PRM | OAuth or key |
| Supabase | `https://mcp.supabase.com/mcp` | 401 | OAuth |
| Tally | `https://mcp.tally.so/mcp` | 401 + PRM (`api.tally.so`) | OAuth |
| Tavily | `https://mcp.tavily.com/mcp` | 401 + PRM | OAuth or key in query |
| Tinybird | `https://mcp.tinybird.co` | 200 | token in header |
| Vercel | `https://mcp.vercel.com` | 401 | OAuth |
| Windsor.ai | `https://mcp.windsor.ai/` | 401 | bearer |
| Xero | `https://mcp.xero.com/mcp` | 401 | OAuth |
| Zapier | `https://mcp.zapier.com/api/mcp/mcp` | 401 | per-user URL and key from their dashboard |

### B. Official local servers — a child process on a runtime you have

| Service | Package | Runtime |
|---|---|---|
| 21st.dev (Magic) | `@21st-dev/magic` | Node |
| Anthropic Filesystem (reference server) | `@modelcontextprotocol/server-filesystem` | Node — *Gantry's own `filesystem` connector already covers this; not planned* |
| Azure | `@azure/mcp` | Node |
| Blender | `blender-mcp` (PyPI) — community, not Blender's | Python |
| Browserbase | `@browserbasehq/mcp-server-browserbase` | Node |
| BrowserStack | `@browserstack/mcp-server` | Node |
| Docker | `docker mcp gateway` (Docker Desktop) and `docker/hub-mcp` | Docker |
| ElevenLabs | `elevenlabs-mcp` (PyPI) | Python |
| Grafana (self-hosted) | `grafana/mcp-grafana` (Go binary) | binary |
| Hostinger | `hostinger-api-mcp` | Node |
| JetBrains | `@jetbrains/mcp-proxy` | Node + the IDE plugin |
| Kagi | `kagimcp` (PyPI) | Python |
| Make | `@makehq/mcp-server` | Node |
| Microsoft 365 (Softeria, third-party) | `@softeria/ms-365-mcp-server` | Node |
| Microsoft Clarity | `@microsoft/clarity-mcp-server` | Node |
| MiniMax | `minimax-mcp` (PyPI) / `minimax-mcp-js` | Python or Node |
| Perplexity | `@perplexity-ai/mcp-server` | Node |
| Playwright | `@playwright/mcp` | Node |
| PostHog | `posthog/mcp` (also remote) | Node |
| Qdrant | `mcp-server-qdrant` (PyPI) | Python |
| Ramp | `ramp-public/ramp-mcp` (also remote) | Python |
| Salesforce | `@salesforce/mcp` | Node |
| Shopify Dev | `@shopify/dev-mcp` | Node |
| Twilio | `@twilio-alpha/mcp` | Node |
| Unleash | `Unleash/unleash-mcp` | Node |
| Xero | `@xeroapi/xero-mcp-server` | Node |
| 1Password (Environments, beta) | ships with 1Password's own tooling, not a registry package | 1Password app |
| Rive | the Rive editor's own MCP bridge | Rive app |
| LottieFiles (Lottie Creator) | LottieFiles' own MCP | hosted by them |

### C. Official, but the URL is yours

These need a host or tenant before they mean anything, so they use `user_config` and a
`${user_config.HOST}` in the runtime URL rather than a fixed endpoint.

| Service | Shape |
|---|---|
| Databricks | `https://<workspace-host>/api/2.0/mcp/…` |
| Metabase | `https://<your-metabase>/api/mcp` — official since Metabase 60 |
| n8n | the MCP Server Trigger node on your own n8n |
| Grafana (self-hosted) | your Grafana URL plus a service-account token |
| Unleash | your Unleash URL plus a token |
| Tencent Cloud | `mcp.tencentcloudapi.com` answers, but with a signed, region-scoped request — their marketplace, not one server |

### D. No official server — what exists instead

| Service | What there is |
|---|---|
| Cloudflare Code Mode | Not a server. Code Mode is a Workers *pattern* — MCP tools handed to a model as a sandboxed TypeScript API. Nothing to connect to. This is the one on the list with nothing behind it. |
| "Anthropic Command Execution" | No such published server. The reference set is filesystem, fetch, git, memory, time, sequential-thinking. Command execution is Gantry's own `shell`. |
| Polymarket | Several community servers, no vendor one |
| Twitch | Community only |
| Tailscale | Community only |
| Ollama | Not applicable — Ollama serves models, it is not an MCP server. It belongs in 02, as a provider. |
| n8n (`n8n-mcp`) | The popular package is community; n8n's own answer is the trigger node in §C |
| Airtable (`airtable-mcp-server`) | Community, and now redundant: Airtable hosts its own (§A) |
| Microsoft 365 Agents Toolkit | A developer toolkit, not a server. Microsoft's connectable surfaces are Learn, Fabric and Clarity (§A, §B) |

## 3. The batches

Five at a time, grouped **by auth shape rather than by popularity**, because the shape is where
the work is. A batch that reuses a proven shape is an afternoon; a batch that introduces one is a
day and one sign-in from Olav.

| Batch | Shape | Connectors | New mechanism |
|---|---|---|---|
| **B0** | — | none | The probe harness, tier rules and the folder template (§5, §6) |
| **B1** | Remote, no auth | Microsoft Learn, Hugging Face, Socket, Context7, DeepWiki | none — proven by `cloudflare-docs` |
| **B2** | Remote, OAuth (DCR) | Linear, Notion, Sentry, Netlify, Vercel | none — proven by `cloudflare-bindings` |
| **B3** | Remote, OAuth | Slack, Atlassian, Asana, Figma, Canva | Slack's admin-approval step |
| **B4** | Remote, OAuth | GitLab, Supabase, Neon, PostHog, Railway | none |
| **B5** | Remote, key in a header or query | Stripe, Tavily, Firecrawl, Tinybird, Exa | **first key-in-header connector — one setup from Olav** |
| **B6** | Local, Node | Playwright, Shopify Dev, Azure, Salesforce, BrowserStack | **the runtime check and command preview (03 §11) — one install from Olav** |
| **B7** | Local, Python | ElevenLabs, Qdrant, Kagi, MiniMax, Ramp | **uv detection — one install from Olav** |
| **B8** | Remote, OAuth client you supply | Google Drive, Calendar, Gmail, BigQuery, Cloud Run | Google's console steps; the GitHub path already proves the mechanism |
| **B9** | Remote, OAuth, money | PayPal, Square, Xero, HubSpot, Ramp | tier review is the work, not the auth |
| **B10** | Remote, host you supply | Metabase, Grafana, Databricks, n8n, Unleash | **`user_config` in a runtime URL — one setup from Olav** |
| **B11+** | proven shapes only | The long tail: Granola, Cal.com, Resend, MailerLite, Tally, Jotform, Lovable, incident.io, Datadog, Postman, Axiom, LangSmith, Cloudinary, Browserbase, Clarity, Fabric, Perplexity, Apify, Make, Hostinger, 21st.dev, Twilio, Windsor, Cardboard, Replicate, Intercom, Plaid, Docker, JetBrains, 1Password, Rive, Lottie | none |

B0–B2 land with M9's remaining work; B3–B5 with M10; B6–B7 need the runtime check, so they wait
for the rest of M9 (09, "the runtime check, elicitation, `user_config` forms"); B8–B11 are
release-cadence work after that, five per release, and each release note lists what was added.

Two rules keep the tail from rotting:

- **A batch is not done until its probe run is green and committed.** The recorded tool lists are
  part of the batch's diff.
- **A connector nobody can sign in to still ships**, with its README saying which parts were
  verified mechanically and which were not.

## 4. What one connector costs

For a remote server on a proven shape: the folder from the template, the manifest (endpoint and
auth from the probe output), an icon, a README, tier overrides for anything destructive, and the
probe run. Twenty to forty minutes, most of it the README. For a local server, add the runtime
requirement and one real install.

## 5. The harness — `cargo xtask probe-connectors`

B0's deliverable, and the thing that makes the rest cheap. It reads every manifest in
`desktop/connectors/`, and for each:

**`mcp-remote`** — POSTs `initialize` with no credential and records the status; on `401`, follows
`WWW-Authenticate` to `/.well-known/oauth-protected-resource`, then to the authorization server
metadata, and records whether dynamic registration and a client-id metadata document are offered;
on `200`, calls `tools/list` and writes the names, descriptions and input schemas to
`desktop/connectors/<id>/fixtures/tools.json`. It then **asserts the manifest agrees**: `auth.type`
matches what the server asks for, `auth.registration` lists only modes the server actually
supports, and every key in `tool_overrides` exists in the recorded list.

**`mcp-stdio`** — resolves the package in the npm or PyPI registry, checks the version the manifest
pins still exists, and (with `--spawn`) starts it in a scratch directory with no credentials, to
record `tools/list` the same way.

Three ways it runs:

- `cargo xtask probe-connectors` on demand, while writing a batch;
- `--offline` in CI on every push: no network, it only checks each manifest against its recorded
  fixture, so the assertions above run on every commit;
- a weekly scheduled job with the network, which opens an issue when a server's shape drifts — a
  retired endpoint, a new destructive tool with no override, an auth mode that changed. Vendors
  move; this is how we find out before a user does.

`validate-connectors` keeps its job (schema, icon, README, id equals folder) and gains: the id is
unique across the catalogue, `catalog.sort_weight` does not collide inside a category, and every
`suggest_for` term is lowercase.

## 6. Tier rules

Runtime-discovered tools get `risk.default_tool_tier` unless a rule or an override says otherwise.
The rules, applied to the recorded tool list at review time and printed by the probe as a
suggestion, never silently:

| Name matches | Tier | Also |
|---|---|---|
| `list_`, `get_`, `search_`, `read_`, `describe_`, `fetch_` | `read` | |
| `create_`, `update_`, `add_`, `set_`, `post_`, `send_`, `merge_`, `deploy_`, `publish_` | `write_external` | |
| `delete_`, `remove_`, `drop_`, `revoke_`, `cancel_`, `refund_`, `transfer_`, `pay_` | `destructive` | `always_confirm: true` |
| anything that spends money, messages a human, or changes production | `destructive` | `always_confirm: true` |

The last row is a judgement, not a regex: PayPal's `create_order`, Slack's `post_message` and
Vercel's `create_deployment` are all "create" and all get confirmed every time. Where the probe's
suggestion and the reviewer disagree, the reviewer wins and the override records why.

## 7. The website

`web/site/src/data/connectors.ts` is the public catalogue and it must not describe connectors that
do not exist. Every entry carries `status`: `available` for a connector with a folder in
`desktop/connectors/`, `soon` for one this document plans. The directory shows both, marks the
planned ones, and leads with how many are ready today; each batch flips its five to `available` in
the same commit that ships them, and adds new entries for anything not yet listed. The home page's
logo cloud keeps its "more coming" tile.

The site's list and the shipped catalogue therefore disagree by design, in one direction only:
everything `available` on the site exists in the app, and the app never has a connector the site
does not list.

## 8. Names, marks and law

Third-party names and logos belong to their owners; a manifest describes a server it does not ship
(`desktop/connectors/README.md`). The site draws marks from Simple Icons in the vendor's colour and
falls back to initials; the app uses monograms until the licensing question is cleared (15, M0b).
Neither claims endorsement, and the note under the directory says so.
