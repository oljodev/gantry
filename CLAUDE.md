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

## The plan and the code

`docs/plan/README.md` is the architecture plan; read it before changing structure. Document 15 is
the app's design system: every colour, size, radius and duration lives in `src/styles/tokens.css`
and nowhere else. When code contradicts a plan document, update the document in the same commit.

## Building

- Frontend: `pnpm install && pnpm fonts`, then `pnpm dev` (browser only), `pnpm typecheck`,
  `pnpm lint`, `pnpm test`, `pnpm build`.
- Rust: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all`.
- The app: `pnpm tauri dev`. After changing a command: `cargo xtask gen-bindings` and commit
  `src/bindings.ts` (the `gen_bindings` test fails on drift).
- On this machine the editor runs in a Flatpak sandbox without WebKitGTK; anything that compiles
  `src-tauri` (`cargo build`, `cargo test --workspace`, `cargo xtask …`, `pnpm tauri …`) runs on
  the host through `host-spawn`, e.g. `host-spawn cargo test --workspace`. Pure crates check fine
  inside the sandbox. The website needs the user-local Node 22 in `~/.local/share/node-22/bin`.
