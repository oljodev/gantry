# 15 — Application design system

**Status:** all three layers, decided and built in session 5 (2026-09-06/07); M0b landed the tokens, primitives, composites, gallery and mock screens. This document is the design table: every visual and interaction decision for the desktop app, with its reason. Layer 2 turns it into tokens in code (`desktop/frontend/src/styles/tokens.css` and the Tailwind theme); layer 3 is the component library (reshaped shadcn primitives plus Gantry composites, each shown in a dev-only gallery route). Nothing in a feature may contradict this document; when it must, the document changes in the same commit.

The website (14) is designed on its own terms and is not a reference for the app. The two share the wordmark and the portal-frame mark and nothing else.

## 1. Design read

**Linear chrome, Claude conversation.** The chrome (sidebar, title bar, settings, connector pages, activity rows, dialogs, menus) is quiet, dense and keyboard-first at Linear's 13 px scale. The chat column alone gets more air: a 15 px reading size, a bounded measure and generous message spacing, because reading long answers is a different job from scanning a list.

The references, and what is taken from each:

| Reference | Taken | Left |
|-----------|-------|------|
| Linear (desktop) | Density, one type size for almost everything, hairlines over shadows, surface steps, settings layout, command palette, motion timing | Its blue accent, its issue-tracker information architecture |
| Claude Desktop | Message layout (tinted user block, plain assistant text), the floating composer with controls inside, spacious reading column, onboarding tone | Serif display type, warm cream palette |
| Cursor | Inline diffs with tinted lines, unified diff in a side pane, the composer's mode and model controls | Editor chrome |
| Raycast | Palette interaction model, keyboard hints (`Kbd`), how empty states point at one action | Its list-only surface |
| Zed | Overlay title bar handling on all three OSes, restraint | Editor-specific layout |

Principles, in priority order:

1. **Nothing is a default.** Every colour, size and radius on screen is in the table below. A component that needs a value not in the table is a request to change the table, not a local exception.
2. **Hierarchy from surfaces and hairlines, not shadows.** Shadows exist only on things that float.
3. **One accent, rationed.** Orange means "this is active, primary or yours to decide". Everything else is neutral.
4. **Every state is designed.** Loading, empty, error, streaming, disabled, selected and "needs your decision" are drawn before the happy path is wired.
5. **Motion explains a change and then gets out of the way.** No idle motion, ever.
6. **Keyboard reaches everything.** If it cannot be done from the palette or a shortcut, it is not finished.
7. **Native where it matters.** Title bar, drag regions, window background, scrollbars, menus and dialogs behave like the OS expects; the app never looks like a website in a window.

## 2. Decisions

| # | Decision | Choice | Why |
|---|----------|--------|-----|
| A1 | Design read | Linear chrome, Claude conversation | Chrome is scanned, conversation is read; one density for both fails one of them |
| A2 | Theme | Follows the OS by default; dark designed first, light derived from the same table and finished before M1 | Contrast mistakes show in dark first; a light-mode Mac user must not open a dark window |
| A3 | Accent | Gantry orange, rationed: primary button, focus ring, running indicator, active mode chip, selection. Text links and secondary states stay neutral | Chrome must not compete with streaming content |
| A4 | Type | Inter (UI and chat) and JetBrains Mono (paths, commands, code, diffs), both bundled | Best small-size rendering across WebKit, WebView2 and WebKitGTK; identical on every OS |
| A5 | Title bar | Overlay on every OS: macOS traffic lights inset into the sidebar; our own window controls on Windows and Linux; the top strip is a drag region | The sidebar reaches the top edge; the window reads as one designed object |
| A6 | Sidebar | One labelled sidebar, 240 px, resizable 200–320, `Cmd/Ctrl+B` hides it entirely; a top-left button restores it | One layout to design; an icon rail is a second layout with no extra information |
| A7 | Activity | Inline in the assistant message, in the order it happened: each run of reasoning and tool work between two pieces of text folds behind one line ("Created an artifact, ran 2 commands"; the current step while it runs), collapsed by default; detail opens in the right pane | Reading order stays honest; matches 05 §1. The website's hero mock shows a side feed and is not the reference |
| A8 | Density | Comfortable by default; Compact is a setting (11 §2) | First impressions and screenshots are the comfortable setting |
| A9 | Shape | Radius 4 / 6 / 8 / 12; hairlines for structure; shadows only on menus, popovers, dialogs and the pane when it overlays | Precision reads as quality; soft cards make diffs look like widgets |
| A10 | Motion | 120–200 ms ease-out, only where it explains a change; nothing loops except the skeleton shimmer; off under reduced motion | Frames on WebKitGTK are finite and reading tool output must not be interrupted |
| A11 | Icons | Phosphor, regular weight, one family | Consistent 1.5 px strokes and enough glyphs; Lucide is the shadcn default look this design avoids |
| A12 | Surfaces | Sidebar one step darker than content, hairline between | Anchors the chrome; the conversation is the brightest thing on screen |
| A13 | Composer | Floating card at the bottom of the chat column: text on top, one toolbar row below (`+`, mode chip, model picker, roots · thinking, Send) | Mode and model are visible without a separate bar; the eye lands there on every screen |
| A14 | Messages | User in a subtle tinted block, right-aligned, max 75 % of the measure; assistant plain text flush left with no bubble or avatar; model label on hover | Markdown, code, diffs and activity rows share one clean column |
| A15 | Palette | `Cmd/Ctrl+K` global palette: chats, messages, actions, settings sections, projects, connector install | Widens the M2 search into the thing that makes the app keyboard-first |
| A16 | App icon | Orange portal-frame mark on a near-black rounded tile with a faint inner gradient | Reads at 16 px; dark tiles are the category norm |
| A17 | Right side | One resizable pane, opening at half the window and resizable down to 360 px, with tabs: artifacts, and tool-call or diff detail as a temporary tab | One mechanism; an artifact gets real room by default, the way Claude's panel does |
| A18 | Settings | **Two dialogs over the app** (decided 2026-09-07, replacing the full page): **Settings** (General, Appearance, Providers & models, Guard, Data & privacy, Advanced, About) with a search box over its rail, and **Customize** (Connectors, Skills, Memory). Each is 1000 × 760 at most, a rail on the inset ground and the section scrolling beside it, and each rail's foot opens the other | Settings is a detour from a conversation, not a place; a dialog returns you to the exact chat you left. Splitting what the app does from what you add to it keeps both rails short |
| A19 | Diffs | Row: first hunks inline, tinted lines, no gutter, mono 12 px. Pane: unified with line numbers by default, side-by-side toggle, Revert. The same view serves the code surface's **Changes** tab, which lists every file a session touched with its net counts and shows the selected one underneath (16 §5) | Unified fits a 400 px pane; side-by-side is a choice, not a default |
| A20 | First launch | Three steps (add a key, pick a theme, optionally add a folder), then an empty chat with a short welcome, three suggested prompts and a hint to the `+` menu | The empty state exists from day one instead of being retrofitted in M13 |

