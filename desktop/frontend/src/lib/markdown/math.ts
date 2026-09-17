import type { ComponentProps } from 'react';
import type ReactMarkdown from 'react-markdown';

type Plugins = NonNullable<ComponentProps<typeof ReactMarkdown>['remarkPlugins']>;

/**
 * KaTeX, fetched by the first answer that has a formula in it (docs/dev/performance.md).
 *
 * The maths renderer and its stylesheet are 300 kB, and most conversations have no maths in
 * them at all. They used to be part of the markdown renderer's own chunk, which every chat
 * loads. Now a block is scanned first — a cheap regular expression over text that is being
 * parsed anyway — and the plugins arrive only where they are needed.
 *
 * The cost of that is one re-render: the first formula in a window is painted as its own source
 * for as long as the fetch takes, then again as maths. Every one after it is immediate.
 */

/** `$…$`, `$$…$$`, `\(…\)` and `\[…\]` — what `remark-math` itself recognises. */
const MATH = /\$\$|\\\(|\\\[|\$[^$\n]+\$/;

export function hasMath(text: string): boolean {
  return MATH.test(text);
}

let plugins: { remark: Plugins; rehype: Plugins } | null = null;
let loading: Promise<void> | null = null;

/** The plugins, or `null` until they have arrived. */
export function mathPlugins(): { remark: Plugins; rehype: Plugins } | null {
  return plugins;
}

/** Fetches them once; every caller waits on the same promise. */
export function loadMath(): Promise<void> {
  loading ??= Promise.all([
    import('remark-math'),
    import('rehype-katex'),
    import('katex/dist/katex.min.css'),
  ]).then(([remarkMath, rehypeKatex]) => {
    plugins = {
      remark: [remarkMath.default],
      rehype: [[rehypeKatex.default, { throwOnError: false, output: 'html' }]],
    };
  });
  return loading;
}
