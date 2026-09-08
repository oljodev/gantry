# 16 — The two surfaces

Gantry has one window with two surfaces: **Chat**, for conversation, documents, research and
connectors, and **Code**, for working inside a folder on this machine. A control in the title
strip moves between them.

This document settles the shape of that split: what each surface is, what the switch does, how a
code session differs from a chat, and what falls out of it elsewhere in the plan. It was decided
with Olav on 2026-09-07 after looking at how Claude Desktop separates its own two modes.

The tools a code session uses are specified in `docs/connectors/code-editor.md`. This document
says where they live and how they are turned on, not what they do.

---

## 1. Why two surfaces rather than one

The single-surface design had a specific problem. Two connectors wanted to own file access, and
the boundary between them could not be drawn cleanly by capability: both need to read a file,
both need to write one, and any rule dividing them by file type collapses the first time somebody
edits a Dockerfile.

The boundary that does work is by **kind of session**. Conversation about documents and
connectors is one thing. Working inside a repository is another. They want different tools,
different defaults, different right-hand panes and different sidebars. Once they are separate
sessions, a chat can have file reading without file editing, and the question of which connector
a chat should hand a repository to stops being asked.

The two do meet in a code session, where both are attached, and there the answer is ownership
rather than separation: one tool exists in exactly one connector, over a workspace layer they
share (C6, §8). What was never workable was two connectors offering the *same* tool and a rule
about file types deciding between them.

The second reason is honesty about defaults. A coding session wants edits applied without
confirming each one; a chat about a PDF does not want a shell. Making that a property of the
surface means the user is never one wrong click from a mismatch.

## 2. Decisions

| # | Decision | Why |
|---|----------|-----|
| C1 | **One window, one sidebar, two surfaces.** Not two apps, not two windows, not a second layout. | The shell, the composer, the activity feed, the right pane, the palette and settings are all shared. The surface changes their contents, never their mechanism. |
| C2 | **The switch is a two-icon segmented control in the title strip**, at the top of the sidebar. `Cmd/Ctrl+Shift+K` toggles it. | It is chrome, not content. Putting it in the strip keeps it out of the reading column and available from every page including settings. |
| C3 | **A code session is a chat with a surface.** The `chats` table gains one column. Everything downstream is unchanged. | One transcript model, one runner, one event stream, one search index. A second entity would double the persistence layer for no gain. |
| C4 | **The surface is chosen at creation and never changes.** | A session whose tools changed halfway has a transcript that cannot be explained. "Continue in the other surface" starts a new session carrying context, which is the same pattern artifacts already use. |
| C5 | **A code session requires a folder before its first message.** | It is what makes it a code session. Without one there is nothing to read, nothing to edit and nothing to run. |
| C6 | **The code surface's tools are connectors, installed and attached when the surface is first opened, and it says so.** `filesystem`, `code-editor` and `shell` keep their manifests, their cards and their detail pages; the surface turns them on rather than reimplementing them. *(Revised 2026-09-08 with Olav; the original decision made them built-in runtime tools with no catalog entry.)* | One implementation of file access, not two. A user who wants to see what the surface can do reads the same card as for any other connector, and can remove any of the three. What 03 §11 promises — nothing installed without an explicit user action — holds, because opening the Code surface **is** that action; §8 says so in the empty state, once, naming all three. |
| C7 | **Each surface has its own defaults**, including permission mode. | Auto-edit is right for coding and wrong for a chat about a spreadsheet. |
| C8 | **Switching never converts the current session.** It goes to the other surface's last place. | The switch is navigation. Anything that silently changed the tools of a running conversation would be a trap. |
| C9 | **Connectors still attach in code sessions.** | GitHub, Linear and Sentry are more useful there than anywhere. What is not offered there is the filesystem connector, whose job the built-in tools already do. |
| C10 | **One wordmark.** The app is called Gantry on both surfaces; the active icon carries the state. | Gantry Code is not a second product and pretending otherwise sets an expectation the roadmap does not meet. |