## 3. Colour

Two palettes from one table. Names are the CSS custom properties without the `--` prefix; Tailwind exposes each through `@theme` so `bg-surface`, `text-fg-2` and `border-line` are utilities. Values are the starting point for layer 2; every text/background pair is checked there against WCAG AA (4.5:1 body, 3:1 large text and UI edges) with a script before the tokens are committed, and any value that fails is adjusted here first.

Neutrals are zinc with no hue bias. Alpha neutrals (hover, borders) are used instead of solid greys wherever they sit on more than one surface, so one token works on every step.

### Surfaces

| Token | Dark | Light | Used for |
|-------|------|-------|----------|
| `bg-base` | `#111113` | `#f4f4f5` | Window background, sidebar, title strip, settings section list |
| `bg-surface` | `#18181b` | `#ffffff` | Content area: chat column, settings content, pages |
| `bg-raised` | `#1f1f23` | `#ffffff` + `line-subtle` | Composer card, cards, the user message block base, inputs |
| `bg-overlay` | `#232327` | `#ffffff` | Menus, popovers, dialogs, tooltips, the palette |
| `bg-inset` | `#0d0d0f` | `#f4f4f5` | Code blocks, command output, diff pane background |
| `bg-hover` | `rgb(255 255 255 / 0.04)` | `rgb(0 0 0 / 0.04)` | Hover on rows, menu items, ghost buttons |
| `bg-active` | `rgb(255 255 255 / 0.07)` | `rgb(0 0 0 / 0.07)` | Pressed state |
| `bg-selected` | `rgb(255 255 255 / 0.09)` | `rgb(0 0 0 / 0.08)` | Selected row (sidebar current chat, list selection) |
| `bg-backdrop` | `rgb(0 0 0 / 0.5)` | `rgb(0 0 0 / 0.3)` | Behind dialogs |

### Lines

| Token | Dark | Light | Used for |
|-------|------|-------|----------|
| `line-subtle` | `rgb(255 255 255 / 0.06)` | `rgb(0 0 0 / 0.06)` | Separators inside a surface, card edges on raised surfaces |
| `line` | `rgb(255 255 255 / 0.10)` | `rgb(0 0 0 / 0.10)` | The sidebar/content hairline, input borders, pane edges |
| `line-strong` | `rgb(255 255 255 / 0.16)` | `rgb(0 0 0 / 0.16)` | Hovered input border, focused secondary button, drag handles |

### Text

| Token | Dark | Light | Used for |
|-------|------|-------|----------|
| `fg` | `#ededef` | `#18181b` | Primary text, titles, message text |
| `fg-2` | `#a8a8ad` | `#52525b` | Secondary text: explanations, meta, sidebar section labels, timestamps |
| `fg-3` | `#8b8b92` | `#6b6b73` | Tertiary: placeholders, disabled-looking hints, tool argument summaries |
| `fg-disabled` | `#55555b` | `#a1a1aa` | Disabled controls (never for information) |
| `fg-on-accent` | `#1a0a00` | `#1a0a00` | Text and icons on the accent |

