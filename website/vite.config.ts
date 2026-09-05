import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { defineConfig, type Plugin } from 'vite';
import { siGithub } from 'simple-icons';
import { connectors, categories } from './src/connectors/data.ts';
import { renderConnectorGrid, renderFilterChips } from './src/connectors/markup.ts';
import { renderPoster } from './src/scene/poster.js';

const require = createRequire(import.meta.url);

/** Phosphor's regular-weight glyphs, inlined as currentColor SVG. Icons come from the library, never hand-drawn. */
function phosphor(name: string): string {
  const svg = readFileSync(require.resolve(`@phosphor-icons/core/assets/regular/${name}.svg`), 'utf8');
  return svg.replace('<svg', '<svg aria-hidden="true" focusable="false" fill="currentColor"');
}
function brand(path: string): string {
  return `<svg aria-hidden="true" focusable="false" viewBox="0 0 24 24" fill="currentColor"><path d="${path}"/></svg>`;
}

const icons: Record<string, string> = {
  github: brand(siGithub.path),
  download: phosphor('download-simple'),
  lock: phosphor('lock-simple'),
  arrow: phosphor('arrow-up-right'),
  folder: phosphor('folder-simple'),
  code: phosphor('code'),
  terminal: phosphor('terminal-window'),
  globe: phosphor('globe-simple'),
};

/** Build-time injection: the poster SVG (a projection of the real scene layout), the connector grid and the icons.
 *  Everything a visitor needs is in the HTML before any script runs. */
function inject(): Plugin {
  return {
    name: 'gantry-inject',
    transformIndexHtml(html) {
      return html
        .replace('<!-- poster -->', renderPoster())
        .replace('<!-- connector-filters -->', renderFilterChips(categories))
        .replace('<!-- connector-grid -->', renderConnectorGrid(connectors, icons))
        .replace(/<!-- icon:([a-z-]+) -->/g, (_m, name: string) => icons[name] ?? '');
    },
  };
}

export default defineConfig({
  plugins: [inject()],
  build: {
    target: 'es2022',
    modulePreload: { polyfill: false },
    sourcemap: false,
    chunkSizeWarningLimit: 650, // the scene chunk is three.js; it is loaded lazily and only on capable devices
  },
});
