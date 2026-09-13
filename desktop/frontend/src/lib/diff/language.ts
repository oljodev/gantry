/**
 * The shiki language for a path (docs/plan/15 A19).
 *
 * Extensions only, and a short list of them: this decides whether a diff gets colour, and a
 * wrong guess costs nothing because shiki is asked for the language and skipped when it does
 * not have it. Anything not here renders as plain mono text, which is what the diff was before.
 */
const BY_EXTENSION: Record<string, string> = {
  bash: 'bash',
  c: 'c',
  cc: 'cpp',
  cjs: 'javascript',
  cpp: 'cpp',
  cs: 'csharp',
  css: 'css',
  fish: 'fish',
  go: 'go',
  h: 'c',
  hpp: 'cpp',
  htm: 'html',
  html: 'html',
  java: 'java',
  js: 'javascript',
  json: 'json',
  jsonc: 'jsonc',
  jsx: 'jsx',
  kt: 'kotlin',
  lua: 'lua',
  md: 'markdown',
  mdx: 'mdx',
  mjs: 'javascript',
  php: 'php',
  py: 'python',
  rb: 'ruby',
  rs: 'rust',
  scss: 'scss',
  sh: 'bash',
  sql: 'sql',
  svelte: 'svelte',
  svg: 'xml',
  swift: 'swift',
  toml: 'toml',
  ts: 'typescript',
  tsx: 'tsx',
  vue: 'vue',
  xml: 'xml',
  yaml: 'yaml',
  yml: 'yaml',
  zsh: 'bash',
};

/** Files whose whole name is the signal, because they have no extension. */
const BY_NAME: Record<string, string> = {
  Dockerfile: 'docker',
  Makefile: 'make',
  '.gitignore': 'ini',
  '.env': 'ini',
};

export function languageForPath(path: string | undefined): string | undefined {
  if (!path) return undefined;
  const name = path.split(/[\\/]/).pop() ?? '';
  if (name in BY_NAME) return BY_NAME[name];
  const ext = name.includes('.') ? name.slice(name.lastIndexOf('.') + 1).toLowerCase() : '';
  return BY_EXTENSION[ext];
}
