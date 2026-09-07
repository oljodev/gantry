// Copies the bundled font files from the Fontsource packages into src/assets/fonts/.
// Run after `pnpm install` (`pnpm fonts`). Inter and JetBrains Mono are SIL OFL 1.1.
import { copyFileSync, mkdirSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const out = join(root, 'src/assets/fonts');
mkdirSync(out, { recursive: true });

const packages = [
  ['@fontsource-variable/inter', 'inter'],
  ['@fontsource-variable/jetbrains-mono', 'jetbrains-mono'],
];

for (const [pkg, prefix] of packages) {
  const dir = join(root, 'node_modules', pkg);
  const files = readdirSync(join(dir, 'files')).filter(
    (f) =>
      f.startsWith(`${prefix}-latin-wght-normal`) ||
      f.startsWith(`${prefix}-latin-ext-wght-normal`),
  );
  for (const f of files) copyFileSync(join(dir, 'files', f), join(out, f));
  copyFileSync(join(dir, 'LICENSE'), join(out, `${prefix}-LICENSE`));
  console.log(`${pkg}: ${files.length} files`);
}
