// @ts-check
import { defineConfig, fontProviders } from 'astro/config';
import react from '@astrojs/react';
import starlight from '@astrojs/starlight';
import tailwindcss from '@tailwindcss/vite';

const LATIN = /** @type {[string, ...string[]]} */ (['U+0000-00FF', 'U+0131', 'U+0152-0153', 'U+02BB-02BC', 'U+02C6', 'U+02DA', 'U+02DC', 'U+0304', 'U+0308', 'U+0329', 'U+2000-206F', 'U+20AC', 'U+2122', 'U+2191', 'U+2193', 'U+2212', 'U+2215', 'U+FEFF', 'U+FFFD']);

// https://astro.build/config
export default defineConfig({
  site: 'https://oljo.dev',
  output: 'static',
  trailingSlash: 'always',
  compressHTML: true,
  build: { format: 'directory', inlineStylesheets: 'never' },
  // The one Cloudflare entry became the two connectors the app actually ships (docs/plan/17-connector-catalog.md §7).
  redirects: { '/connectors/cloudflare/': '/connectors/cloudflare-bindings/' },
  vite: { plugins: [tailwindcss()], build: { assetsInlineLimit: 0 } },
  fonts: [
    {
      provider: fontProviders.local(),
      name: 'Geist',
      cssVariable: '--font-geist',
      fallbacks: ['system-ui', 'sans-serif'],
      options: {
        variants: [{ src: ['./src/assets/fonts/geist-latin-wght-normal.woff2'], weight: '100 900', style: 'normal', display: 'swap', unicodeRange: LATIN }],
      },
    },
    {
      provider: fontProviders.local(),
      name: 'Geist Mono',
      cssVariable: '--font-geist-mono',
      fallbacks: ['ui-monospace', 'monospace'],
      options: {
        variants: [{ src: ['./src/assets/fonts/geist-mono-latin-wght-normal.woff2'], weight: '100 900', style: 'normal', display: 'swap', unicodeRange: LATIN }],
      },
    },
  ],
  // Starlight registers MDX, the sitemap and Expressive Code itself, in the order they need.
  integrations: [
    react(),
    starlight({
      title: 'Gantry docs',
      disable404Route: true,
      customCss: ['./src/styles/docs.css'],
      components: {
        ThemeProvider: './src/components/docs/ThemeProvider.astro',
        ThemeSelect: './src/components/docs/Empty.astro',
        Head: './src/components/docs/Head.astro',
        Header: './src/components/docs/Header.astro',
        Banner: './src/components/docs/Banner.astro',
        MobileMenuFooter: './src/components/docs/MobileMenuFooter.astro',
      },
      sidebar: [
        { label: 'Start here', items: [{ autogenerate: { directory: 'docs/start' } }] },
        { label: 'Connectors', items: [{ autogenerate: { directory: 'docs/connectors' } }] },
        { label: 'Reference', items: [{ autogenerate: { directory: 'docs/reference' } }] },
      ],
      expressiveCode: { themes: ['github-dark'] },
    }),
  ],
});
