import { defineConfig } from 'vite';

/**
 * Builds the sandbox document as one page whose script and style are inlined afterwards by
 * `scripts/inline.mjs` into `dist/runtime.html` (docs/plan/13 §5: a sandboxed iframe cannot
 * reliably load external files, so everything ships inside the string the app injects).
 */
export default defineConfig({
  build: {
    target: 'safari15',
    minify: true,
    sourcemap: false,
    cssCodeSplit: false,
    modulePreload: false,
    assetsInlineLimit: 100_000_000,
    rollupOptions: {
      output: {
        inlineDynamicImports: true,
        entryFileNames: 'runtime.js',
        assetFileNames: 'runtime.[ext]',
      },
    },
  },
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
