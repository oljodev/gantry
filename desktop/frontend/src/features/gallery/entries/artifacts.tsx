import { useState } from 'react';

import { CONFORMANCE_HTML, PROBES } from '@gantry/artifact-runtime/src/conformance/probe';

import { Button } from '@/components/ui/button';
import type { ConsoleLine } from '@/features/artifacts/bridge';
import { CodeRenderer } from '@/features/artifacts/renderers/CodeRenderer';
import { MarkdownRenderer } from '@/features/artifacts/renderers/MarkdownRenderer';
import { SandboxHost } from '@/features/artifacts/renderers/SandboxHost';
import { SvgRenderer } from '@/features/artifacts/renderers/SvgRenderer';
import type { GalleryEntry } from '@/features/gallery/types';
import { cn } from '@/lib/utils';

const MARKDOWN = `# Auth expiry fix\n\nThe session cookie's \`max_age\` was set in **minutes** where the middleware expected seconds.\n\n- Fixed in \`auth.rs\`\n- 31 tests pass\n`;
const CODE = `fn expiry(minutes: u32) -> Duration {\n    Duration::from_secs(u64::from(minutes) * 60)\n}\n`;
const SVG = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 60" width="240" height="120"><rect x="4" y="4" width="112" height="52" rx="8" fill="lightsteelblue" stroke="royalblue"/><text x="60" y="36" text-anchor="middle" font-family="sans-serif" font-size="14" fill="navy">gantry</text></svg>`;
const MERMAID = `flowchart LR\n  A[User] --> B{Manual?}\n  B -- yes --> C[Ask]\n  B -- no --> D[Run]\n`;
const REACT = `import { useState } from "react";\nimport { Plus } from "lucide-react";\n\nexport default function App() {\n  const [n, setN] = useState(0);\n  return (\n    <div className="p-4 flex items-center gap-3">\n      <span className="text-lg font-medium">Count: {n}</span>\n      <button className="rounded border px-2 py-1 flex items-center gap-1" onClick={() => setN(n + 1)}>\n        <Plus size={14} /> one more\n      </button>\n    </div>\n  );\n}\n`;
const BROKEN = `export default function App() {\n  const items = undefined;\n  return <ul>{items.map((i) => <li>{i}</li>)}</ul>;\n}\n`;

function Frame({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex w-full flex-col gap-1">
      <div className="text-micro uppercase tracking-[0.04em] text-fg-3">{label}</div>
      <div className="h-64 overflow-auto rounded-3 border border-line bg-surface">{children}</div>
    </div>
  );
}

function Sandbox({
  type,
  content,
  language,
}: {
  type: 'html' | 'mermaid' | 'react';
  content: string;
  language?: string;
}) {
  const [lines, setLines] = useState<ConsoleLine[]>([]);
  const [status, setStatus] = useState<string>('rendering…');
  return (
    <div className="flex h-full flex-col">
      <div className="min-h-0 flex-1 overflow-auto">
        <SandboxHost
          type={type}
          content={content}
          language={language}
          onReport={(r) =>
            setStatus(r.status === 'ok' ? 'ready' : `error: ${r.errors[0]?.message ?? ''}`)
          }
          onConsole={(l) => setLines((ls) => [...ls, l].slice(-6))}
        />
      </div>
      <div className="border-t border-line-subtle px-2 py-1 font-mono text-mono text-fg-3">
        {status}
        {lines.map((l, i) => (
          <div key={i} className={cn(l.level === 'error' && 'text-bad')}>
            {l.text}
          </div>
        ))}
      </div>
    </div>
  );
}

/** The conformance artifact (13 §5): every probe must report `blocked`. */
function Conformance() {
  const [results, setResults] = useState<Record<string, string>>({});
  const [run, setRun] = useState(0);
  const done = PROBES.filter((p) => results[p] !== undefined).length;
  const open = PROBES.filter((p) => results[p]?.startsWith('OPEN'));
  return (
    <div className="flex w-full flex-col gap-2">
      <div className="flex items-center gap-3">
        <Button
          variant="secondary"
          size="sm"
          onClick={() => {
            setResults({});
            setRun((r) => r + 1);
          }}
        >
          Run sandbox conformance
        </Button>
        <span className={cn('text-ui', open.length > 0 ? 'text-bad' : 'text-fg-2')}>
          {run === 0
            ? 'Not run'
            : done < PROBES.length
              ? `${done} of ${PROBES.length} probes…`
              : open.length === 0
                ? `All ${PROBES.length} probes blocked`
                : `${open.length} OPEN: ${open.join(', ')}`}
        </span>
      </div>
      <ul className="grid grid-cols-2 gap-x-6 font-mono text-mono">
        {PROBES.map((p) => (
          <li
            key={p}
            className={cn(
              results[p]?.startsWith('OPEN') ? 'text-bad' : results[p] ? 'text-good' : 'text-fg-3',
            )}
          >
            {p}: {results[p] ?? '…'}
          </li>
        ))}
      </ul>
      {run > 0 && (
        <div className="h-40 overflow-hidden rounded-3 border border-line">
          <SandboxHost
            key={run}
            type="html"
            content={CONFORMANCE_HTML}
            onConsole={(l) => {
              const m = /^probe (.+?): (blocked|OPEN)(.*)$/.exec(l.text);
              if (m) setResults((r) => ({ ...r, [m[1]!]: `${m[2]}${m[3] ?? ''}`.trim() }));
            }}
          />
        </div>
      )}
    </div>
  );
}

function ArtifactsEntry() {
  return (
    <div className="flex w-full flex-col gap-4">
      <div className="grid grid-cols-2 gap-3">
        <Frame label="markdown">
          <MarkdownRenderer content={MARKDOWN} />
        </Frame>
        <Frame label="code · rust">
          <CodeRenderer content={CODE} language="rust" />
        </Frame>
        <Frame label="svg">
          <SvgRenderer content={SVG} title="gantry" />
        </Frame>
        <Frame label="mermaid · sandbox">
          <Sandbox type="mermaid" content={MERMAID} />
        </Frame>
        <Frame label="react · sandbox">
          <Sandbox type="react" content={REACT} language="tsx" />
        </Frame>
        <Frame label="react · runtime error">
          <Sandbox type="react" content={BROKEN} language="tsx" />
        </Frame>
      </div>
      <Conformance />
    </div>
  );
}

export const artifactEntries: GalleryEntry[] = [
  {
    id: 'artifacts',
    title: 'Artifact renderers · Sandbox conformance',
    group: 'Composites',
    render: () => <ArtifactsEntry />,
  },
];
