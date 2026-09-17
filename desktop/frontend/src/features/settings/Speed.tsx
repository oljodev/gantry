import type { ReactNode } from 'react';

import type { StartupPhase } from '@/bindings';
import { SettingsGroup } from '@/components/gantry/settings/SettingsRow';
import { Button } from '@/components/ui/button';
import { usePerfSnapshot, useStartupTiming } from '@/lib/ipc/hooks/perf';
import { reset, type Stat } from '@/lib/perf';

/**
 * Settings → Advanced → Speed: how long this window took to open, and what has been slow since
 * (docs/dev/performance.md).
 *
 * It measures the running app rather than a benchmark, because the numbers that matter are the
 * ones on the machine that is complaining: a login shell that takes a second, a database with
 * ten thousand messages in it, a screen that is drawing a hundred rows nobody scrolled to. All
 * of it is already being counted; this is where it is read.
 */
export function Speed() {
  const startup = useStartupTiming();
  const { boot, stats } = usePerfSnapshot();

  const offset = startup.data?.offsetMs ?? 0;
  const paint = boot.paint ?? boot.mount ?? 0;
  const groups: { title: string; hint: string; rows: Stat[] }[] = [
    {
      title: 'Windows',
      hint: 'From the click or the key to the frame it appeared in.',
      rows: stats.filter((s) => s.name.startsWith('dialog: ')),
    },
    {
      title: 'Answering',
      hint: 'The wait before the first word, and what each frame of it costs to draw.',
      rows: stats.filter((s) => !s.name.startsWith('dialog: ') && !s.name.startsWith('command: ')),
    },
    {
      title: 'Commands',
      hint: 'One round trip to the backend and back, including whatever it did there.',
      rows: stats.filter((s) => s.name.startsWith('command: ')),
    },
  ];

  return (
    <div className="flex flex-col gap-8">
      <SettingsGroup title="Opening this window">
        <Measure
          label="Before the window"
          hint="Starting the process, reading the database and the keyring, creating the webview."
          ms={offset}
        />
        {startup.data && (
          <Measure
            label="Backend startup"
            hint={<Phases phases={startup.data.timing.phases} />}
            ms={startup.data.timing.total_ms}
          />
        )}
        <Measure
          label="Document and scripts"
          hint="Loading and parsing the interface itself, before any of it runs."
          ms={boot.script ?? 0}
        />
        <Measure
          label="First screen"
          hint="Building and painting what you first see."
          ms={Math.max(0, paint - (boot.script ?? 0))}
        />
        <Measure label="Open, in total" ms={offset + paint} strong />
      </SettingsGroup>

      {groups.map((group) => (
        <Measures key={group.title} title={group.title} hint={group.hint} rows={group.rows} />
      ))}

      <div>
        <Button variant="ghost" onClick={() => reset()}>
          Forget these numbers
        </Button>
      </div>
    </div>
  );
}

/** One line of the startup timeline. */
function Measure({
  label,
  hint,
  ms,
  strong,
}: {
  label: string;
  hint?: ReactNode;
  ms: number;
  strong?: boolean;
}) {
  return (
    <div className="flex min-h-(--row) items-center justify-between gap-6 py-3">
      <div className="min-w-0">
        <div className="text-ui font-medium text-fg">{label}</div>
        {hint && <div className="text-meta text-fg-2">{hint}</div>}
      </div>
      <div
        className={
          strong
            ? 'shrink-0 text-title font-medium text-fg tabular-nums'
            : 'shrink-0 text-ui text-fg-2 tabular-nums'
        }
      >
        {round(ms)} ms
      </div>
    </div>
  );
}

/** The backend's phases, in the order they ran; the ones under a millisecond are not news. */
function Phases({ phases }: { phases: StartupPhase[] }) {
  const worth = phases.filter((p) => p.ms >= 1);
  if (worth.length === 0) return <>Every phase finished inside a millisecond.</>;
  return <>{worth.map((p) => `${p.name} ${p.ms} ms`).join(' · ')}</>;
}

/** A group of measurements: what, how often, how long usually, and how long at its worst. */
function Measures({ title, hint, rows }: { title: string; hint: string; rows: Stat[] }) {
  return (
    <section>
      <h2 className="mb-1 text-title font-medium text-fg">{title}</h2>
      <p className="mb-2 text-meta text-fg-2">{hint}</p>
      {rows.length === 0 ? (
        <p className="py-3 text-body text-fg-3">Nothing measured yet.</p>
      ) : (
        <table className="w-full text-meta tabular-nums">
          <thead>
            <tr className="text-left text-fg-3">
              <th className="py-1 font-normal">What</th>
              <th className="py-1 text-right font-normal">Times</th>
              <th className="py-1 text-right font-normal">Typical</th>
              <th className="py-1 text-right font-normal">Worst</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-line-subtle">
            {rows.slice(0, LIMIT).map((row) => (
              <tr key={row.name}>
                <td className="max-w-0 truncate py-1.5 pr-3 text-fg">{shorten(row.name)}</td>
                <td className="py-1.5 text-right text-fg-2">{row.count}</td>
                <td className="py-1.5 text-right text-fg-2">{round(row.total / row.count)} ms</td>
                <td className="py-1.5 text-right text-fg">{round(row.worst)} ms</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

/** Long enough to find the slow one, short enough to read at a glance. */
const LIMIT = 12;

function shorten(name: string): string {
  return name.replace(/^(dialog|command): /, '');
}

/** Whole milliseconds above ten, one decimal below: a frame is 8 ms and 0 ms says nothing. */
function round(ms: number): string {
  return ms >= 10 ? String(Math.round(ms)) : ms.toFixed(1);
}
