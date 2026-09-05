# 14 — Marketing website

Session 3 replaced the plain static draft with the three.js page that now lives in `website/`. The content decisions from session 2 stand (folder placement, stable release asset names, the private-repository note); the domain, the build tooling and the release lookup changed, each flagged in 01 §8 (T19–T21).

## 1. Folders, domains and hosting

| Folder | Contents | Deployed as | Host |
|--------|----------|-------------|------|
| `website/` | the marketing page: `index.html`, `src/` (styles, page scripts, the scene), `public/` (fonts, favicon, connector logo assets), a standalone `package.json` and lockfile | Cloudflare Pages project `gantry-website`: root directory `website`, build command `pnpm build`, output directory `dist` | `oljo.dev` (apex); `www.oljo.dev` redirects to it |
| `client-metadata/` | the MCP OAuth Client ID Metadata Document and a one-paragraph `index.html` | Cloudflare Pages project `gantry-client-metadata`, root `client-metadata`, no build | `id.oljo.dev` |

**Domain.** The instruction is to use `oljo.dev` itself, so the page is served from the apex. The client-metadata document keeps a host of its own, `id.oljo.dev`, because Cloudflare Pages serves one project per hostname and the metadata URL is Gantry's OAuth identity: it must never change and must never be at the mercy of a marketing redesign (03 §7). If no subdomain at all is wanted, the alternative is to copy `client-metadata/` into `website/dist/.well-known/gantry/` at build time and use `https://oljo.dev/.well-known/gantry/client-metadata.json` as the client id; that works but reintroduces the coupling the split was made to avoid. The session-3 brief still refers to the metadata folder as `site/`; it has been `client-metadata/` since session 2.

`website/` is deliberately **not** part of the future application pnpm workspace (07): Pages builds it in isolation with its own lockfile.

## 2. Art direction

**The scene is a gantry at work.** One structure, seen from below and to the left, receding into fog: two rails on nine trussed portal frames, an orange overhead carriage travelling the rails, connector modules hanging from the rails at seven stations, and data flowing along the structure. The three ideas in the brief are not three scenes; they are three roles inside one mechanism:

- **The structure** is the literal gantry: I-beam rails, box-section columns and top beams, X-bracing in each frame and between frames, base plates. It ties the picture to the name and reads as precision engineering rather than abstract 3D.
- **The network** is the modules and the carriage. Each module is a connector; the carriage is the agent. When the carriage docks above a module, a link forms between them, the module's indicator lights, a caption names the tool and the call it is making (`code-editor · str_replace src/lib/auth.rs`), and a ripple crosses the dashed cables to the neighbouring modules. The graph is anchored to the rails, never floating.
- **The flow** is the particle stream: a steady bus of specks running along both rails, and short bursts that travel the link and the cables when a call happens. Particles only ever move along the structure's members, which is what separates this from a stock particle-network background.

**Composition.** Camera low and to the left, looking down the rails; the near frame stands at the right edge, the far end vanishes at about a third from the left behind a scrim. The hero copy sits in the left half over that scrim, so the structure emerges beside and behind the words instead of underneath them. A whisper of parallax follows the pointer (never on touch devices) and the camera dollies back slowly as the page scrolls away. Nothing spins, nothing floats.

**Palette.** One accent, gantry orange, used only for what is *active*: the carriage, the lit indicator, the link and the pulses, the primary download button. Everything else is steel: cool greys with a blue bias on a blue-black ground in the dark theme (`#0c1116`, `#8593a3`, `#e9843f`), graphite on a pale technical-drawing ground in the light theme (`#f2f4f6`, `#3e4a56`, `#d8702b`). Category tints on the connector placeholders are desaturated so they read as neutrals. The page follows the operating system theme and both themes are designed, not inverted: the scene has its own palette per theme, including separate fog distances.

**Type.** Barlow Semi Condensed for the headline and section titles (a road-sign grotesque with an engineered feel), Barlow for text, JetBrains Mono for the captions, the feed rows and metadata. All three are OFL and self-hosted from `public/fonts/` (five latin woff2 files, about 52 KB in total); no request leaves the site's own host for type.

**Motion.** The scene is the motion. Beyond it there is one scroll-triggered animation (the rows of the example feed arrive in order the first time the figure is seen, mirroring what the feed does in the app), hover and press feedback on buttons, and nothing else. Sections are never hidden behind a reveal, so nothing on the page can fail to appear.