## 3. The two surfaces, side by side

| | **Chat** | **Code** |
|---|---|---|
| For | Conversation, documents, research, connectors, artifacts | Working inside a folder on this machine |
| Needs a folder | No | Yes, before the first message |
| File tools | The `filesystem` connector, if installed and attached | `filesystem` and `code-editor`, installed and attached with the surface |
| Shell | Not offered | The `shell` connector, installed and attached with the surface |
| Artifacts | Yes, central | Yes, occasional |
| Connectors | Any | Any |
| Default mode | Manual | Auto-edit |
| Right pane | Artifacts, and detail tabs | Changes, and detail tabs |
| Sidebar list | Chats | Sessions, with their folder |
| Composer | Attach menu, web search toggle | Folder chip, no attach-folder item |

Everything not in this table is identical, deliberately.

## 4. The switch

A segmented control of two icons in the title strip, at the top of the sidebar and above the
first navigation row, matching where Claude Desktop puts its own. A speech-bubble glyph for Chat
and an angle-bracket glyph for Code, both Phosphor regular at 16, the active one on
`bg-selected` with its icon in `fg`, the inactive in `fg-2`. A tooltip names each. Twenty-eight
pixels tall, so it does not compete with the window controls opposite.

Pressing it:

1. Saves the current surface's route.
2. Restores the other surface's last route, or its empty state on first use.
3. Does nothing to any running turn. A turn started on one surface keeps running and keeps
   streaming while the user is on the other; the sidebar's running indicator shows it there.

`Cmd/Ctrl+Shift+K` does the same from the keyboard. The command palette carries "Switch to Code"
and "Switch to Chat" so it is discoverable without knowing the shortcut.

The surface is part of the route, so back and forward move between surfaces naturally and a deep
link from a notification lands on the right one.

## 5. Layout of the code surface

