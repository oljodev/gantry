// Renders model output as Markdown. Agents emit headings, lists, tables and
// fenced code constantly; showing that as pre-wrapped plain text made long
// answers hard to read.
//
// Raw HTML is deliberately NOT enabled (react-markdown's default): model
// output is untrusted text, and this keeps it from injecting markup.

import ReactMarkdown, { type Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { normalizeMarkdown } from '../lib/markdown'

const COMPONENTS: Components = {
  h1: (props) => <h1 className="mt-4 mb-2 text-base font-semibold text-zinc-100" {...props} />,
  h2: (props) => <h2 className="mt-4 mb-2 text-sm font-semibold text-zinc-100" {...props} />,
  h3: (props) => <h3 className="mt-3 mb-1.5 text-sm font-semibold text-zinc-200" {...props} />,
  h4: (props) => <h4 className="mt-3 mb-1.5 text-sm font-semibold text-zinc-300" {...props} />,
  p: (props) => <p className="my-2 leading-relaxed" {...props} />,
  ul: (props) => <ul className="my-2 list-disc space-y-1 pl-5" {...props} />,
  ol: (props) => <ol className="my-2 list-decimal space-y-1 pl-5" {...props} />,
  li: (props) => <li className="leading-relaxed" {...props} />,
  strong: (props) => <strong className="font-semibold text-zinc-100" {...props} />,
  em: (props) => <em className="italic" {...props} />,
  a: (props) => (
    <a
      className="text-sky-300 underline underline-offset-2 hover:text-sky-200"
      target="_blank"
      rel="noreferrer"
      {...props}
    />
  ),
  hr: () => <hr className="my-4 border-zinc-800" />,
  blockquote: (props) => (
    <blockquote className="my-2 border-l-2 border-zinc-700 pl-3 text-zinc-400" {...props} />
  ),
  // Inline code vs. fenced blocks: react-markdown passes fenced code through
  // <pre><code>, so <pre> owns the block styling and <code> only styles the
  // inline case (it must stay transparent inside a <pre>).
  pre: (props) => (
    <pre
      className="my-2 overflow-x-auto rounded-md bg-code px-3 py-2 font-mono text-xs leading-relaxed"
      {...props}
    />
  ),
  code: ({ className, ...props }) => {
    const fenced = typeof className === 'string' && className.startsWith('language-')
    return fenced ? (
      <code className={className} {...props} />
    ) : (
      <code
        className="rounded bg-zinc-800/70 px-1 py-0.5 font-mono text-[0.85em] text-amber-200"
        {...props}
      />
    )
  },
  table: (props) => (
    <div className="my-2 overflow-x-auto">
      <table className="w-full border-collapse text-xs" {...props} />
    </div>
  ),
  th: (props) => (
    <th
      className="border border-zinc-800 bg-zinc-900/60 px-2 py-1 text-left font-semibold"
      {...props}
    />
  ),
  td: (props) => <td className="border border-zinc-800 px-2 py-1 align-top" {...props} />,
}

export function Markdown({ children }: { children: string }) {
  return (
    // [&>*:first-child]:mt-0 — block margins would otherwise push the first
    // element away from the container's own padding.
    <div className="text-sm text-zinc-200 [&>*:first-child]:mt-0 [&>*:last-child]:mb-0">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={COMPONENTS}>
        {normalizeMarkdown(children)}
      </ReactMarkdown>
    </div>
  )
}
