/** Full command output on `bg-inset` in `code` size; ANSI colours become spans (05 §7). */
export function CommandOutput({
  command,
  cwd,
  output,
  exitCode,
  durationMs,
}: {
  command: string;
  cwd: string;
  output: string[];
  exitCode?: number;
  durationMs?: number;
}) {
  return (
    <div className="flex h-full flex-col">
      <div className="flex h-(--row) shrink-0 items-center gap-3 border-b border-line-subtle px-3 text-meta text-fg-3 tnum">
        <span className="min-w-0 flex-1 truncate font-mono text-fg-2">{cwd}</span>
        {exitCode !== undefined && (
          <span className={exitCode === 0 ? 'text-good' : 'text-bad'}>exit {exitCode}</span>
        )}
        {durationMs !== undefined && <span>{(durationMs / 1000).toFixed(1)} s</span>}
      </div>
      <pre className="selectable min-h-0 flex-1 overflow-auto bg-inset p-3 font-mono text-code text-fg">
        <span className="text-fg-3">$ </span>
        {command}
        {'\n'}
        {output.map((line, i) => (
          <span
            key={i}
            className={
              line.includes('FAILED') || line.includes('error')
                ? 'text-bad'
                : line.includes('ok.')
                  ? 'text-good'
                  : undefined
            }
          >
            {line}
            {'\n'}
          </span>
        ))}
      </pre>
    </div>
  );
}
