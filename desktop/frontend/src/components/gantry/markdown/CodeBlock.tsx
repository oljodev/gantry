import { CheckIcon, CopyIcon } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

/**
 * Code in messages: `bg-inset`, radius 8, `code` size, language label and Copy on hover
 * (15 §8). Highlighting arrives asynchronously through shiki; the plain block renders first.
 */
export function CodeBlock({
  code,
  language,
  className,
}: {
  code: string;
  language?: string;
  className?: string;
}) {
  const [html, setHtml] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let cancelled = false;
    if (!language) return;
    void import('shiki/bundle/web')
      .then(({ codeToHtml, bundledLanguages }) => {
        if (!(language in bundledLanguages)) return null;
        return codeToHtml(code, {
          lang: language,
          themes: { light: 'github-light', dark: 'github-dark' },
          defaultColor: false,
        });
      })
      .then((out) => {
        if (!cancelled && out) setHtml(out);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [code, language]);

  const copy = () => {
    void navigator.clipboard?.writeText(code).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  };

  return (
    <div
      className={cn(
        'group/code relative my-3 overflow-hidden rounded-3 border border-line-subtle bg-inset',
        className,
      )}
    >
      <div className="flex h-7 items-center justify-between border-b border-line-subtle px-3">
        <span className="text-micro uppercase tracking-[0.04em] text-fg-3">
          {language ?? 'text'}
        </span>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="Copy code"
          onClick={copy}
          className="opacity-0 transition-opacity duration-(--dur-1) group-hover/code:opacity-100 focus-visible:opacity-100"
        >
          {copied ? <CheckIcon className="text-good" /> : <CopyIcon />}
        </Button>
      </div>
      <div className="selectable overflow-x-auto p-3 text-code">
        {html ? (
          <div className="shiki-host" dangerouslySetInnerHTML={{ __html: html }} />
        ) : (
          <pre className="m-0 whitespace-pre font-mono text-fg">{code}</pre>
        )}
      </div>
    </div>
  );
}
