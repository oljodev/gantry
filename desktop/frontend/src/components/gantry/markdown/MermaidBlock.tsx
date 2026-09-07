import { useState } from 'react';

import { CodeBlock } from '@/components/gantry/markdown/CodeBlock';
import { Button } from '@/components/ui/button';
import { SandboxHost } from '@/features/artifacts/renderers/SandboxHost';
import { openExternal } from '@/lib/clipboard';

/**
 * A ```mermaid fence in an answer, drawn in the same sandboxed frame artifacts use (13 §5), so
 * a diagram in a chat and a diagram in an artifact are the same renderer. A diagram that fails
 * to parse falls back to its source, which is what the model actually wrote.
 */
export function MermaidBlock({ code }: { code: string }) {
  const [failed, setFailed] = useState(false);
  const [source, setSource] = useState(false);
  if (failed || source) {
    return (
      <div className="my-3">
        <CodeBlock code={code} language="mermaid" />
        {failed && <p className="mt-1 text-meta text-fg-3">This diagram could not be drawn.</p>}
        {!failed && (
          <Button variant="ghost" size="sm" onClick={() => setSource(false)}>
            Show the diagram
          </Button>
        )}
      </div>
    );
  }
  return (
    <div className="my-3 overflow-hidden rounded-3 border border-line-subtle">
      <SandboxHost
        type="mermaid"
        content={code}
        onReport={(report) => setFailed(report.status === 'error')}
        onOpenUrl={(url) => void openExternal(url)}
        className="w-full"
      />
      <div className="flex justify-end border-t border-line-subtle px-1 py-0.5">
        <Button variant="ghost" size="sm" onClick={() => setSource(true)}>
          Source
        </Button>
      </div>
    </div>
  );
}
