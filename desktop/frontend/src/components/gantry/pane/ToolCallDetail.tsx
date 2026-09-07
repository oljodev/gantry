/** Raw arguments and result of a tool call, as JSON (05 §1), with the outcome line. */
export function ToolCallDetail({
  title,
  args,
  result,
  isError,
  status,
  durationMs,
}: {
  title: string;
  args: unknown;
  result?: unknown;
  isError?: boolean;
  status?: string;
  durationMs?: number;
}) {
  const outcome = [
    status,
    isError ? 'error result' : undefined,
    durationMs !== undefined ? `${durationMs} ms` : undefined,
  ]
    .filter(Boolean)
    .join(' · ');
  return (
    <div className="flex flex-col gap-4 p-3">
      <div className="flex items-baseline justify-between gap-3">
        <div className="text-ui font-medium text-fg">{title}</div>
        {outcome && <div className="text-meta text-fg-3 tnum">{outcome}</div>}
      </div>
      <Section label="Arguments" value={args} />
      {result !== undefined && <Section label="Result" value={unwrapResult(result)} />}
    </div>
  );
}

/** A single JSON result part reads better as its value than as a one-element list. */
function unwrapResult(result: unknown): unknown {
  if (Array.isArray(result) && result.length === 1) {
    const only = result[0] as { kind?: string; json?: unknown; text?: unknown };
    if (only && only.kind === 'json') return only.json;
    if (only && only.kind === 'text') return only.text;
  }
  return result;
}

function Section({ label, value }: { label: string; value: unknown }) {
  return (
    <div className="flex flex-col gap-1">
      <div className="text-micro font-medium uppercase tracking-[0.04em] text-fg-3">{label}</div>
      <pre className="selectable overflow-auto rounded-2 border border-line-subtle bg-inset p-3 font-mono text-mono text-fg">
        {JSON.stringify(value, null, 2)}
      </pre>
    </div>
  );
}
