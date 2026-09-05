# 14 — Marketing site

## 1. Two folders, two deployments, no ambiguity

| Folder | Contents | Deployed as | Domain |
|--------|----------|-------------|--------|
| `website/` | the public marketing page: `index.html`, `styles.css`, `release.js`, `assets/` | Cloudflare Pages project `gantry-website`, root `website/`, no build step | `gantry.oljo.dev` (working assumption, see loose ends) |
| `client-metadata/` | the MCP OAuth Client ID Metadata Document (`client-metadata.json`) and a one-paragraph `index.html` explaining what the host is | Cloudflare Pages project `gantry-client-metadata`, root `client-metadata/`, no build step | `id.gantry.oljo.dev` |

The first session's `site/` folder is renamed to `client-metadata/` and the CIMD `client_id` becomes `https://id.gantry.oljo.dev/client-metadata.json` (03 §7 updated). The names now say what each folder is for, and the two are separate Pages projects on separate hosts, so a redesign of the marketing page cannot touch the file that is Gantry's OAuth identity, and a URL scheme change on the marketing side never breaks a connector. Both remain tiny static folders; that similarity is fine because nothing about them overlaps in path, host or purpose.

The existing rough draft of the page moves into `website/` unchanged; it is not in the repository yet (loose ends).

## 2. What the page is

Static HTML and CSS, one page, no framework, one optional script. Sections, as drafted: hero with the wordmark, tagline and three download buttons (macOS, Windows, Linux); "What is Gantry" with three feature blurbs; a status section; a footer. Additions worth making when the draft is moved in:

- a one-line license note ("source-available under the Functional Source License; converts to Apache 2.0 two years after each release") and, after the repository goes public, a link to it;
- light and dark palettes through `prefers-color-scheme`, using the app's tokens so screenshots and page agree;
- no analytics, no trackers, no third-party fonts or scripts; the page loads nothing from outside its own host except the release lookup below.

## 3. Download buttons and releases

**Recommendation: stable asset names plus GitHub's "latest release" download URLs, and one tiny JSON file for the version label. No backend, no manual href edits.**

- `release.yml` (tauri-action) already uploads versioned installers to the GitHub Release. Add one step that uploads copies under stable names: `Gantry-macOS.dmg`, `Gantry-Windows-x64.exe`, `Gantry-Linux-x86_64.AppImage` (and `.deb`/`.rpm` if listed). GitHub resolves `https://github.com/<owner>/gantry/releases/latest/download/<asset>` to the newest release, so the page's `href`s never change.
- The same step uploads `releases.json`: `{ "version": "0.3.1", "date": "2026-11-02", "assets": { "macos": "Gantry-macOS.dmg", "windows": "Gantry-Windows-x64.exe", "linux": "Gantry-Linux-x86_64.AppImage" } }`.
- `release.js` fetches `…/releases/latest/download/releases.json`; on success it enables the buttons and writes "v0.3.1 · 2 Nov 2026" under them; on any failure (no release yet, private repository, offline) the buttons stay in their "coming soon" state. The page is correct with JavaScript disabled: buttons disabled, nothing broken.

Two facts to plan around:

- **Release assets in a private repository are not publicly downloadable.** The buttons can only work once the repository is public, which the license is designed for (08). If the source must stay private past the first release, the fallback is to upload the installers to a public bucket (Cloudflare R2 behind `dl.gantry.oljo.dev`) from the same workflow step; the page's URLs change once, the mechanism does not.
- Tauri's updater manifest (`latest.json`) is also a release asset; the site could read it instead of `releases.json`, but it lists updater bundles and signatures, not the installers a visitor wants, so a purpose-built three-line file is clearer.

## 4. Keeping the page truthful

A short checklist in `docs/dev/release.md`, run at every release: version and date appear via `releases.json` automatically; OS list matches what `release.yml` actually builds; screenshots (when they exist) are from the current version; the status section says what is true today. The page has no other moving parts.

## 5. Deliberately absent

A blog, a documentation subsite, a newsletter, analytics, a changelog page (the GitHub Releases page is the changelog), and any build tooling. Each can be added without touching the app.
