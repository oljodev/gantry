# 12 — Skills and memory

Both features add text to the model's context and nothing else. That shared property drives the design: neither can execute anything, both are fully visible to the user, and both are injected in ways that respect the append-only transcript (02 §6, 10 §4).

---

## Part A — Skills

### A1. The constraint and what it buys

A skill is instructions. Importing one from a stranger can, at worst, make the model give worse advice in the tasks the skill matches; it cannot run a script, read a file, or reach the network, because Gantry never executes anything found in a skill folder and never grants a skill any capability. Skills sit at the bottom of the instruction precedence in the core prompt (10 §3): they are playbooks, not authority. The remaining risk, prompt text that steers the model badly, is handled by review-before-install and by the same permission engine that gates every tool call regardless of what a skill says.

### A2. Format

Gantry's `SKILL.md` is a valid **Agent Skills** file (the open specification at agentskills.io), so skills round-trip with Claude Code and other agents. Gantry-specific fields live under `metadata` with a `gantry-` prefix, exactly as the specification recommends for client extensions; unknown clients ignore them.

| Field | Required | Rule |
|-------|----------|------|
| `name` | yes | 1–64 chars, `a-z`, `0-9`, single hyphens, must equal the folder name |
| `description` | yes | 1–1024 chars; what it does *and when to use it*; this text is the primary matching signal |
| `license` | no | short name or reference |
| `compatibility` | no | ≤ 500 chars; rarely needed |
| `metadata.gantry-triggers` | no | comma-separated keywords and phrases that force a strong match ("borrow checker, lifetimes") |
| `metadata.gantry-always` | no | `"true"` = include in every chat's frozen prompt (10 §2 layer 7) |
| `metadata.gantry-version` | no | integer string, bumped by the editor on save |
| `metadata.author` | no | free text |

Body: Markdown, no format restrictions, recommended under 500 lines and ~5,000 tokens; hard cap 32 KB. Optional `references/*.md` text files (Markdown or plain text, 64 KB each, 10 per skill) may accompany `SKILL.md` and are loaded only when the model asks for them (A7). This is an explicit interpretation of the "text only" rule: reference documents are text, and the Agent Skills layout already uses them; `scripts/`, `assets/` and any non-text file are refused on import, listed in the review screen as dropped. If you want the stricter reading, remove `references/` support; nothing else depends on it.

