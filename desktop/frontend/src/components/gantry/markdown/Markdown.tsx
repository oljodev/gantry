import { CopyIcon } from '@phosphor-icons/react';
import {
  type ComponentProps,
  createContext,
  memo,
  type ReactNode,
  useContext,
  useEffect,
  useState,
} from 'react';
import ReactMarkdown, { defaultUrlTransform } from 'react-markdown';
import remarkGfm from 'remark-gfm';

import { CodeBlock } from '@/components/gantry/markdown/CodeBlock';
import { MarkdownImage } from '@/components/gantry/markdown/MarkdownImage';
import { MermaidBlock } from '@/components/gantry/markdown/MermaidBlock';
import { copyText, openExternal } from '@/lib/clipboard';
import { splitBlocks } from '@/lib/markdown/blocks';
import { hasMath, loadMath, mathPlugins } from '@/lib/markdown/math';
import { cn } from '@/lib/utils';

/** The markdown of the block being rendered, so a table can offer its own source for Copy. */
const BlockSource = createContext('');

const FOOTNOTE = /^\[\^[^\]]+\]:/m;

/**
 * `data:` images are kept, because an answer may draw its own picture and the app is the only
 * thing that put it there. Every other URL goes through react-markdown's own filter, which
 * drops anything but http, https, mailto and tel.
 */
function urlTransform(url: string): string {
  return /^data:image\//i.test(url) ? url : defaultUrlTransform(url);
}

/**
 * Assistant markdown at `chat` size (15 §8). GitHub flavour plus maths, so tables, task lists,
 * footnotes and formulas all render; a ```mermaid fence becomes a drawn diagram. Headings map
 * to `title` and `ui` weights so an answer's headings never outrank the app's own (15 §4). The
 * text is split into top-level blocks and each block is memoised, so a streaming message
 * re-parses only its last block (05 §4).
 */
export function Markdown({ children, className }: { children: string; className?: string }) {
  // Footnotes are the one construct whose halves sit in different blocks, so a message that
  // defines one is parsed whole. Everything else keeps the per-block memoisation.
  const blocks = FOOTNOTE.test(children) ? [children] : splitBlocks(children);
  return (
    <div className={cn('prose-gantry selectable break-words text-chat text-fg', className)}>
      {blocks.map((block, i) => (
        <MarkdownBlock key={i} text={block} />
      ))}
    </div>
  );
}

const MarkdownBlock = memo(function MarkdownBlock({ text }: { text: string }) {
  const math = useMath(text);
  return (
    <BlockSource.Provider value={text}>
      <ReactMarkdown
        remarkPlugins={math ? [remarkGfm, ...math.remark] : [remarkGfm]}
        rehypePlugins={math ? math.rehype : []}
        urlTransform={urlTransform}
        components={components}
      >
        {text}
      </ReactMarkdown>
    </BlockSource.Provider>
  );
});

/**
 * KaTeX for a block that has a formula in it, and nothing at all for one that has not
 * (`lib/markdown/math.ts`). The first such block in a window renders once as plain text while
 * the plugins are fetched, and again when they arrive.
 */
function useMath(text: string) {
  const needed = hasMath(text);
  const [, setLoaded] = useState(mathPlugins() !== null);
  useEffect(() => {
    if (needed && mathPlugins() === null) void loadMath().then(() => setLoaded(true));
  }, [needed]);
  return needed ? mathPlugins() : null;
}

function Pre({ children }: ComponentProps<'pre'>) {
  const child = Array.isArray(children) ? children[0] : children;
  if (child && typeof child === 'object' && 'props' in child) {
    const props = (child as { props: { className?: string; children?: ReactNode } }).props;
    const language = /language-([\w-]+)/.exec(props.className ?? '')?.[1];
    const code = String(props.children ?? '').replace(/\n$/, '');
    if (language === 'mermaid') return <MermaidBlock code={code} />;
    return <CodeBlock code={code} language={language} />;
  }
  return <pre>{children}</pre>;
}

/**
 * A table, with the markdown that produced it one click away. The source is the block's own
 * text, which for a table is the table itself.
 */
function Table({ children }: { children?: ReactNode }) {
  const source = useContext(BlockSource);
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await copyText(source.trim());
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };
  return (
    <div className="group/table relative my-3">
      <button
        type="button"
        onClick={() => void copy()}
        aria-label="Copy table as markdown"
        title="Copy as markdown"
        className="absolute -top-1 right-0 z-10 flex h-(--control-sm) items-center gap-1 rounded-2 border border-line bg-raised px-1.5 text-meta text-fg-2 opacity-0 transition-opacity duration-(--dur-1) group-hover/table:opacity-100 focus-visible:opacity-100"
      >
        <CopyIcon className="size-3" />
        {copied ? 'Copied' : 'Copy'}
      </button>
      <div className="overflow-x-auto">
        <table className="w-full border-collapse text-ui">{children}</table>
      </div>
    </div>
  );
}

const components: ComponentProps<typeof ReactMarkdown>['components'] = {
  pre: Pre,
  code: ({ className, children, ...props }) => (
    <code
      className={cn('rounded-1 bg-inset px-1 py-0.5 font-mono text-[0.9em] text-fg', className)}
      {...props}
    >
      {children}
    </code>
  ),
  // A link never navigates the app: the URL is confirmed and handed to the system browser,
  // because an answer can quote a link that came from a page the model read (04 §2).
  a: ({ children, href }) => (
    <a
      href={href}
      onClick={(e) => {
        e.preventDefault();
        if (href) void openExternal(href);
      }}
      className="cursor-pointer text-fg underline decoration-line-strong underline-offset-2 hover:decoration-fg"
    >
      {children}
    </a>
  ),
  img: ({ src, alt, title }) => (
    <MarkdownImage src={typeof src === 'string' ? src : undefined} alt={alt} title={title} />
  ),
  h1: ({ children }) => <h2 className="mt-5 mb-2 text-title font-medium">{children}</h2>,
  h2: ({ children }) => <h2 className="mt-5 mb-2 text-title font-medium">{children}</h2>,
  h3: ({ children }) => <h3 className="mt-4 mb-1 text-chat font-medium">{children}</h3>,
  h4: ({ children }) => <h4 className="mt-4 mb-1 text-chat font-medium">{children}</h4>,
  p: ({ children }) => <p className="my-2 leading-6">{children}</p>,
  ul: ({ children }) => <ul className="my-2 list-disc pl-5">{children}</ul>,
  ol: ({ children, start }) => (
    <ol className="my-2 list-decimal pl-5" start={start}>
      {children}
    </ol>
  ),
  li: ({ children }) => <li className="my-0.5">{children}</li>,
  blockquote: ({ children }) => (
    <blockquote className="my-2 border-l-2 border-line-strong pl-3 text-fg-2">
      {children}
    </blockquote>
  ),
  hr: () => <hr className="my-4 border-line-subtle" />,
  table: Table,
  th: ({ children }) => (
    <th className="border-b border-line px-2 py-1 text-left font-medium">{children}</th>
  ),
  td: ({ children }) => (
    <td className="border-b border-line-subtle px-2 py-1 align-top">{children}</td>
  ),
};
