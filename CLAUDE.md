# Gantry

## Keeping two machines in sync

This project is worked on from two machines, never at the same time. GitHub
`main` is the single source of truth, and the repo is set up so that neither
machine can quietly drift from it:

- A `SessionStart` hook runs `git pull --ff-only`. If it reports that local
  and GitHub have diverged, resolve that before doing anything else.
- A `Stop` hook pushes every local commit at the end of each turn, so a commit
  never sits on one machine. It also flags uncommitted changes.
- VS Code is configured to sync after every manual commit.

What this means for the way you work:

- Commit when a piece of work is done, in small coherent commits. The push is
  automatic.
- Never end a task with finished work uncommitted. If a session has to stop
  mid-way, commit the work in progress anyway, so the other machine can pick it
  up.
- Do not force-push and do not rewrite history on `main`.

## Layout

- `desktop/` is the product: `app/` (the Tauri crate `gantry-app`), `frontend/` (the React
  package `@gantry/frontend`), `crates/` (the engine crates and `xtask`), `connectors/`, `skills/`,
  `assets/` (prompts, model overrides, guardrails, branding) and `schemas/`.
- `web/` holds the two Cloudflare Pages sites: `site/` (oljo.dev, its own package and lockfile)
  and `client-metadata/` (id.oljo.dev).
- `docs/plan/` is the plan; the root keeps only the Cargo and pnpm workspace files, the licence
  documents and this file. `docs/plan/07-repository-structure.md` has the full tree.

## The plan and the code

`docs/plan/README.md` is the architecture plan; read it before changing structure. Document 15 is
the app's design system: every colour, size, radius and duration lives in
`desktop/frontend/src/styles/tokens.css` and nowhere else. When code contradicts a plan document,
update the document in the same commit.

## Building

- Everything runs from the repository root. Frontend: `pnpm install && pnpm fonts`, then
  `pnpm dev` (browser only), `pnpm typecheck`, `pnpm lint`, `pnpm test`, `pnpm build`; the root
  scripts forward to `desktop/frontend`, and `pnpm format` covers the whole repository.
- Rust: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`, and `cargo deny check` when dependencies change (licences, advisories,
  sources; it is the one check that used to exist only in CI).
- The app: `pnpm tauri dev` (the Tauri CLI finds `desktop/app/tauri.conf.json` by itself). After
  changing a command: `cargo xtask gen-bindings` and commit `desktop/frontend/src/bindings.ts` (the
  `gen_bindings` test fails on drift).
- Windows: `cargo xtask check-windows [--clippy]` type-checks the Windows build from here (it needs
  `rustup target add x86_64-pc-windows-msvc` once). It proves the `#[cfg(windows)]` code compiles
  and nothing more — nothing links and no test runs; `docs/dev/setup.md` has the checklist for what
  only a Windows machine can answer. Windows-only logic that is pure — a path rule, an encoder —
  is written as an ordinary function called under `cfg(windows)` and tested on every platform.
- On this machine the editor runs in a Flatpak sandbox without WebKitGTK; anything that compiles
  `desktop/app` (`cargo build`, `cargo test --workspace`, `cargo xtask …`, `pnpm tauri …`) runs on
  the host through `host-spawn`, e.g. `host-spawn cargo test --workspace`. Pure crates check fine
  inside the sandbox. The website needs the user-local Node 22 in `~/.local/share/node-22/bin`.
