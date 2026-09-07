# 14 — Marketing website

Session 4 replaced the three.js site with a company-style site for an open-source project, planned with Olav one decision at a time (the plan is summarised in §11). The folders, the domain split and the release integration from earlier sessions stand; the stack, the visual system, the sitemap and the animation approach are new, and every change against a recorded decision is flagged in 01 §8 (T22–T26).

## 1. Folders, domains and hosting

| Folder | Contents | Deployed as | Host |
|--------|----------|-------------|------|
| `web/site/` | the Astro site: pages, components, one React island, content collections, docs; a standalone package with its own lockfile | Cloudflare Pages project `gantry-website`: root directory `web/site`, build command `pnpm build` (`astro check && astro build`), output `dist`, `NODE_VERSION=22`, `PNPM_VERSION=10.34.5`, build watch paths `web/site/**` | `oljo.dev` (apex); `www.oljo.dev` redirects to it |
| `web/client-metadata/` | the MCP OAuth Client ID Metadata Document and a one-paragraph `index.html` | Cloudflare Pages project `gantry-client-metadata`, root `web/client-metadata`, no build | `id.oljo.dev` |

**Domain.** The site is served from the apex. The client-metadata document keeps its own host because Pages serves one project per hostname and the metadata URL is Gantry's OAuth identity, which must never move with a redesign (03 §7). The single-project alternative (serve it from `website/public/oauth/` at `https://oljo.dev/oauth/client-metadata.json`) is recorded and still available.

