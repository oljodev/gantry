---
name: artifact-authoring
description: Build a Gantry artifact that renders the first time: choosing between markdown, code, html, svg, mermaid and react, the React component contract and its six importable modules, what the sandbox does not have, and how to answer a render error. Use when creating or editing an artifact, building a component, page, diagram, chart or interactive demo, or when an artifact failed to render.
license: FSL-1.1-ALv2
metadata:
  gantry-triggers: "artifact, component, react, chart, diagram, mermaid, svg, html page, interactive, render error"
  gantry-always: "false"
  gantry-version: "1"
  author: Gantry
---

# Writing an artifact

An artifact is content shown in a panel beside the chat, versioned and editable. It runs in a
sandbox with **no network, no storage, no tools and no clipboard** — the complete list of what
it can do is: draw, hold its own state, and report errors. Plan for that before writing a line.

## Choosing the type

| Type | Use it for | Not for |
|---|---|---|
| `markdown` | a document, a report, notes | an answer that belongs in the chat |
| `code` | one source file — set `language` | a snippet under a dozen lines |
| `mermaid` | a flow, sequence, state or ER diagram | anything needing exact placement |
| `svg` | an illustration, an icon, a fixed drawing | data that changes |
| `html` | a complete page with its own styles and scripts | a single component |
| `react` | something interactive: a control, a chart, a small tool | static content |

Pick the narrowest type that does the job. A `react` artifact that renders a paragraph is a
document with extra ways to fail.

## The React contract

- **One file.** Its **default export is the component**; a named `App` export is accepted as a
  fallback. It is rendered into the root with **no props** — a component that requires props
  renders nothing.
- Hooks and state work normally. `useEffect` runs. There is no router, no context provider
  above you, and no way to persist anything past a reload.
- **Tailwind classes are available.** Use them rather than inventing a stylesheet.
- Imports are limited to six modules; `references/react-runtime.md` lists them and what each one
  is good for. Read it with `gantry__read_skill_file` before reaching for anything else — an
  import outside the list fails at compile time, not at runtime.
- Every `for`, `while` and `do` body is instrumented with a three-second guard. A loop that runs
  longer throws `Artifact loop guard`, so compute a large result in chunks or precompute it.

## Data

There is no network. Data that the artifact draws is **written into the artifact**: a literal
array at the top of the file, named for what it is, with the real numbers from the conversation
rather than invented ones. If the numbers are examples, say so on the page.

## Answering a render error

The tool result carries `render.status` and, when it failed, the errors with a line and column
mapped back to your source. Read the line before rewriting anything — the common causes are an
import that is not on the list, a missing default export, and a typo in a hook name.

Fix it **at most twice**. If the third attempt would be a guess, stop and tell the user what the
error says and what you would need to resolve it. A silent third rewrite wastes a turn and
usually makes the diff harder to read.

## Editing

Use `gantry__edit_artifact` with exact text for a small change — it keeps the version history
readable and costs a fraction of a rewrite. Use `gantry__update_artifact` when more than about a
third of the file changes. Read an artifact with `gantry__read_artifact` before changing one you
did not write in this turn; the user may have edited it.

## Pitfalls

- Building `react` when `markdown` was the answer.
- A component that fetches. There is no network; the fetch fails silently and the panel shows an
  empty state you did not design.
- A chart with no axis labels, or labels the drawing does not reach. Check the extremes of the
  scale are inside the viewBox.
- Forgetting the introduction. One sentence in the chat saying what the artifact is; the panel
  does not speak for itself.
