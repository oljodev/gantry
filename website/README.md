# Gantry marketing site

The public page at https://oljo.dev. Static output, built with Vite as a bundler only (no framework), a three.js hero
scene as progressive enhancement, self-hosted fonts, no third-party scripts at runtime.

The art direction, the technical approach and the degradation strategy are recorded in `docs/plan/14-marketing-site.md`.

## Develop

```sh
pnpm install
pnpm dev        # http://localhost:5173
pnpm build      # → dist/
pnpm preview    # serve dist/
pnpm typecheck
```

## Deploy

Cloudflare Pages project `gantry-website`: root directory `website`, build command `pnpm build`, output directory `dist`.
The package is standalone (its own lockfile) so Pages can build it without the rest of the repository.

## Editing

| Change | Where |
|--------|-------|
| Add, remove or reorder a connector | one line in `src/connectors/data.ts` |
| Swap in a confirmed logo | drop the file in `public/connectors/`, set `logo` on the entry |
| Copy | `index.html` |
| Colours | `src/styles/tokens.css` (page) and `src/scene/palette.ts` (scene), kept in step by hand |
| Scene layout: stations, frames, camera | `src/scene/layout.js` (drives both the live scene and the static poster) |
| Fonts | `pnpm fonts` re-downloads the OFL subsets into `public/fonts/` and rewrites `src/styles/fonts.css` |

## Release integration

Download buttons stay "coming soon" until `releases/latest/download/releases.json` (uploaded by the release workflow)
or the GitHub releases API answers; see `src/releases.ts` and `docs/plan/14-marketing-site.md` §3.
