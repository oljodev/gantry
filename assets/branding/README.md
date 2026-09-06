# Branding

Identity sources. `app-icon/icon.svg` is the master app icon (design: `docs/plan/15-app-design.md`
§13); `src-tauri/icons/` is generated from it with `pnpm tauri icon assets/branding/app-icon/icon-1024.png`
after rendering the PNG (`rsvg-convert -w 1024 -h 1024 icon.svg > icon-1024.png`). The mark's
geometry is the same as the website's `Logo.astro`.
