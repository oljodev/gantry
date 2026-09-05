// Downloads the latin woff2 subsets of the site's typefaces from Google Fonts (all OFL)
// so the page self-hosts every font. Run once: `node scripts/fetch-fonts.mjs`.
import { writeFile, mkdir } from 'node:fs/promises';
const UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36';
const families = [
  ['Barlow', 'wght@400;500', 'barlow'],
  ['Barlow Semi Condensed', 'wght@600', 'barlow-semi-condensed'],
  ['JetBrains Mono', 'wght@400;500', 'jetbrains-mono'],
];
await mkdir('public/fonts', { recursive: true });
const face = [];
for (const [family, axis, slug] of families) {
  const url = `https://fonts.googleapis.com/css2?family=${encodeURIComponent(family).replace(/%20/g, '+')}:${axis}&display=swap`;
  const css = await (await fetch(url, { headers: { 'User-Agent': UA } })).text();
  const blocks = css.split('@font-face').slice(1);
  for (const b of blocks) {
    if (!/\/\* latin \*\//.test(b)) continue;
    const weight = /font-weight:\s*(\d+)/.exec(b)[1];
    const src = /url\((https:[^)]+\.woff2)\)/.exec(b)[1];
    const range = /unicode-range:\s*([^;]+);/.exec(b)[1].trim();
    const file = `${slug}-${weight}-latin.woff2`;
    const buf = Buffer.from(await (await fetch(src)).arrayBuffer());
    await writeFile(`public/fonts/${file}`, buf);
    face.push(`@font-face{font-family:"${family}";font-style:normal;font-weight:${weight};font-display:swap;src:url("/fonts/${file}") format("woff2");unicode-range:${range};}`);
    console.log(file, buf.length, 'bytes');
  }
}
await writeFile('src/styles/fonts.css', face.join('\n') + '\n');
console.log('wrote src/styles/fonts.css');
