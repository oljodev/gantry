import { useEffect, useState } from 'react';

import { cn } from '@/lib/utils';

/**
 * Highlighted source with line numbers (13 §3): shiki when it knows the language, plain
 * text first and while streaming. Also the Source view of every other type.
 */
export function CodeRenderer({
  content,
  language,
  streaming,
  className,
}: {
  content: string;
  language: string;
  streaming?: boolean;
  className?: string;
}) {
  const [html, setHtml] = useState<string | null>(null);
  useEffect(() => {
    if (streaming) return;
    let cancelled = false;
    void import('shiki/bundle/web')
      .then(({ codeToHtml, bundledLanguages }) => {
        if (!(language in bundledLanguages)) return null;
        return codeToHtml(content, {
          lang: language,
          themes: { light: 'github-light', dark: 'github-dark' },
          defaultColor: false,
        });
      })
      .then((out) => {
        if (!cancelled) setHtml(out);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [content, language, streaming]);

  const lines = content.split('\n');
  return (
    <div className={cn('selectable flex min-h-full text-code', className)}>
      <div
        aria-hidden
        className="shrink-0 select-none border-r border-line-subtle bg-inset px-2 py-3 text-right font-mono text-fg-3 tnum"
      >
        {lines.map((_, i) => (
          <div key={i}>{i + 1}</div>
        ))}
      </div>
      <div className="min-w-0 flex-1 overflow-x-auto px-3 py-3">
        {html && !streaming ? (
          <div className="shiki-host" dangerouslySetInnerHTML={{ __html: html }} />
        ) : (
          <pre className="m-0 whitespace-pre font-mono text-fg">{content}</pre>
        )}
      </div>
    </div>
  );
}
