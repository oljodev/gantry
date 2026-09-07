// Copies the latin variable subsets of Geist and Geist Mono (SIL OFL 1.1, no reserved font name) out of the
// Fontsource packages into src/assets/fonts/, where the Astro Fonts API picks them up. Run after `pnpm install`.
import { copyFileSync, mkdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const out = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'assets', 'fonts');
mkdirSync(out, { recursive: true });
const files = [
  ['@fontsource-variable/geist/files/geist-latin-wght-normal.woff2', 'geist-latin-wght-normal.woff2'],
  ['@fontsource-variable/geist-mono/files/geist-mono-latin-wght-normal.woff2', 'geist-mono-latin-wght-normal.woff2'],
  ['@fontsource-variable/geist/LICENSE', 'LICENSE-geist.txt'],
];
for (const [from, to] of files) {
  copyFileSync(require.resolve(from), join(out, to));
  console.log(`${to}`);
}
