/** Raw arguments and result of a tool call, as JSON (05 §1). */
export function ToolCallDetail({
  title,
  args,
  result,
}: {
  title: string;
  args: unknown;
  result?: unknown;
}) {
  return (
    <div className="flex flex-col gap-4 p-3">
      <div className="text-ui font-medium text-fg">{title}</div>
      <Section label="Arguments" value={args} />
      {result !== undefined && <Section label="Result" value={result} />}
    </div>
  );
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