**What it is not.** No centred hero over a gradient blob, no floating dots with lines between them, no scroll cues, no glow on everything, no fake window chrome. The one illustrative feed is typographic and captioned as illustrative.

## 3. Content

| Section | What it does | Layout family |
|---------|--------------|---------------|
| Header | Wordmark, three links (What it does, Connectors, GitHub with a "Private" tag) | single line, 64 px |
| Hero | Headline "Chat, code and connect. On your machine.", one sentence, the three download buttons (the visitor's platform is styled as primary, the others secondary), a version line that appears only once a release exists | split: copy left, scene behind and right |
| Watch it work | Four capabilities in plain sentences (edits files, runs commands, uses connectors, asks first) beside an illustrative activity feed. The 3D captions show the same actions abstractly; the copy makes them concrete. This is where the brief's "what agents can do" lands: the scene carries the feeling, the section carries the facts | two columns: copy and figure |
| Connectors | A data-driven grid of 34 tiles with category filter chips and a one-line note that marks are placeholders until each integration is confirmed | filter chips + grid |
| What Gantry is | Five cells (chat client, coding agent, connectors, your keys, every desktop) with tinted backgrounds on three of them | bento, 2 + 3 |
| Early, and in the open | The status paragraph, the source-available note, the repository card with a lock badge | two columns: copy and card |
| Footer | Wordmark, copyright and license line, GitHub link | single row |

Connector tiles are rendered from `src/connectors/data.ts` at build time: each entry is `{ name, slug, category, logo }`. `logo: null` produces a monogram in the category tint; the four first-party connectors use a Phosphor glyph instead because they are Gantry's own. Adding, removing or swapping a connector is one line, and a cleared logo file dropped into `public/connectors/` plus one path is a swap. No third-party mark is drawn by hand.

## 4. Technical approach

**Stack: Vite as a bundler only, TypeScript, vanilla DOM, three.js.** No framework. Session 2 planned a page with no build step; a three.js scene changes the arithmetic. Bundling gives a tree-shaken, hashed, lazily loaded scene chunk (144 KB gzipped against roughly 200 KB for the whole library plus addons through an import map), keeps the "no third-party scripts at runtime" rule intact by self-hosting everything, and makes progressive enhancement a one-line dynamic `import()`. The output is still a static folder and Pages still deploys on push.

```
website/
  index.html                 all content; three build-time injection points (poster, filters, grid) and icon slots
  vite.config.ts             the injection plugin: poster SVG, connector grid, Phosphor and Simple Icons glyphs
  src/main.ts                boot: OS detection, release lookup, filters, feed stagger, scene tier and lazy load
  src/releases.ts            GitHub releases API → button hrefs and the version line
  src/os.ts  src/theme.ts  src/tiers.ts  src/reveal.ts
  src/connectors/data.ts     the connector list (the only thing to edit for the showcase)
  src/connectors/markup.ts   grid and chip markup, pure functions used at build time
  src/scene/layout.js        rails, frames, stations, camera, shared by the scene and the poster
  src/scene/poster.js        projects the layout through the same camera into the static SVG
  src/scene/index.ts         renderer, lights, environment, tick loop, theme, frame-rate guard
  src/scene/structure.ts     rails and instanced frames, braces, feet, ground
  src/scene/modules.ts       connector modules, cables, the dashed network
  src/scene/carriage.ts      the agent: route, easing, wheels, lamp, link line
  src/scene/particles.ts     GPU rail flow (shader) and CPU pulses along paths
  src/scene/captions.ts  camera.ts  post.ts  palette.ts
  src/styles/tokens.css      the two palettes, three-state theme pattern
  src/styles/styles.css      layout and components
  public/fonts  public/favicon.svg  public/connectors/
  scripts/fetch-fonts.mjs    re-downloads the OFL font subsets
```

**The poster is the scene.** `layout.js` holds every coordinate; `poster.js` projects them through the same camera into an SVG that the build inlines into the HTML. It is the no-JavaScript rendering, the reduced-motion rendering, the low-power rendering, and the first frame everyone sees while the scene chunk loads. Changing the camera or a station changes both.

**Scene structure and budget.** Under 40 draw calls (rails, four instanced meshes, modules, carriage parts, one line set, two point clouds), under 120k triangles, at most 1,500 flow particles and 256 pulses, one bloom pass on the high tier only, pixel ratio capped at 2 (high) or 1.25 (medium). Lighting is one directional key, one fill, a hemisphere light and a room environment map; materials are standard PBR. Theme changes recolour materials and fog in place.

**Payload** (built): HTML 37.7 KB (8.9 KB gzipped, including the poster and the grid), CSS 13.3 KB (3.5 KB), page script 5.8 KB (2.5 KB), scene chunk 572 KB (144 KB), loaded lazily and only on capable devices; fonts about 52 KB.

## 5. Performance and graceful degradation

| Tier | Who gets it | What runs |
|------|-------------|-----------|
| `high` | fine pointer, more than 4 cores, more than 4 GB, WebGL2, no reduced-motion, no data-saver | full scene, bloom, up to 2× pixel ratio, 1,500 flow particles |
| `medium` | coarse pointer on a wide screen, or 4 cores / 4 GB | no bloom, 1.25× pixel ratio cap, 600 particles |
| `poster` | reduced motion, no WebGL2, data-saver, phones (coarse pointer under 768 px), or any load failure | the static SVG; the scene chunk is never downloaded |

- **Order of appearance.** HTML, CSS and the poster paint first; the page script (2.5 KB) wires the buttons; the scene chunk is requested on `requestIdleCallback` (1.5 s timeout) and cross-fades in over 700 ms only after its first frame. If the import fails or throws, the poster simply stays.
- **Runtime guard.** After a 1.2 s warm-up the scene measures two 4 s windows: below 45 fps on `high` it drops to `medium` (bloom off, fewer particles, lower pixel ratio); below 30 fps on `medium` it fades back to the poster and disposes itself. Targets: 60 fps on 2020-class laptops at the high tier, 60 fps on 4-core integrated hardware at medium, and a hard floor of 30 fps below which the scene yields.
- **Pausing.** The loop stops when the hero leaves the viewport and when the tab is hidden; a reduced-motion change while the page is open switches to the poster; an OS theme change recolours the scene live.
- **Preview switch.** `?theme=dark` or `?theme=light` stamps `data-theme` for screenshots and checks; there is no toggle in the UI.

## 6. Accessibility

- The whole scene wrapper is `aria-hidden` and `inert`; the canvas has `tabindex="-1"`; captions are decorative. Nothing in the scene can take focus or is needed to reach any content.
- Copy, buttons and links are ordinary DOM in reading order before the scene in the tab sequence; a skip link precedes the header; landmarks and headings are real.
- Download buttons carry `aria-disabled` with a visible "Coming soon" until a release exists; enabled buttons are plain links.
- Contrast: body and secondary text pass AA on both grounds; the primary button is dark text on the orange accent (about 8:1); the scrim keeps the hero copy legible over the scene.
- Reduced motion removes the scene, the feed stagger and smooth scrolling; the page is complete without JavaScript (grid rendered at build, buttons in their disabled state, poster inline).

## 7. Release integration (revised)

Session 2 planned a `releases.json` next to the installers, fetched by the page. Testing showed why that cannot work: a cross-origin fetch of `github.com/…/releases/latest/download/…` is blocked by CORS before the redirect to the asset host, even when the file exists. The GitHub REST API does send CORS headers, so:

- `src/releases.ts` calls `GET https://api.github.com/repos/oljodev/gantry/releases/latest` once per page load (6 s timeout). On success it enables the three buttons, points them at `https://github.com/oljodev/gantry/releases/latest/download/<stable asset name>` and shows "Version 0.3.1, released 2 November 2026" under them. The stable asset names `Gantry-macOS.dmg`, `Gantry-Windows-x64.exe` and `Gantry-Linux-x86_64.AppImage` are still uploaded by the release workflow; `releases.json` is no longer needed.
- Any failure (no release yet, private repository, the 60-requests-per-hour unauthenticated limit, offline) leaves the buttons in their "coming soon" state with nothing broken. The page is correct with JavaScript disabled for the same reason.
- The repository must be public for release assets to be downloadable at all (08). If it must stay private past the first release, the installers go to a public bucket (`dl.oljo.dev`) from the same workflow and the asset URLs change once.

## 8. At every release

`docs/dev/release.md` keeps the checklist: the OS list on the page matches what `release.yml` builds; stable-named assets were uploaded; the status section still says what is true; screenshots (when they exist) are from the current version. The version line updates itself.

## 9. Deliberately absent

A blog, a documentation subsite, a newsletter, analytics, a changelog page, a theme toggle, and any framework. Each can be added without touching the app.