**Node.** Astro 7 requires Node ≥ 22.12. The development machine runs a user-local Node 22 (`~/.local/share/node-22`, on the PATH for the site's scripts); `web/site/.node-version` says `22`; Pages gets `NODE_VERSION=22`. TypeScript is pinned to 5.9 until `@astrojs/check` accepts 7.

## 2. Direction

**Dark, one accent, company-style.** The references were n8n, Linear, Vercel, Ramp, Replit, Notion and Tavily. From them: a floating pill navbar with dropdown menus (n8n), two-tone headlines and product-mock figures inside bordered cells (Linear), a bento of small figures (Vercel), a searchable connector directory grouped by category (Ramp, Vercel Connect), line drawings for principles (Linear), a monospace eyebrow (Tavily). Nothing 3D.

**Design system** (`web/site/src/styles/theme.css`, the single source):

| Token | Value | Role |
|-------|-------|------|
| `bg` / `surface` / `surface-2` | `#0a0a0a` / `#111113` / `#17171a` | ground and raised surfaces |
| `line` / `line-2` | `#232326` / `#2e2e33` | hairlines |
| `fg` / `fg-2` / `fg-3` | `#f5f5f5` / `#a1a1aa` / `#8b8b92` | headings / body / metadata (≈19:1, 7.6:1, 5.8:1 on `bg`) |
| `brand` / `brand-hover` / `brand-ink` | `#ff7a1f` / `#ff8f47` / `#1a0a00` | the one accent, its hover, text on it (≈6.9:1) |
| `good` / `bad` | `#4ade80` / `#f87171` | diff colours in figures only |

Neutral zinc, no blue bias, one soft radial glow behind the hero and nowhere else. Connector marks are shown in their own brand colours (lifted towards white when they would vanish on the ground) and supply all the other colour on the site.

**Type.** Geist for everything, Geist Mono for eyebrows, feed rows, code and metadata; latin variable subsets vendored from Fontsource (SIL OFL 1.1, no reserved name) into `src/assets/fonts/` and served through Astro's Fonts API with hashed files, a preload for the sans, and generated fallback metrics. Display 56–72 px semibold with tight tracking; two-tone section titles (white sentence, grey continuation); body 17 px at 60–65 characters.

**Mark.** "Gantry" in Geist semibold beside a small orange portal frame (two uprights, a beam, the hoist below); the same drawing is the favicon.

**Components.** The pill navbar (sticky, blurred, `<details name>` dropdowns for Product and Resources, Connectors, Download, a GitHub chip whose star count is fetched at runtime and hidden on failure, one orange CTA; a `<details>` sheet under 52 rem); primary orange pill button and secondary hairline pill; bordered cards with a one-pixel top highlight and 12–16 px radius; figure frames drawn in the app's own language; built-in/MCP badges; the logo-cloud card.

## 3. Sitemap

| Page | What it is |
|------|------------|
| `/` | Hero (eyebrow, the headline "The AI workspace that stays on your machine.", one sentence, "Download for <OS>" and "View on GitHub", then the product-mock window bleeding off the bottom) · three-beat strip · "What agents can do": three Linear-style rows with figures (the activity feed; permission modes with a scroll-drawn wire; the connector logo cloud) · "One workspace": a bento of five figures · "Local by construction": three principles with FIG 0.1–0.3 line drawings · "In the open": repository card · download band |
| `/product/` | Five chapters (Chat, Agent, Permissions, Connectors, Local) in two-column form beside a sticky figure that switches as the reader scrolls; the risk-tier table; a download band |
| `/connectors/` | Ramp's directory: search, category pills, a live count, tiles grouped by category linking to each connector's page; "Missing one? Request a connector"; the trademark note. The full list is in the HTML |
| `/connectors/<slug>/` | One page per connector from the data file: mark, name, badges, summary, example tool calls, how it connects (sign-in, runtime, permissions), install steps, a facts card, related connectors, the request link |
| `/pricing/` | "Free." Three columns (Gantry, your model provider with links to their pricing pages, connectors and services); an FAQ |
| `/download/` | Three OS cards with Tauri's platform floors as requirements, the stable asset names, coming-soon state; verify, build from source, then add a key |
| `/about/` | Why Gantry exists, where it stands (the roadmap in plain words), the licence in a paragraph, how to follow along. No byline |
| `/blog/`, `/blog/<id>/` | Markdown/MDX posts with a list page, RSS, and a designed empty state |
| `/changelog/`, `/changelog/<id>/` | One entry per release with highlights, RSS, and a designed empty state |
| `/docs/…` | Starlight, restyled to the site's tokens, with the pill navbar as its header; pre-release pages written from these plan documents, each carrying a pre-release banner: Start here (install, keys, first chat, projects, the agent, permission modes), Connectors (overview, install, signing in, add your own), Reference (skills, memory, artifacts, settings, where your data lives, FAQ) |
| `/privacy/`, `/license/`, `/security/` | Trust pages in one prose column; Security ends with GitHub private vulnerability reporting |
| `/404` | Real 404 page (Pages needs `dist/404.html`) |

`/how-it-works/` from the previous site redirects to `/product/`. The site is written as if the repository is public: GitHub links everywhere, Issues and Discussions as the channels, no email.

## 4. Motion

- **Reveals.** Sections opt in with `data-reveal`; a 20-line IntersectionObserver script adds `.is-in` (fade + 14 px rise, 600 ms, staggered with `--reveal-delay`). Elements already on screen are marked before the `html.js-reveal` class turns the effect on, so nothing visible ever hides, nothing hides without JavaScript, and a 1.5 s safety timer reveals everything regardless.
- **The hero session.** A React island (the only React on the site) renders the app window: sidebar, chat, activity feed. The session is a typed event list; a pure `frameAt(t)` derives the visible frame for any time, so the server renders a mid-session snapshot (the permission prompt is up, the feed has history) and the client resumes from exactly that frame: no blank hero, no flash. Playback pauses off-screen and when the tab is hidden, loops through a cross-fade, and under reduced motion holds the snapshot. Motion via `LazyMotion` and `motion/react-m` for the feed rows and the prompt. Fixed inner dimensions so streaming never reflows.
- **The logo cloud.** 34 connector tiles plus two informational ones in a bordered card that is one link to the directory. Each tile carries two build-time phases (desktop and phone column counts) from its row + column; one keyframe brightens and colours a tile; `animation-delay: phase × 2.4 s` on a shared 7 s period keeps the waves coherent forever. Hover pauses; reduced motion shows the lit grid; no JavaScript involved.
- **Scroll-linked figures.** The product tour's sticky stage switches on an IntersectionObserver (inline figures on narrow screens and without JavaScript); the permission wire draws itself and the hero window settles with CSS scroll-driven animations inside `@supports (animation-timeline: …)`, finished state at rest.
- Dropdowns open with a short `@starting-style`-free keyframe; everything above is off under `prefers-reduced-motion`.

## 5. Technical approach

**Stack.** Astro 7.3 (static output, `trailingSlash: 'always'`, directory build format, `compressHTML`), Tailwind CSS 4.3 through `@tailwindcss/vite` with the tokens in `@theme`, React 19 for the hero island, Motion 13, Starlight 0.42 (pinned to the minor) for docs, `simple-icons` 16 resolved at build time, `@phosphor-icons/core` glyphs inlined at build time, `@astrojs/rss`. Starlight registers MDX, the sitemap and Expressive Code itself, so they are not listed separately.

```
web/site/
  .node-version  package.json  pnpm-lock.yaml  astro.config.mjs  tsconfig.json  README.md
  scripts/sync-fonts.mjs             copies the Fontsource woff2 files and licence into src/assets/fonts/
  public/                            _headers  _redirects  robots.txt  favicon.svg  connectors/ (cleared logo overrides)
  src/
    content.config.ts                blog, changelog, docs collections
    data/connectors.ts               THE connector list: slug, name, category, does, summary, capabilities, auth, runtime, website, icon
    lib/                             marks.ts (Simple Icons → mark, initials, legibleOnDark), contrast.ts, connectors.ts, nav.ts (site map), seo.ts, dates.ts
    styles/                          theme.css (tokens) · chrome.css (navbar/button classes, shared with docs) · components.css (marketing classes, reveal) · global.css (Tailwind + base) · docs.css (Starlight mapping)
    layouts/BaseLayout.astro  ProseLayout.astro
    components/                      Logo, Icon, nav/{Navbar,Footer}, home/{Hero,Strip,Capabilities,FeedFigure,PermissionsFigure,LogoCloud,Bento,Principles,OpenSource,DownloadBand}, product/Figures, connectors/{Mark,ConnectorTile}, download/DownloadButtons, docs/{ThemeProvider,Empty,Head,Header,Banner}
    islands/hero/                    HeroDemo.tsx, script.ts, reducer.ts, usePlayback.ts, hero.css, parts/
    scripts/                         reveal.ts, nav.ts, steps.ts, os.ts, releases.ts, connector-filter.ts
    pages/                           index, product/, connectors/{index,[slug]}, pricing/, download/, about/, privacy/, license/, security/, blog/{index,[id],rss.xml.ts}, changelog/{index,[id],rss.xml.ts}, 404
    content/                         blog/, changelog/ (with _template.md each), docs/docs/{index.mdx, start/, connectors/, reference/}
```

**Build-time resolution.** Connector marks: `simpleIconKey('google-drive') → siGoogledrive`; `icon` overrides the slug, `icon: false` forces initials; near-black brand colours are lifted with `color-mix(in oklch, hex 40%, white)`. First-party connectors use Gantry's own glyphs in orange. Nothing from `simple-icons` reaches the client. Provider marks in the bento: Anthropic, Google Gemini, OpenRouter, Ollama and Mistral exist; OpenAI and xAI do not and are text.

**Docs.** Starlight is mounted at `/docs/` by keeping the content in `src/content/docs/docs/**`; `src/pages/docs/` stays empty. It is dark by default at the CSS level; the `ThemeProvider` and `ThemeSelect` overrides are empty, `Head` adds the fonts, `Header` renders the site's navbar with Starlight's search in the pill, `Banner` shows the pre-release notice on every page. `docs.css` maps `--sl-*` onto the tokens and sets `--sl-nav-height` to the pill's height. CSS isolation: only `BaseLayout` imports `global.css` (Tailwind preflight); the docs load `theme.css` and `chrome.css` only, so Starlight's reset and the marketing classes never meet. Pagefind indexes the docs (and any blog post marked `data-pagefind-body`); search works in build and preview only.

**Releases.** `scripts/releases.ts` asks `api.github.com/repos/oljodev/gantry/releases/latest` once per page; with a release carrying the stable asset names (`Gantry-macOS.dmg`, `Gantry-Windows-x64.exe`, `Gantry-Linux-x86_64.AppImage`) every `[data-dl]` button gets its `releases/latest/download/` URL, the hero's primary button points at the visitor's build, and every version line fills in. Any failure leaves the coming-soon state, and every button still links somewhere useful (the download page, the releases page). The 404 in the console until then is expected.

**Headers and redirects.** `public/_headers`: nosniff, referrer policy, permissions policy, a CSP with `script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'` (Astro inlines the island bootstrap; Pagefind runs WebAssembly; hash-based tightening is a follow-up), immutable caching for `/_astro/*`. `public/_redirects` sends `/how-it-works/` to `/product/`. `robots.txt` names the sitemap.

**Payload.** Home HTML about 90 KB; one CSS bundle per layout; the hero island (react-dom, Motion's `domAnimation` features, the component) loads on `/` only; fonts 52 KB; no third-party request except the two optional GitHub API calls.

## 6. Accessibility and degradation

- Every page is complete without JavaScript: the menus and the mobile sheet are `<details>`, the hero shows its server-rendered frame, the logo cloud animates in CSS, the directory is complete with the filter toolbar hidden, download buttons are in their disabled state, the product tour shows every figure inline.
- `prefers-reduced-motion`: no reveals, no waves, the hero holds its frame, scroll-driven figures at rest, no dropdown animation.
- The hero window and every figure are `aria-hidden` with a text description beside them; the logo cloud is one link with an `aria-label`; the directory's count is a live region; the risk tables have headers; the navbar marks the current page with `aria-current` and closes on Escape; a skip link comes first.
- Contrast on the ground: body ≈ 7.6:1, metadata ≈ 5.8:1, brand text ≈ 6.9:1, near-black brand marks lifted to ≥ 3:1.

## 7. Verification done in session 4

`astro check` clean; 63 pages built; every route serves with a trailing slash; sitemap and both RSS feeds emitted; the directory's filter, search and count checked in the browser; the product tour's stage checked to switch chapters on scroll; the hero session checked to render its snapshot and play; the mobile sheet checked to open; desktop and mobile screenshots of home, connectors, a connector page, product, pricing, download, about, changelog and two docs pages reviewed. Not yet verified: the first Cloudflare deploy (`curl -I` on `/about`, cache headers, CSP violations in the console, the 301, `404.html`), and the docs search in a deployed build.

## 8. At every release

`docs/dev/release.md` keeps the checklist: the OS list matches what `release.yml` builds; stable-named assets were uploaded; a changelog entry exists in `src/content/changelog/`; the download page's requirements still match the toolchain; the pre-release banner comes off the docs (`src/components/docs/Banner.astro`) when the docs describe shipped behaviour. The version lines and buttons update themselves.

## 9. Content rules

- Copy is plain and direct, second person, no exclamation marks; product names are used as their owners spell them.
- Connector names and logos belong to their owners and indicate compatibility, not endorsement: said on the directory, on every connector page's footer line, and in the footer.
- Nothing on the site claims a feature that the plan documents do not describe; the docs carry the pre-release banner until the first release.
- Adding a connector is one entry in `src/data/connectors.ts`; the build finds its mark or uses initials; a cleared file in `public/connectors/` overrides.

## 10. Deliberately absent

A light theme, analytics, cookies, a newsletter, testimonials and customer logos (none exist), pricing tiers (there are none), a theme toggle, any 3D, any third-party script.

## 11. Decisions of session 4

Dark only · a built product mock in the hero · a complete company-style site · neutral base with orange as the one accent · Geist · the pill navbar with menus · reveals plus a few scroll-linked figures · the Ramp-style logo cloud · Astro + Tailwind 4 + a React island + Motion, Starlight for docs · the headline "The AI workspace that stays on your machine." · one page per connector · Privacy, License and Security, no Terms · written as if the repository is public, no email · About without a byline · the portal-frame mark · "Request a connector" on the directory and every connector page, and, as a follow-up for the app, in the Connectors modal (03 §11).