`fg-3` on `bg-surface` is the lowest-contrast text pair in the app and must stay at or above 4.5:1 in both themes; it is the pair the contrast script watches first.

### Accent

| Token | Dark | Light | Used for |
|-------|------|-------|----------|
| `accent` | `#ff7a1f` | `#ea640b` | Primary button, active mode chip fill, running indicator, selection caret, switch on-state |
| `accent-hover` | `#ff8a3d` | `#d85c0a` | Primary button hover |
| `accent-text` | `#ff9a55` | `#b94a05` | Orange text where text must be orange (a link inside a permission card, the "needs decision" count); never body text |
| `accent-subtle` | `rgb(255 122 31 / 0.12)` | `rgb(234 100 11 / 0.10)` | Tinted backgrounds: pending decision rows, the active mode chip at rest |
| `focus` | `#ff7a1f` | `#ea640b` | The focus ring, 2 px, 2 px offset, on every focusable element without exception |

Where orange is **not** used: sidebar selection (that is `bg-selected`), text links (`fg` with an underline on hover), icons (`fg-2`), headings, charts, badges. The accent appears a handful of times on any screen; if a screenshot shows more than four orange elements, something is misusing it.

### Semantic

Separate from the accent. Used only for status and diffs, never for decoration.

| Token | Dark | Light | Used for |
|-------|------|-------|----------|
| `good` | `#3fb950` | `#15752f` | Connected, passed, allowed, added lines |
| `warn` | `#d29922` | `#8a5c00` | Needs attention, reconnect, always-confirm tier |
| `bad` | `#f85149` | `#cf222e` | Errors, denied, blocked by guard, destructive buttons, removed lines |
| `info` | `#58a6ff` | `#0969da` | Notices ("earlier conversation summarized"), rarely |
| `diff-add-bg` | `rgb(63 185 80 / 0.15)` | `rgb(21 117 47 / 0.10)` | Added line background |
| `diff-del-bg` | `rgb(248 81 73 / 0.15)` | `rgb(207 34 46 / 0.10)` | Removed line background |

Each semantic colour also has a `-subtle` alpha tint at 0.12 (dark) / 0.08 (light; `bad` and `info` at 0.10) for badge and row backgrounds, following the accent pattern. The light values of `fg-3`, the accent set, `good` and `warn` were darkened in M0b so every pair passes the contrast script; the dark `fg-3` was lightened for the same reason.

### Risk tiers

The five connector risk tiers (04 §1) plus `app` are shown with a small filled dot and a label, never a coloured background: `read` and `app` in `fg-3`, `write` in `fg-2`, `execute` in `warn`, `destructive` and `network-write` in `bad`. Colour is a secondary cue; the label is the primary one.

## 4. Typography

Inter Variable and JetBrains Mono Variable, bundled as woff2 in `desktop/frontend/src/assets/fonts/` with `font-display: block` (the app is local; a swap flash is worse than a 30 ms delay). `-webkit-font-smoothing: antialiased` on macOS only; Windows and Linux keep the OS rendering. Inter features: `cv11` (single-storey a) everywhere; `tnum` wherever digits align (tables, timers, token counts, diff stats). Tracking is zero at or below 15 px and `-0.01em` from 18 px up.

| Role | Face | Size / line | Weight | Used for |
|------|------|-------------|--------|----------|
| `micro` | Inter | 11 / 16 | 500 | Badges, `Kbd` hints, tier labels, uppercase section labels with `0.04em` tracking |
| `meta` | Inter | 12 / 16 | 400 | Timestamps, counts, secondary rows in lists, tooltips |
| `ui` | Inter | 13 / 20 | 400 · 500 | The default for all chrome: sidebar, settings rows, menus, buttons, inputs, activity rows, cards |
| `body` | Inter | 14 / 22 | 400 | Longer chrome text: settings explanations, dialog bodies, connector READMEs |
| `chat` | Inter | 15 / 24 | 400 | User and assistant messages, the composer |
| `title` | Inter | 16 / 22 | 500 | Dialog and pane titles, connector names on detail pages |
| `page` | Inter | 18 / 24 | 600 | Page titles: settings section, project, connector browse |
| `hero` | Inter | 22 / 28 | 600 | Onboarding and the empty chat welcome only |
| `mono` | JetBrains Mono | 12 / 18 | 400 | Paths, commands, argument summaries, diff hunks in rows, inline code in chrome |
| `code` | JetBrains Mono | 13 / 20 | 400 | Code blocks in messages, the diff pane, command output, artifact source |

Rules: 500 is the heaviest weight in chrome, 600 exists only for page and hero titles, and 700 is not used. Italic is not used. Uppercase appears only in `micro` section labels. Chat markdown headings map to `title` (h1, h2) and `ui` 500 (h3 and below), never to `page` or `hero`, so a heading in an answer never outranks the app's own titles. The chat measure is 720 px, which is about 72 characters at 15 px.

## 5. Spacing, sizing, shape

Base unit 4 px; layout on an 8 px grid.

