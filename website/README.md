# Gantry website

The public site at https://oljo.dev: an Astro 7 static site with Tailwind 4, one React island (the hero's product
mock), Starlight for the docs, self-hosted Geist, and no third-party scripts at runtime. Dark only.

The design system, the sitemap, the motion approach and the verification checklist are recorded in
`docs/plan/14-marketing-site.md`.

## Develop

Node 22 or later is required (`.node-version`). On the development machine it lives in `~/.local/share/node-22`.

```sh
export PATH="$HOME/.local/share/node-22/bin:$PATH"   # if Node 22 is not the default
pnpm install
pnpm fonts       # once after install: vendors the Geist woff2 files into src/assets/fonts/
pnpm dev         # http://localhost:4321
pnpm build       # astro check && astro build → dist/
pnpm preview     # serve dist/ (docs search only works here, not in dev)
```

## Deploy

Cloudflare Pages project `gantry-website`: root directory `website`, build command `pnpm build`, output directory
`dist`, environment `NODE_VERSION=22` and `PNPM_VERSION=10.34.5`, build watch paths `website/**`. The package is
standalone (its own lockfile) so Pages builds it without the rest of the repository.

## Editing

| Change | Where |
|--------|-------|
| Add, remove or reorder a connector | one entry in `src/data/connectors.ts`; the directory, its page and the home-page logo cloud follow |
| Use a cleared logo file instead of the Simple Icons mark | drop it in `public/connectors/`, set `logo` on the entry |
| A colour or a font | `src/styles/theme.css` (the marketing pages and the docs both read it) |
| Page copy | the page under `src/pages/`; the hero session in `src/islands/hero/script.ts` |
| Navigation and footer links | `src/lib/nav.ts` |
| A blog post or a changelog entry | copy the `_template.md` in `src/content/blog/` or `src/content/changelog/` |
| Docs | Markdown under `src/content/docs/docs/`; the pre-release banner is `src/components/docs/Banner.astro` |
| Fonts | `pnpm fonts` after updating the `@fontsource-variable/geist*` packages |

## Release integration

Download buttons stay "coming soon" until the GitHub releases API reports a release carrying the stable asset names
`Gantry-macOS.dmg`, `Gantry-Windows-x64.exe` and `Gantry-Linux-x86_64.AppImage`; see `src/scripts/releases.ts`.
The 404 in the browser console until then is expected.
