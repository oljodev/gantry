import type { ComponentProps, ReactNode } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

import { CodeBlock } from '@/components/gantry/markdown/CodeBlock';
import { cn } from '@/lib/utils';

/**
 * Assistant markdown at `chat` size. Headings map to `title` and `ui` weights so an answer's
 * headings never outrank the app's own (15 §4). Block-level memoisation arrives with M1.
 */
export function Markdown({ children, className }: { children: string; className?: string }) {
  return (
    <div className={cn('prose-gantry selectable text-chat text-fg', className)}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {children}
      </ReactMarkdown>
    </div>
  );
}

function Pre({ children }: ComponentProps<'pre'>) {
  const child = Array.isArray(children) ? children[0] : children;
  if (child && typeof child === 'object' && 'props' in child) {
    const props = (child as { props: { className?: string; children?: ReactNode } }).props;
    const language = /language-([\w-]+)/.exec(props.className ?? '')?.[1];
    const code = String(props.children ?? '').replace(/\n$/, '');
    return <CodeBlock code={code} language={language} />;
  }
  return <pre>{children}</pre>;
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
  a: ({ children, ...props }) => (
    <a
      className="text-fg underline decoration-line-strong underline-offset-2 hover:decoration-fg"
      target="_blank"
      rel="noreferrer"
      {...props}
    >
      {children}
    </a>
  ),
  h1: ({ children }) => <h2 className="mt-5 mb-2 text-title font-medium">{children}</h2>,
  h2: ({ children }) => <h2 className="mt-5 mb-2 text-title font-medium">{children}</h2>,
  h3: ({ children }) => <h3 className="mt-4 mb-1 text-chat font-medium">{children}</h3>,
  h4: ({ children }) => <h4 className="mt-4 mb-1 text-chat font-medium">{children}</h4>,
  p: ({ children }) => <p className="my-2 leading-6">{children}</p>,
  ul: ({ children }) => <ul className="my-2 list-disc pl-5">{children}</ul>,
  ol: ({ children }) => <ol className="my-2 list-decimal pl-5">{children}</ol>,
  li: ({ children }) => <li className="my-0.5">{children}</li>,
  blockquote: ({ children }) => (
    <blockquote className="my-2 border-l-2 border-line-strong pl-3 text-fg-2">
      {children}
    </blockquote>
  ),
  hr: () => <hr className="my-4 border-line-subtle" />,
  table: ({ children }) => (
    <div className="my-3 overflow-x-auto">
      <table className="w-full border-collapse text-ui">{children}</table>
    </div>
  ),
  th: ({ children }) => (
    <th className="border-b border-line px-2 py-1 text-left font-medium">{children}</th>
  ),
  td: ({ children }) => (
    <td className="border-b border-line-subtle px-2 py-1 align-top">{children}</td>
  ),
};
