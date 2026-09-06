import type { ReactNode } from 'react';

import { isSection, SECTIONS, type Section } from '@/features/settings/sections';
import { isTauri } from '@/lib/ipc/client';
import { useAppInfo } from '@/lib/ipc/hooks/useAppInfo';
import { type Density, type ThemePref, useUiStore } from '@/lib/stores/uiStore';
import { cn } from '@/lib/utils';

const ARRIVES: Partial<Record<Section, string>> = {
  general: 'M2',
  providers: 'M1',
  guard: 'M7',
  connectors: 'M9',
  skills: 'M12',
  memory: 'M12',
  data: 'M2',
  advanced: 'M2',
};

export function SettingsSection({ section }: { section: string }) {
  const id: Section = isSection(section) ? section : 'appearance';
  const label = SECTIONS.find(([s]) => s === id)?.[1] ?? id;

  return (
    <div className="mx-auto w-full max-w-(--measure) px-6 py-8">
      <h1 className="text-page font-semibold text-fg">{label}</h1>
      <div className="mt-6">
        {id === 'appearance' && <Appearance />}
        {id === 'about' && <About />}
        {ARRIVES[id] && (
          <p className="text-body text-fg-2">This section arrives with milestone {ARRIVES[id]}.</p>
        )}
      </div>
    </div>
  );
}

function Appearance() {
  const theme = useUiStore((s) => s.theme);
  const density = useUiStore((s) => s.density);
  const setTheme = useUiStore((s) => s.setTheme);
  const setDensity = useUiStore((s) => s.setDensity);
  return (
    <div className="divide-y divide-line-subtle">
      <SettingsRow label="Theme" hint="Follow the system, or pick one.">
        <Segmented<ThemePref>
          value={theme}
          onChange={setTheme}
          options={[
            ['system', 'System'],
            ['light', 'Light'],
            ['dark', 'Dark'],
          ]}
        />
      </SettingsRow>
      <SettingsRow label="Density" hint="Row heights and gutters in lists and settings.">
        <Segmented<Density>
          value={density}
          onChange={setDensity}
          options={[
            ['comfortable', 'Comfortable'],
            ['compact', 'Compact'],
          ]}
        />
      </SettingsRow>
    </div>
  );
}

function About() {
  const info = useAppInfo();
  if (!isTauri()) {
    return (
      <p className="text-body text-fg-2">
        Not running inside the Gantry window, so there is no backend to ask. Version and directories
        show here in the app.
      </p>
    );
  }
  if (info.isPending) return <p className="text-body text-fg-3">Loading…</p>;
  if (info.isError)
    return <p className="text-body text-bad">Could not read app info: {String(info.error)}</p>;
  const d = info.data;
  return (
    <dl className="divide-y divide-line-subtle">
      <Fact label="Version">
        {d.version}
        {d.debug ? ' (debug)' : ''}
      </Fact>
      <Fact label="Platform">
        {d.os} · {d.arch}
      </Fact>
      <Fact label="Data directory">
        <code className="selectable text-mono">{d.data_dir}</code>
      </Fact>
      <Fact label="Logs">
        <code className="selectable text-mono">{d.log_dir}</code>
      </Fact>
      <Fact label="Licence">Functional Source License, FSL-1.1-ALv2</Fact>
    </dl>
  );
}

function SettingsRow({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex min-h-(--row) items-center justify-between gap-6 py-3">
      <div className="min-w-0">
        <div className="text-ui font-medium text-fg">{label}</div>
        {hint && <div className="text-meta text-fg-2">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex min-h-(--row) items-center justify-between gap-6 py-3">
      <dt className="text-ui font-medium text-fg">{label}</dt>
      <dd className="text-ui text-fg-2">{children}</dd>
    </div>
  );
}

/** A segmented control from plain buttons; the reshaped RadioGroup replaces it in M0b. */
function Segmented<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (v: T) => void;
  options: readonly (readonly [T, string])[];
}) {
  return (
    <div role="radiogroup" className="inline-flex rounded-2 border border-line bg-raised p-[2px]">
      {options.map(([v, label]) => (
        <button
          key={v}
          type="button"
          role="radio"
          aria-checked={value === v}
          onClick={() => onChange(v)}
          className={cn(
            'h-(--control-sm) rounded-[4px] px-3 text-ui transition-colors duration-(--dur-1)',
            value === v ? 'bg-selected text-fg' : 'text-fg-2 hover:text-fg',
          )}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