Same skeleton as 15 §7, three regions, different contents.

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●   Gantry   [ 💬 │ </> ]      ⋯ drag ⋯          [pane] [–][□][×]│  38 px
├────────────┬───────────────────────────────────────────┬─────────────────────┤
│ + New      │                                           │ Changes · Detail    │
│ ⌕ Search   │      ┌─────────────── 720 px ──────────┐   │ ┌─────────────────┐ │
│            │      │ Assistant text …                │   │ │ 4 files changed │ │
│ Projects   │      │ ▸ 12 tool calls · 4 files · 3 cmd│  │ ├─────────────────┤ │
│            │      │   ✎ src/auth.rs        +12 −3    │   │ │ src/auth.rs +12 │ │
│ Sessions   │      │   $ cargo test -p api  ✓ 1.4 s   │   │ │ src/lib.rs   +4 │ │
│  · gantry  │      │                                 │   │ │ …               │ │
│  · zovy    │      │                     ┌──────────┐│   │ │ [unified diff]  │ │
│  · gantry  │      │                     │ user msg ││   │ │                 │ │
│            │      │                     └──────────┘│   │ │                 │ │
│            │      ├─ composer card ─────────────────┤   │ │                 │ │
│            │      │ Describe a task…                │   │ │                 │ │
│ ⚙ Settings │      │ [📁 gantry] [Auto-edit ▾] [⌄] ↑ │   │ └─────────────────┘ │
└────────────┴───────────────────────────────────────────┴─────────────────────┘
```

### Sidebar

`New` starts a session and opens the folder picker. `Search` is the same palette. `Projects` is
shared with the chat surface, filtered to those that have a workspace folder.

`Sessions` is a flat list, most recent first, no day groups, matching the existing rule for
chats. Each row is the session title on the first line and its folder name in `micro` `fg-3` on
the second, so the same repository appearing three times is legible at a glance. A filter control
at the section header narrows to one folder.

Everything else about the sidebar, including width, resizing, `Cmd/Ctrl+B`, the running dot and
the decision badge, is unchanged from 15 §7.

### Content column

Identical to the chat surface: the same measure, the same turn structure, the same inline
activity behind a summary line, the same interaction cards. A code session produces more tool
calls and more file edits, which is exactly what the collapsed activity summary was designed for.

### Right pane

Two kinds of tab, as today, but the permanent one changes.

**Changes** replaces Artifacts as the pane's home tab. It lists every file the session has
touched, with its net line counts, newest first, and shows the selected file's unified diff
underneath. Per-file **Revert** and a session-level **Revert all** both go through the existing
journal. This is the single most useful thing the pane can show during a coding session: a
running answer to "what has it actually done to my repository".

Detail tabs are unchanged: diff, command, tool call, guard, opened temporarily and closed with
`Esc`.

Artifacts still open here when a code session produces one; they are simply not the default.

**Deliberately absent: a file tree.** The agent finds files; a tree is an editor feature, and
building one invites the expectation that Gantry is an editor. If a user wants to browse the
repository they have one open already.

### Composer

The attach menu loses "Add folder to workspace", because the folder is chosen at creation and
shown as a chip. The chip opens a menu to add a second folder or switch the primary one. The
web-search toggle stays. The placeholder reads `Describe a task or ask about gantry…`.

Otherwise identical: mode chip, model picker, thinking selector, Send and Stop, and the same
keyboard behaviour including `Shift+Tab` to cycle modes.

## 6. Sessions

Per C3, a code session is a row in `chats` with `surface = 'code'`. It has messages, turns,
events, tool calls, file edits, command runs, grants, attachments and a title, exactly as a chat
does, produced by exactly the same code.

What the surface decides:

- **Which tools are offered.** Built-in code tools plus whatever connectors are attached.
- **The default permission mode**, from settings.
- **Which shell the frontend renders**, meaning the sidebar contents, the pane's home tab and the
  composer's controls.
- **Which list it appears in.** A code session never appears in the chat list, and the reverse.
  Search spans both and labels each result with its surface.

Titles are generated the same way, from the first exchange, using the same small model.

**Continue in the other surface** appears in the session menu. It creates a new session on the
other surface, carrying a short summary and, from code to chat, the folder as an attached root if
the filesystem connector is installed. It never moves the original.

## 7. The workspace folder

A code session is created by picking a folder, and the picker is the empty state rather than a
dialog over an empty conversation. Recent folders are offered first, so the second session on a
repository is one click.

The folder becomes the session's root, in the sense `gantry-workspace` means it
(`docs/connectors/filesystem.md` §4): canonicalised once, held open, and the boundary for every
path the model names. Additional folders can be added from the chip, which is how a session
spanning a repository and its sibling library works.

If a project has a workspace folder, starting a code session from that project skips the picker
and inherits the project's instructions, defaults and pinned skills.

Nothing about this changes the containment rules. The code surface makes the folder easier to
choose; it does not make the boundary weaker.

## 8. The tools of a code session

Per C6 the surface does not own tools; it turns three connectors on. Opening the Code surface for
the first time installs and attaches `filesystem`, `code-editor` and `shell`, and every later code
session attaches the same three. They are ordinary instances: one row each in
`connector_instances`, a card each in Discover, a detail page each, removable like anything else.
Removing one leaves the surface working without it, which is the honest behaviour — a session
whose shell was removed can still read and edit.

Their ownership is settled rather than overlapping, which is what the two-surface split was for.
`gantry-workspace` owns the roots, the canonicalisation, the sensitive-path rules, the edit
journal and the freshness rule, and all three connectors sit on it. So:

- **`filesystem`** owns reading, writing whole files, listing, globbing, searching and moving.
- **`code-editor`** owns the surgical edits and only those: replace, insert, patch, undo. It needs
  no `read_file` of its own, because the rule that a file must have been read before it is changed
  is enforced by the shared journal underneath both connectors, not by which connector did the
  reading. This answers `docs/connectors/filesystem.md` §19.
- **`shell`** owns running commands.

No tool exists twice, so nothing in a code session's tool array is ambiguous.

The shell arriving this way is the part with real weight: the most dangerous capability in the
product becomes available on opening a folder. Two things make that defensible, and both must
actually hold.

**Permission modes still gate every call**, unchanged. Attached means present in the tool list,
not allowed. The default of Auto-edit applies edits without asking and still asks for anything
that runs a command or touches something outside the folder.

**The first code session says so.** The empty state, before the first message, states in one short
paragraph what was just turned on — the three connectors by name, that they read and change files
in the chosen folder and run commands as you, and where to remove them. Once, with a "don't show
again", not a modal wall. A `ConnectorsChanged` event fires like any other install, so the
Connectors list is never out of step with what happened.

## 9. Permissions and defaults

| Setting | Chat default | Code default |
|---------|-------------|--------------|
| Permission mode | Auto-edit | Auto-edit |
| Guard, when in Auto | Judge | Judge |
| Suggest connectors | On | On |

The chat column said Manual when this was written, on the reasoning that Auto-edit is wrong for
a conversation about a spreadsheet. As built (2026-09-08) both start at Auto-edit, because that
reasoning does not survive contact with what Auto-edit actually allows: it applies `write` calls
inside the session's own folders and asks for everything else, and a chat has no folders. Manual
there would buy no safety and would prompt for every connector read — every GitHub search, every
documentation lookup. The two settings still exist separately, so a user who wants Manual in one
place and not the other has it, and the moment a chat can attach a folder the question is worth
reopening.

Both are per-surface settings in General, both overridable per project and per session. The mode
chip works identically on both surfaces, so a user who wants Manual in code has it one click
away, and `Shift+Tab` still cycles.

The matrix in 04 §3 is unchanged. The surface changes the starting mode, not what a mode means.

## 10. Projects across surfaces

Projects are shared. A project holds instructions, knowledge files, defaults and optionally a
workspace folder, and it can contain sessions of both kinds: a code session that implemented
something and a chat that discussed it, under one heading.

The project page lists both, labelled, and its Artifacts tab is unchanged. Only projects with a
workspace folder can start a code session directly; the rest ask for a folder as usual.

This is why the sidebar shows Projects on both surfaces rather than duplicating them.

## 11. Empty states

**Code, no sessions yet.** A short line, a prominent folder picker, and the capability paragraph
from §8. Recent folders listed underneath once there are any.

**Code, folder chosen, no messages.** The composer, focused, with the folder chip filled in and
two or three suggested openings that suit a repository: understand the structure, find where
something happens, run the tests.

**Chat** is unchanged from the existing welcome (15 A20).

## 12. Routing and state

| Route | Surface | Renders |
|-------|---------|---------|
| `/chat` | Chat | The chat empty state |
| `/chat/$chatId` | Chat | A chat session |
| `/code` | Code | The folder picker empty state |
| `/code/$sessionId` | Code | A code session |
| `/project/$projectId` | Either | Shared; the surface is whichever the user came from |
| `/connectors`, `/settings/$section`, `/dev/gallery` | Either | Shared, and the switch stays visible |

The last route per surface lives in the persisted UI store next to the sidebar width and theme,
so the app reopens where the user left each side.

The run store is unchanged. It is keyed by session, and a session's surface is one more field on
its row, so a turn running on the other surface streams into the same store and lights the same
indicators.

## 13. Data model

One migration, and it is small.

- `chats` gains `surface TEXT NOT NULL DEFAULT 'chat'`, with a check constraint of `chat` or
  `code`, and an index on `(surface, last_message_at)` because the two lists are the hot query.
- Existing rows become `chat`, which is correct.
- Code sessions must have at least one `chat_roots` row before their first turn. Enforced in the
  agent, not by the schema, so the row can be added in the same transaction as the first message.

Nothing else changes. `messages`, `turns`, `events`, `tool_calls`, `file_edits`, `command_runs`,
`interactions`, `chat_grants` and `chat_connectors` are all surface-agnostic and stay that way.

## 14. What this changes elsewhere

| Document | Change |
|----------|--------|
| `01-architecture-overview.md` §5 | The sidebar section describes one list; there are now two, and a surface toggle above them. The module map gains a `features/code` folder for the surface-specific shell |
| `01-architecture-overview.md` §8 | Tension T12 is about connectors never being auto-installed. It needs a sibling row, or an amendment, covering the code surface's built-in tools: the promise is about the catalog, not about app-owned capability |
| `03-connector-system.md` §11 | "Nothing is installed by default" gains its one named exception: opening the Code surface installs `filesystem`, `code-editor` and `shell` together, discloses it in the empty state, and leaves all three removable |
| `03-connector-system.md` §5 | Unchanged in kind: all three stay catalog connectors. The subsections gain the note that the Code surface attaches them |
| `06-data-model.md` §3 | The `chats` row gains `surface` |
| `07-repository-structure.md` | The three connector folders stay; the frontend gains `features/code` |
| `09-roadmap.md` | §16 below |
| `11-settings-and-theming.md` | General gains per-surface defaults (`chat.default_mode` / `chat.code_default_mode` and their guards) |
| `15-app-design.md` §7 | The layout section gains the toggle and the code surface's sidebar and pane contents. A19 on diffs now also covers the Changes tab |
| `docs/connectors/filesystem.md` §19 | Answered: `read_file` and `write_file` stay with `filesystem`. The freshness rule that made the editor want its own read is enforced by the shared journal in `gantry-workspace`, under both connectors |
| `docs/connectors/code-editor.md` | Was an empty file. Written: the four editing tools, the journal they write, and what they refuse |

## 15. Cost, honestly

The shell of this is small: a toggle, a column, a route pair, a persisted last-route per surface,
and a filtered list. Perhaps a week including the empty states.

What is not small is the Changes tab, which needs a session-scoped view over `file_edits` with
per-file and whole-session revert, and the settings and project work that follows from
per-surface defaults. Call it a week and a half in total, and note that most of it is composition
of components M0b already built rather than new design.

Removing two connectors from the catalog gives some of that back: two manifests, two icons, two
READMEs, two install paths and two detail pages that no longer need to exist or be tested.

## 16. Where it lands

It must land **no later than the milestone that builds code editing**, because after this
document that is where code editing lives. The current roadmap puts filesystem and the code
editor in M6.

Recommendation: make the surface the first half of M6 and let that milestone grow by about a
week, rather than inventing a milestone for it. The reasons are that the surface is not usable
without the tools and the tools now have nowhere to live without the surface, so shipping them
apart produces two half-milestones that neither demonstrate anything.

M6's "done when" becomes: a real repository can be opened in the code surface, modified by the
model with every change visible as a diff, and reverted byte for byte, while a chat about
something else continues on the other surface.

## 17. Open questions

1. **Does the shell follow the code editor out of the catalog?** *Answered 2026-09-08: neither
   does.* Both stay catalog connectors, and the Code surface installs and attaches them with
   `filesystem` on first use, disclosed once (C6, §8). The reasoning that made "built in" look
   right — that a surface which cannot run a test is half a tool — is satisfied by attaching
   rather than by absorbing, and it costs no duplicated file access.
2. **Should a chat be able to attach a folder at all** once the code surface exists? *Answered
   2026-09-08: yes.* The `filesystem` connector stays attachable in a chat for document work,
   which is its stated purpose. A chat never gets `code-editor` or `shell` unless the user
   installs and attaches them deliberately.
3. **Scheduled and background sessions.** Claude Desktop's sidebar has them; Gantry's roadmap has
   them post-MVP. Worth confirming they belong to the code surface when they arrive, since that
   is where an unattended task usually is.
4. **A third surface later.** Nothing here forbids it, and the column is a string rather than a
   boolean for that reason.
