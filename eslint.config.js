import js from '@eslint/js';
import { defineConfig, globalIgnores } from 'eslint/config';
import reactHooks from 'eslint-plugin-react-hooks';
import reactRefresh from 'eslint-plugin-react-refresh';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default defineConfig([
  globalIgnores([
    'dist',
    'target',
    'node_modules',
    'src/routeTree.gen.ts',
    'src/bindings.ts',
    'src-tauri',
    'website',
    'client-metadata',
  ]),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: { ecmaVersion: 2023, globals: globals.browser },
    rules: {
      // Route files export `Route` next to their components by design.
      'react-refresh/only-export-components': ['error', { allowExportNames: ['Route'] }],
    },
  },
  {
    // Design tokens are the only place a colour or pixel size is written (docs/plan/15 §11).
    files: ['src/**/*.{ts,tsx}'],
    ignores: ['src/styles/**', 'src/fixtures/**', 'src/bindings.ts'],
    rules: {
      'no-restricted-syntax': [
        'error',
        {
          selector: 'Literal[value=/(^|[^\\w])#[0-9a-fA-F]{3,8}\\b|\\b(rgba?|hsla?|oklch)\\(/]',
          message:
            'Raw colours are not allowed in components; add a token to src/styles/tokens.css (docs/plan/15 §3).',
        },
        {
          selector:
            'TemplateElement[value.raw=/(^|[^\\w])#[0-9a-fA-F]{3,8}\\b|\\b(rgba?|hsla?|oklch)\\(/]',
          message:
            'Raw colours are not allowed in components; add a token to src/styles/tokens.css (docs/plan/15 §3).',
        },
        {
          selector: "JSXAttribute[name.name='className'] Literal[value=/\\[\\d+(\\.\\d+)?px\\]/]",
          message:
            'Arbitrary pixel sizes are not allowed in className; use a size token (docs/plan/15 §5).',
        },
      ],
    },
  },
  {
    // Route files export `Route`; shadcn primitives export their variant helpers.
    files: [
      'src/routes/**/*.tsx',
      'src/components/ui/**/*.tsx',
      'src/features/gallery/entries/**/*.tsx',
    ],
    rules: { 'react-refresh/only-export-components': 'off' },
  },
]);