| Token | Value | Used for |
|-------|-------|----------|
| `space-1` … `space-8` | 4, 8, 12, 16, 20, 24, 32, 48 | The only spacing values |
| `gutter-chrome` | 16 | Sidebar and pane inner padding |
| `gutter-content` | 24 | Chat column and settings content padding |
| `row` | 32 (comfortable) · 28 (compact) | List rows, settings rows minimum, activity rows |
| `row-sidebar` | 28 · 24 | Sidebar items |
| `control-sm` / `-md` / `-lg` | 24 / 28 / 32 | Button and input heights; `md` is the default, `lg` is the primary Send and dialog actions, `sm` is toolbar and row actions |
| `icon` | 16 (chrome) · 14 (inside `sm` controls) · 20 (empty states, onboarding) | Phosphor at these three sizes only |
| `sidebar` | 240 default, 200–320 | Persisted in the UI store |
| `pane` | 360 minimum, 50 % maximum | Persisted; remembers its width per kind (artifact vs detail) |
| `measure` | 720 | Chat column maximum width; centred with `gutter-content` on each side |
| `composer-max` | 40 vh | The text area grows to this, then scrolls |
| `title-strip` | 38 | Height of the drag region at the top of the sidebar and content (matches macOS traffic-light placement) |

Radius scale: `r-1` 4 (badges, checkboxes, `Kbd`, tier dots' containers), `r-2` 6 (buttons, inputs, menu items, chips), `r-3` 8 (rows, cards, code blocks, the user message block, tabs), `r-4` 12 (dialogs, popovers, the palette, the composer card). Nothing is fully round except the switch thumb, status dots and the running indicator.

Density switches `row`, `row-sidebar`, and the vertical padding of settings rows and activity rows through one `data-density` attribute on `<html>`; nothing else changes.

## 6. Elevation

| Level | What | Treatment |
|-------|------|-----------|
| 0 | Everything by default | Flat on its surface; separation by `line-subtle` or spacing |
| 1 | Raised: composer card, cards, inputs, user message block, connector tiles, the right pane | `bg-raised` plus a `line-subtle` border; no shadow |
| 2 | Floating: menus, popovers, tooltips, the palette, toasts | `bg-overlay`, `line` border, shadow `0 8px 24px rgb(0 0 0 / 0.35)` dark / `0 8px 24px rgb(0 0 0 / 0.10)` light |
| 3 | Dialogs, the pane when it overlays at narrow widths | Level 2 plus `bg-backdrop` behind |

A 1 px inner top highlight (`rgb(255 255 255 / 0.04)`) is allowed on level 2 and 3 in dark only; it is the one decorative touch and it is there because flat dark panels on dark surfaces lose their edge.

## 7. Layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●   Gantry ▾                       ⋯ drag region ⋯          [pane] [–][□][×]│  title strip 38px
├────────────┬───────────────────────────────────────────┬─────────────────────┤
│ + New chat │                                           │ Artifacts · Detail   │
│ ⌕ Search ⌘K│      ┌─────────────── 720px ───────────┐   │ ┌─────────────────┐ │
│            │      │ Assistant text …               │   │ │ tab   tab   ×   │ │
│ Pinned     │      │ ▸ 7 tool calls · 3 files · 2 cmd│   │ ├─────────────────┤ │
│  · Chat    │      │   ✎ src/auth.rs                    +12 −3        │   │ │                 │ │
│  · Project │      │   $ cargo test -p api   ✓ 1.4s  │   │ │  artifact or    │ │
│            │      │ ┌─ Permission ─────────────────┐│   │ │  unified diff   │ │
│ Projects   │      │ │ GitHub · create_pull_request ││   │ │                 │ │
│  · …       │      │ │ [Allow once] [This chat] Deny││   │ │                 │ │
│            │      │ └──────────────────────────────┘│   │ │                 │ │
│ Recents    │      │                     ┌──────────┐│   │ │                 │ │
│ Today      │      │                     │ user msg ││   │ │                 │ │
│  · …       │      │                     └──────────┘│   │ │                 │ │
│  · …       │      ├─ composer card ─────────────────┤   │ │                 │ │
│            │      │ Message Gantry…                  │   │ │                 │ │
│            │      │ [+] [Auto-edit ▾] [Claude ▾]  ↑  │   │ └─────────────────┘ │
│ ⚙ Settings │      └─────────────────────────────────┘   │                     │
└────────────┴───────────────────────────────────────────┴─────────────────────┘
  bg-base 240px        bg-surface, measure centred          bg-surface, 360px+
```

- **Title strip.** 38 px, part of every surface it crosses, `data-tauri-drag-region`. macOS: traffic lights at their native inset over `bg-base`, the sidebar's first row starts below them. Windows and Linux: three window controls drawn at the top right in `fg-2`, hover `bg-hover`, close hover `bad` with white glyph; double-click on the strip maximises. The window's `backgroundColor` is set from `bg-base` before the webview paints (11 §3), so a new window never flashes white.
- **Sidebar.** `bg-base`, hairline `line` on its right. The logo and wordmark in the strip. Order (Claude Desktop's, chosen 2026-09-07): New chat, Search, Projects, Artifacts; then Pinned chats; then Chats, a flat list most-recent-first with no day groups; Settings pinned to the bottom. Projects are not pinned in the sidebar; they live under Projects. Items are `row-sidebar` tall, `ui` size, icon 16 in `fg-2`, label in `fg`; the current chat is `bg-selected`; a running chat shows a 6 px `accent` dot pulsing once per second and "needs your decision" shows a `micro` count in `accent-subtle`. Section labels are `micro` uppercase `fg-3`. Hidden with `Cmd/Ctrl+B`; a `sidebar` icon button appears at the top left of the content area when hidden. Resizing by dragging the hairline, 200–320 px, cursor `col-resize`, no drag handle drawn until hover.
- **Chat column.** `bg-surface`, messages centred at `measure`. A turn is: user block, then the assistant text with its work folded inline (`TurnSteps`, A7): reasoning alone shows as the **thinking block** (one collapsed `ui` line in `fg-3` with a caret: "Thinking…" pulsing while it streams, "Thought for 4 s" after; expands to the reasoning at `ui` size behind a hairline); reasoning mixed with tool calls folds behind one `fg-2` line that names the work ("Created an artifact, read 3 files") or the current step ("Creating Dashboard…" with a 12 px spinner), and expands to the thinking blocks and activity rows in order behind a hairline. The spinner follows the turn, not the rows: once a turn has stopped the line names what actually ran ("Stopped before any work ran" when the answer was cut short before a call finished) and the cancelled rows say `cancelled`, so a stop always settles the line. After the text, an `ArtifactCard` per artifact the turn created or changed; then an optional interaction card, an inline error row in `bad-subtle` when the turn failed, and a `meta` footer on hover (model, duration, tokens). Scroll pins to the bottom while streaming and releases the moment the user scrolls up; a "↓ New" pill in `bg-overlay` appears at the bottom when new content arrives while released.
- **Composer.** Level 1 card at `r-4`, 12 px inner padding, sits 16 px above the bottom edge with the column's gutters. Text area in `chat` size with a `fg-3` placeholder ("Message Gantry…"; with a workspace: "Ask about or change *repo*…"). Toolbar row below at `control-sm`: `+` (icon button), mode chip, model picker, root chips; right side: thinking selector when the model supports it, Send as a `control-md` primary icon button that becomes Stop (square glyph, `bad` on hover) during a turn. `Enter` sends, `Shift+Enter` newlines, `Shift+Tab` cycles modes, `Esc` cancels a running turn after a confirmation toast.
- **Right pane.** A level 1 card at `r-4`, floating 8 px in from the window's right and bottom edges under the title strip, the way Claude's artifact panel sits over the chat; the sandbox document inside it paints the same `bg-raised`. Opens from the right at 200 ms at half the window (A17), pushes the chat column; at window widths under 1100 px it overlays the chat instead (level 3, the card gains the float shadow). Tabs at the top in `ui` 500: artifacts by title with their type icon, a temporary detail tab (diff, command, tool call, guard) in `fg-2` italic-free with a dot, closed with `×` or `Esc`. A toolbar row under the tabs belongs to the content (artifact toolbar per 13 §4: view glyphs, version stepper, Copy and a menu; diff toolbar: unified/side-by-side, wrap, Revert).
- **Settings and Customize.** Two dialogs on the same frame (`PrefsDialog`, A18): a level 3 panel `--prefs-width` × `--prefs-height` centred over the app, a `--settings-list` rail on `bg-base` (search box first in Settings, section rows with a Phosphor glyph, current in `bg-selected`), and the section itself scrolling on `bg-raised` with a `title` heading and stacked rows. A row: label in `ui` 500, explanation in `meta` `fg-2` underneath, the control at the right edge (switch, select, button, or a key status). Tables (Providers) are rows too, with the control column holding the actions. The rail's foot holds one link to the other dialog; the close button sits outside the scroller so it never leaves. Connectors, Skills and Memory live in Customize, connectors under Discover / Your connectors tabs.
- **Pages** (connector browse, connector detail, project, skills, memory) use the settings content column without the left list: `page` title, `body` intro, then content at `measure` or, for grids, the full width minus gutters.

## 8. Components

Two tiers. Both live in the app repository, in `desktop/frontend/src/components/ui/` (primitives) and `desktop/frontend/src/components/gantry/` (composites), and every one has an entry in the gallery route (§11).

### Primitives (shadcn on Base UI, reshaped once)

Button (primary · secondary · ghost · danger; `sm` / `md` / `lg`; icon-only variant; loading state replaces the label with a 12 px spinner and keeps the width), Input, NumberInput (a bounded integer with `−` / `+` steppers at `control-md`; no native spinner), Textarea, Select, Combobox, Checkbox, Switch, RadioGroup, Tabs, Dialog, Popover, DropdownMenu, ContextMenu, Tooltip (`meta` size, 400 ms delay, no arrow), Toast (bottom right, level 2, `ui` size, auto-dismiss 5 s, one action at most), ScrollArea (overlay scrollbars, 6 px thumb in `line-strong`, visible on hover and while scrolling), Separator, Badge (`micro`, `r-1`, neutral or semantic `-subtle` fill), Kbd (`micro` mono, `r-1`, `line` border), Skeleton (`bg-hover` shimmer), Command (the palette list).

The reshape pass replaces every shadcn default with a token, removes the default shadows and rings, sets the sizes above, swaps Lucide for Phosphor, and standardises the focus ring. It is done once, in the design milestone, and later shadcn updates are merged by hand.

### Composites

| Component | The shape of it |
|-----------|-----------------|
| `TitleStrip`, `WindowControls` | §7 |
| `SidebarItem`, `ChatRow` | §7 sidebar; a chat row is a small dot (hollow at rest, filled accent while running), the title and a pending count; context menu on right-click |
| `UserMessage` | `bg-raised` block, `r-3`, 12 × 16 px padding, right-aligned, max 75 % of `measure`; an attached image as an 80 px thumbnail read back from its blob and opening full size on click, other files as chips, both above the text |
| `AssistantMessage` | Plain `chat` text; markdown per 01 §5 with `CodeBlock` (level 0 on `bg-inset`, `r-3`, `code` size, language label and Copy in the top right on hover). GitHub flavour plus maths: tables (scrolling inside the column, with Copy as markdown on hover), task lists, footnotes, strikethrough, images bounded at 384 px that open full size, `$`/`$$` maths typeset with KaTeX whose fonts ship with the app, and a `mermaid` fence drawn in the artifact sandbox with a Source toggle. A link never navigates the app: it is confirmed and handed to the system browser |
| `ImageLightbox` | One image over the app at up to 88 vh, from a thumbnail in the composer, a sent message or an answer; Escape closes |
| `TurnSteps` | One `ui` row in `fg-2` per run of reasoning and tool work: chevron, "Created an artifact, read 3 files, ran a command" (one fragment per kind of work, in order of first occurrence; "· 1 failed" in `bad` when a call failed), or the current step with a spinner while it runs; collapsed by default; click or `→` expands to the thinking blocks and activity rows behind a hairline |
| `ArtifactCard` | After the assistant text, one per artifact the turn created or changed: a `bg-raised` bubble at `r-4` with a hairline, max 512 px, holding a 44 px `bg-surface` tile with the type glyph, the title in `body` 500, "Markdown · v2" (the artifact's current version) in `meta` `fg-3`, and a secondary Open button; the whole bubble opens the panel |
| `ArtifactLibrary` | The Artifacts page: every artifact across every chat, newest change first, as rows (type glyph tile, title, "Markdown · v2 · chat title", relative time); a row opens the chat with the artifact in the pane |
| `ActivityRow` | 32 px min, icon 16 (connector mark or kind glyph in `fg-2`), title in `ui`, argument summary in `mono` `fg-3`, status at the right (spinner · `good` check · `bad` cross · elapsed in `meta`); variants edit (with `HunkPreview` below), command (last three output lines in `mono` on `bg-inset` while running, Kill button), connector call (progress bar 2 px `accent` when reported), read/search, guard mark (`good` "guard ✓" in `meta` or a `bad-subtle` row "Blocked by guard: reason" with Allow anyway), notice (`info` icon, `meta`), artifact ("Created artifact · Title" with the type icon; opens the pane), context ("2 skills, 5 memories") |
| `HunkPreview` | First two hunks, `mono`, tinted lines with a 2 px left bar in `good`/`bad`, no gutter, "Show all in pane" link |
| `PermissionsDialog` | The chat's Permissions panel (04 §8), from the chat row's menu: its mode and guard, then each standing grant as a row with what it allows, which connector it came from, when it was granted and a Revoke, plus Revoke all |
| `InteractionCard` | The shared shell for permission, access request, connector suggestion, elicitation, auth required, skill and memory proposals (04 §7): level 1, `r-3`, a 2 px `accent` left bar, title in `ui` 500 with the connector mark, the request in `body`, a tier dot and label, then the actions row: primary action, secondary, Deny as ghost `bad`; a scope selector where grants apply. Pending cards also raise the sidebar count. As built it carries three: `PermissionCard`, `AccessRequestCard` ("GitHub in this chat?", the model's reason, **Attach for this chat** · **Attach and allow …** · **Not now**) and `ConnectorSuggestionCard` ("Install Cloudflare Workers?", the reason, badges for what it needs, **Install** · **Not now**) |
| `AttachmentTray` | What is waiting to be sent: an image as a 56 px thumbnail that opens full size, everything else as a chip, each with a × to remove. A pasted image is attached from the clipboard, including on WebKitGTK where the webview hands over no file and the app asks the system clipboard itself |
| `Composer`, `ModeChip`, `ModelPicker`, `RootChip`, `SlashMenu` | §7 composer; the mode chip is `control-sm`, `accent-subtle` fill with `accent-text` label when the mode is Auto, neutral otherwise; the guard state is a suffix ("Auto · guarded") |
| `PaneTabs`, `ArtifactPanel`, `DiffView`, `CommandOutput`, `ToolCallDetail`, `JudgeDetail` | §7 right pane; `DiffView` is CodeMirror merge in unified mode with the diff tokens; `CommandOutput` renders ANSI on `bg-inset` in `code` |
| `SurfaceToggle` | Two Phosphor icons at 16 in a segmented control, 28 px tall, in the title strip above the sidebar's first row: a speech bubble for Chat, an angle bracket for Code, the active one on `bg-selected` with its icon in `fg` and the other in `fg-2`, each with a tooltip. `Cmd/Ctrl+Shift+K` toggles (16 §4) |
| `SessionRow`, `FolderChip`, `ChangesPane`, `FirstRunNotice` | The code surface (16 §5): a session row is the title with its folder name in `micro` `fg-3` underneath; the folder chip sits where the root chips do and opens the add-or-switch menu; `ChangesPane` is the pane's home tab there, a file list over a `DiffView` with per-file and session-level Revert; `FirstRunNotice` is the one-time paragraph naming the three connectors the surface just turned on, dismissible for good |
| `SettingsList`, `SettingsSection`, `SettingsRow`, `ProviderRow`, `KeyStatus` | §7 settings; `KeyStatus` is a `Badge`: `none` neutral, `set ····abcd` `good-subtle`, `invalid` `bad-subtle` |
| `ConnectorTile`, `ConnectorCard`, `ConnectorHeader`, `InstallSteps`, `RuntimeCheck` | Browse is a grid of tiles (mark 24, name, one line, Built in / MCP badge, an Installed check); the install dialog is a numbered vertical stepper (03 §11) |
| `CommandPalette` | Level 2, 560 px wide, top-aligned at 15 vh, `ui` size, groups (Actions, Chats, Messages, Settings, Projects, Connectors), `Kbd` hints at the right, fuzzy match with the matched characters in `fg` and the rest in `fg-2` |
| `EmptyState` | Icon 20 in `fg-3`, one `ui` 500 line, one `meta` explanation, one primary or secondary action; centred in whatever area is empty; never a paragraph |
| `Onboarding` | Three full-window steps on `bg-surface`: `hero` title, `body` text, one control, Continue / Skip; a three-dot progress indicator |
| `Welcome` | The empty chat: `hero` "What should we work on?", three suggested prompts as level 1 cards, a `meta` hint about `+` and `Cmd+K` |

Every icon button has a tooltip and an `aria-label`. Every destructive action (Delete chat, Remove connector, Revert, Clear all data) confirms in a dialog whose primary button is `danger` and names the object.

## 9. States

| State | Rule |
|-------|------|
| Loading | Skeletons in the shape of the content (rows, a message block, tiles), never a page spinner. Spinners are 12 px and live only inside a button or an activity row |
| Empty | `EmptyState` with exactly one action; the sidebar's empty Recents says "No chats yet" with New chat |
| Error | Inline where it happened (a `bad-subtle` row with the message and Retry); toasts only for transient confirmations and background failures; provider errors (rate limit, auth, context too long) render as a notice row with the specific action (Retry, Open Providers, Summarize) |
| Streaming | Text appends without per-character animation; a 2 px `accent` caret at the end of the growing block; activity rows appear with a 160 ms fade and 4 px rise and never shift earlier rows |
| Needs decision | The card is the only orange-barred thing in the column; the sidebar row shows the count; the window requests attention (`requestUserAttention`) when unfocused |
| Disabled | `fg-disabled` and no hover; a tooltip explains why when the reason is not obvious (no key, runtime missing) |
| Selected / current | `bg-selected`; text stays `fg` |
| Focus | The `focus` ring, always, including on rows and cards that are focusable; `:focus-visible` only |
| Offline provider | The model picker shows the provider dimmed with "No key" and links to Providers |
| Interrupted | A chat closed mid-turn reopens with a notice row "Interrupted" and Retry (09 M2) |

## 10. Motion

| Token | Value | Used for |
|-------|-------|----------|
| `dur-1` | 120 ms | Colour and opacity changes: hover, focus ring, chip state |
| `dur-2` | 160 ms | Menus and popovers (opacity + scale from 0.97 at the trigger), tooltips, activity rows appearing, toasts |
| `dur-3` | 200 ms | The right pane, the sidebar collapsing, dialog enter (opacity + scale from 0.98) |
| `ease-out` | `cubic-bezier(0.2, 0, 0, 1)` | Everything entering or changing |
| `ease-in` | `cubic-bezier(0.4, 0, 1, 1)` | Exits, at `dur-1` |

No spring physics, no layout animation on lists, no animated empty states, no parallax. The only continuous motions are the skeleton shimmer (1.6 s, low contrast), the running dot in the sidebar and the 12 px spinner. `prefers-reduced-motion` sets every duration to 0 except opacity at `dur-1` and stops the shimmer and the pulse. Implementation: CSS transitions and keyframes only; Motion (the library) is not a dependency of the app.

## 11. Gallery and enforcement

- `/dev/gallery` is a route compiled only in development builds: every primitive and composite in every state with fixture data, both themes side by side, both densities. It is the contract for layer 3 and the page reviewed by screenshot at the end of every milestone.
- `desktop/frontend/src/fixtures/` holds the fixture data that fills the gallery and the three mock screens: one chat with a full coding turn (reads, edits, a command, a permission card, a guard block, an artifact), one connector catalog, one settings state with two providers. The mock screens become the real screens when the backend arrives; the fixtures stay for the gallery.
- An ESLint rule (`no-restricted-syntax` on colour literals and `px` values outside `tokens.css` and the Tailwind theme) fails the build on raw values. Tailwind's palette is disabled with `--color-*: initial` so only the tokens exist as utilities.
- A contrast script (`pnpm design:contrast`) checks every text/surface pair in §3 in both themes and fails under 4.5:1 for text and 3:1 for UI edges.
- Screenshots: WebKitGTK locally through agent-eyes; macOS and Windows from CI artifacts once M0's builds exist. Both themes, both densities, the three mock screens and the gallery.

## 12. Accessibility

Contrast per §3. Full keyboard operation: every action reachable from the palette or a documented shortcut (`Cmd/Ctrl+N` new chat, `Cmd/Ctrl+K` palette, `Cmd/Ctrl+B` sidebar, `Cmd/Ctrl+,` settings, `Cmd/Ctrl+.` toggle the pane, `Shift+Tab` mode, `Esc` closes the topmost thing). Minimum target 24 px. Focus ring on everything. Live regions: streaming text is `aria-live="off"` with a polite announcement when a turn completes or needs a decision. Icon buttons labelled. Dialogs trap focus and restore it. Colour is never the only carrier of meaning: tiers and statuses have labels, diffs have `+`/`−` prefixes.

## 13. App icon

Master SVG at 1024: a `r` = 22 % rounded tile filled `#141416` with a radial highlight (`rgb(255 255 255 / 0.06)` at the top) and a 1 px inner edge, the portal-frame mark (two uprights and a beam, the same geometry as the website's `Logo.astro`) in `accent` at 56 % of the tile height, optically centred. `pnpm tauri icon` generates the `.icns`, `.ico` and PNG set; macOS gets the tile as-is (the OS masks nothing on Big Sur-style icons, so the tile carries its own radius and a 1 px shadow), Windows and Linux get the same artwork. The About screen shows the 128 px version.

## 14. Banned

Raw colour or pixel values in components · shadows for hierarchy · page-level spinners · bubbles or avatars on messages · italic, bold 700, or more than one uppercase style · a second accent · orange on links, icons or selection · emoji in the UI · a second icon family · decorative or idle motion · per-feature CSS files · `h-screen`-style fixed heights (use the grid) · placeholder text as labels · text that truncates without a tooltip · `Cancel` as a button label (use the verb: Keep, Stop, Close).

## 15. What this changes elsewhere

- **09 roadmap.** A new milestone between M0 and M1: **M0b — Design system and mock screens (1 week)**: tokens (§3–§6, §10) in code with the contrast script and the lint rule; the reshape pass over the primitives; the composites; `/dev/gallery`; the three mock screens on fixtures (chat with a coding turn and a permission card, connector browse, Settings → Providers); the title strip on all three OSes; the app icon. Done when the gallery and the three screens pass a screenshot review in both themes and densities, and the light theme passes contrast. M1 then wires streaming into the approved chat view.
- **01 §5 module map.** Adds `components/gantry/` (composites), `features/palette/` (replaces `features/search/`), `features/onboarding/`, `features/gallery/` (dev only), `fixtures/`, and `app/layout/TitleStrip.tsx` with `WindowControls.tsx`.
- **05 §1.** Unchanged in substance; the detail drawer is the right pane's temporary tab (A17).
- **11 §2.** Appearance gains nothing; density and theme already exist. The composer placeholder and the onboarding are the only new copy.
- **13 §4.** The artifact panel is the right pane; each open artifact is a closable tab, and its one-row toolbar (Rendered | Source as glyphs, the version stepper when there is more than one version, Restore and Fix this when they apply, Copy, and a menu with Download, Open in window and Edit source) sits at the top of the tab's content with the Problems strip at the bottom.
- **Website.** The hero mock's side feed stays as marketing; it is not updated to match A7.
- **What M0b built (2026-09-07).** Everything in §8 except the `⋯` row menu (the context menu covers it), plus the contrast script (`desktop/frontend/scripts/contrast.mjs`) and the ESLint rule. Two library constraints worth knowing: Base UI menu labels must sit inside a group, and floating layers portal to `<body>`, so the gallery's dark frame shows them in the app's theme.

## 16. Left out, on purpose

| Not included | Why |
|--------------|-----|
| Custom accent or font settings | 11 §7; one considered palette |
| Icon rail sidebar | A6 |
| Springs and layout animation | A10 |
| Storybook | A dev route in the real webview costs nothing and shows the truth; Storybook is setup weight for one developer |
| A design tool file (Figma) | The gallery and the mock screens are the design file; the browser is the canvas |
| Light-theme-only or dark-theme-only shortcuts | Both are first-class from M0b |