```markdown
---
name: rust-idioms
description: Idiomatic Rust for this codebase: thiserror/anyhow error handling, ownership over cloning, clippy-clean code, tokio conventions. Use when writing or reviewing Rust or Cargo code, or when the user mentions the borrow checker, lifetimes, traits or async Rust.
license: FSL-1.1-ALv2
metadata:
  gantry-triggers: "rust, cargo, clippy, borrow checker, lifetime, trait, tokio, async rust"
  gantry-always: "false"
  gantry-version: "2"
  author: olav
---

# Rust idioms

## Error handling
1. Library crates: `thiserror` enums with `#[from]` where the mapping is obvious…
```

### A3. Storage and indexing

Two sources, one index:

- **Bundled skills** live in the repository at `skills/<name>/SKILL.md`, mirror the `connectors/` folder pattern (a `README.md` in `skills/` states the contract), and are embedded at build time by `gantry-agent`'s `build.rs` the same way connector manifests are. They ship enabled but not pinned; a starter set of three or four (commit messages, code review, writing a plan, the artifact authoring guide) is enough.
- **User skills** live on disk at `<app_data>/skills/<name>/SKILL.md` so a user can edit them with any editor, put the folder in Git, or delete it. Gantry rescans the folder when the Skills page opens, when the app regains focus, and before each turn (a `stat` per folder is cheap); a changed hash re-indexes the file and records an `external_change` version.
- The **index** is the `skills` table (06 §3): id, source (`bundled` | `user` | `imported`), path, name, description, triggers, `always`, enabled, content hash, size, version, timestamps, usage counters. `skill_versions` keeps full-text snapshots on every save, import, proposal or external change, so "Replace" is always reversible. Pinning is per project (`project_skills`) and per chat (`chat_skills`).

### A4. Relevance matching (v1)

Deterministic keyword matching, plus the model pulling skills on demand. No embeddings: a local embedding model means shipping an inference runtime and a model file for a problem that descriptions written for matching already solve at the scale of tens of skills. Revisit if a user's library passes about a hundred skills.

For each user message:

1. Tokenize the message: lowercase, split on non-alphanumerics, drop stopwords, light stemming (`rust-stemmers`). Keep trigger phrases with spaces as bigrams.
2. Score each enabled, unpinned skill: `3 × trigger hits + 2 × name hits + 1 × description hits`. A skill qualifies at score ≥ 3 (one trigger hit, or a name hit plus a description hit).
3. Rank, take the top 3, and skip any skill injected within the last 6 turns of this chat (the model still has it in context; re-injecting only costs tokens).
4. Add `gantry-always` skills and skills pinned to the project or chat, which live in the frozen prompt instead (10 §2) and are never re-injected.
5. An explicit `/skill-name` in the composer forces that skill regardless of score.

Matched skills go into the turn's `<gantry_turn_context>` block as `<skill name="…" source="…">…</skill>` (10 §5). The `context.injected` event lists what was injected so the "Context used" row in the activity feed shows it (05 §2).

**Model-driven loading** covers what keywords miss. The frozen context block lists installed skills as `name — first 160 characters of description` (capped at 40 entries; pinned skills first; beyond the cap the list says "more available via `gantry__list_skills`"). The model calls `gantry__load_skill { name }` when the task fits a listed skill; the body comes back as the tool result. This is the progressive-disclosure pattern of the Agent Skills specification: roughly 100 tokens per skill at rest, the full body only when activated.

### A5. The four flows

**1. Write.** `/skills` → **New skill** opens a two-pane editor: a frontmatter form (title → slug preview, description with a live character count, trigger chips, Always toggle, author) and a CodeMirror Markdown editor pre-filled with a template body (When to use · Steps · Examples · Pitfalls). Validation follows the specification rules and blocks save on violations. A **Test match** box takes a sample message and shows the score. Save writes `<app_data>/skills/<name>/SKILL.md`, snapshots a version, and re-indexes.

**2. Export / share.** **Export** writes the exact `SKILL.md` to a location the user picks, named `<name>.skill.md` so it is recognizable in a downloads folder; a skill with `references/` exports as `<name>.skill.zip` containing the folder. No signing, no manifest wrapper: one readable file is the whole point of text-only skills.

**3. Import / download.** **Import** accepts a `.md` file, a `.zip`, or a folder, and also a pasted HTTPS URL to a raw Markdown file (text only, 256 KB cap, no redirects to other hosts). Every path leads to the same **review screen**: rendered body, parsed frontmatter, size and token estimate, and warnings (dropped `scripts/` or `assets/` entries listed by name, oversize references, frontmatter fixes applied). Nothing is written before the user clicks **Install**. Name collisions follow the rule in flow 4.

A shared community catalog is **deferred**. Reasons: a catalog needs moderation, provenance and a quality signal to be worth trusting, plus an endpoint to run, and the text-only import from a URL already lets people share skills through a gist, a repository or a message. The post-MVP shape is the same as the connector catalog's remote overlay (03 §11): a static, signed JSON index of `{ name, description, url, author, hash }` entries hosted on the Gantry domain, browsed in-app, every entry passing through the review screen, never auto-installed.

**4. AI-generated.** Three triggers, one mechanism:

- the composer command `/skill new <what it should cover>`,
- a **Save as skill** action on a completed turn, which sends a canned request ("Turn the approach you used in this conversation into a reusable skill"),
- the model calling the tool on its own when the user asks in plain language.

The mechanism is the runtime tool `gantry__propose_skill { name, description, triggers, body, references? }`. It creates `Interaction::SkillProposal` and returns immediately; the turn is not blocked. The `SkillProposalCard` in the activity feed shows the rendered skill with every field editable and three actions: **Save**, **Save as…** (new name), **Discard**. The tool result the model eventually sees on its next turn, through a `SystemNote`, says what happened.

Name collisions are never silent. If `name` already exists, the card header says "Replaces v3 of `rust-idioms`" and shows a diff against the installed body; the actions become **Replace (keeps history)**, **Save as `rust-idioms-2`** (suggested suffix), **Discard**. Bundled skills cannot be replaced, only shadowed by a user skill with a different name. The model cannot delete or disable skills; only the user can.

### A6. UI

- `/skills`: list (name, description, source badge, Always/pinned markers, enabled toggle, last used), New, Import, Export, and the editor.
- Composer: typing `/` lists skills to invoke for the current message; the attach menu's **Skills ▸** pins a skill to the chat (10 §4 appends the `SystemNote`).
- Project settings: pinned skills for the project.
- Settings → Skills mirrors the list and links here (11 §2).

### A7. Runtime tools

| Tool | Args → result | Tier |
|------|---------------|------|
| `gantry__list_skills` | `{}` → names and descriptions | app |
| `gantry__load_skill` | `{ name }` → the body | app |
| `gantry__read_skill_file` | `{ name, file }` → a `references/*.md` file | app |
| `gantry__propose_skill` | `{ name, description, triggers, body, references? }` → `{ status: "proposed" }` | app |

All are `app` tier (04 §2): they act only on Gantry's own state, never prompt, and persist nothing without the user's card.

---

## Part B — Memory

### B1. Principles

- Local only. Stored in the same SQLite database as everything else (06).
- **Every entry is visible, editable and deletable on the Memory page.** No memory reaches a prompt without appearing there.
- Every entry has provenance: who created it (user or assistant), from which chat and message.
- Assistant-written memories are confirmed by the user before they exist (B3), because a memory is a standing instruction to future chats and the model may have been reading untrusted content when it decided to remember something.
- Bounded cost: the tokens spent on memory per turn have a ceiling that does not grow with the size of the store (B4).

### B2. Data model (extends 06 §3)

```
memories        id, scope_kind (global | project), scope_id NULL, kind (instruction | preference | fact | note),
                text (≤ 500 chars), always_include, source (user | assistant), origin_chat_id NULL,
                origin_message_id NULL, tags_json, enabled, use_count, last_used_at,
                created_at, updated_at, archived_at
memories_fts    FTS5 over text and tags
```

Kinds: `instruction` (how to behave: "answer in Norwegian"), `preference` (tool and style preferences: "prefer pnpm"), `fact` (about the user, their projects, their environment: "the API lives in services/api"), `note` (working notes for a project). Proposals are `interactions` of kind `memory_proposal`; there is no separate proposals table.

### B3. How a memory gets created

**Decision: the assistant proposes, the user confirms, and the proposal never blocks the turn.**

- The core prompt tells the model to propose a memory only for durable preferences, stable facts about the user or their projects, and explicit "remember this" requests; never task details, never secrets; at most two proposals per turn.
- The model calls `gantry__propose_memory { text, kind, scope, reason, replaces_id? }`. The tool returns `{ status: "proposed" }` immediately and the model continues. Gantry refuses proposals that match the secret-pattern guardrail (API key and token formats) before they reach the user.
- The `MemoryProposalCard` appears in the activity feed with the text editable, the scope and kind selectable, and **Save** / **Discard**. Unresolved cards expire when the chat is archived. Nothing is stored until Save.
- `replaces_id` shows a diff against the older entry and Save archives the old one (restorable from Recently deleted).
- Forgetting is symmetric: `gantry__propose_forget { memory_id, reason }` produces a card; only the user deletes.
- Users create memories directly: the Memory page, the composer command `/remember …`, or selecting text in any message → **Remember this**.

Why confirmation rather than write-now-review-later: the cost is one click on a card that is already in the feed; the benefit is that a memory can never be planted by content the model read (a web page, a tool output, a skill) without the user seeing it, and the store never contains surprises. For users who find the click tedious, Settings → Memory offers **Auto-save assistant memories** per scope, off by default; in that mode the card still appears, already saved, with **Undo**.

### B4. Which memories reach a chat

Two tiers with separate budgets, so cost is bounded whether the store holds 20 entries or 2,000:

| Tier | Members | Where | Budget |
|------|---------|-------|--------|
| **Core set** | `always_include` entries plus every enabled `instruction` and `preference` in scope (global, and the chat's project) | The chat's frozen prompt, `<memory>` block (10 §2), selected at chat creation; ids recorded in `chats.snapshot_memory_ids_json` | ~1,500 tokens; if exceeded, most recently used first and the Memory page shows a warning |
| **Long tail** | `fact` and `note` entries in scope | Per message, in `<gantry_turn_context>` (10 §5): an FTS5 query built from the message's significant terms, ranked by BM25, top 8, skipping entries injected in the last 6 turns | ~800 tokens |

Project-scoped memories are only ever offered to chats in that project. `use_count` and `last_used_at` update on injection, and the `context.injected` event names the entries so the "Context used" row and the Memory page's "used in N chats" are honest. The arithmetic: a store of 500 facts would cost 20,000 tokens per turn if sent whole; with the two tiers a turn spends at most ~2,300 tokens on memory.

### B5. The Memory page

`/memory`: a table with search (FTS), filters (scope, kind, source, enabled), inline editing of text, kind, scope and Always, per-row delete with a **Recently deleted** section (30 days, restore), multi-select delete, export to JSON, import from JSON (reviewed like a skill import), provenance links to the originating chat and message, and a global **Pause memory** switch (nothing injected, nothing proposed) plus per-project switches. The page is reachable from the sidebar footer and from Settings → Memory.

### B6. Changes and the frozen prefix

Memory edits follow the same rule as instruction edits (10 §4): new chats get a new snapshot; existing chats receive a `SystemNote` before the next user message: "Memory updated: added …; changed …; forget: …". Deleted entries are announced with their text so the model stops relying on them. The old text remains inside the frozen `system_snapshot` of chats that were created while it existed, visible in developer mode, until those chats end or are refreshed; the Memory page says so in one sentence next to Delete. This is the price of prefix stability (01 §8, T5), stated rather than hidden.

### B7. Runtime tools

| Tool | Args → result | Tier |
|------|---------------|------|
| `gantry__propose_memory` | `{ text, kind, scope, reason, replaces_id? }` → `{ status: "proposed" }` | app |
| `gantry__propose_forget` | `{ memory_id, reason }` → `{ status: "proposed" }` | app |
| `gantry__search_memory` | `{ query }` → matching entries in scope (for "what do you know about X") | app |
