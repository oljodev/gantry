import type { ReactNode } from 'react';

import { SettingsRow } from '@/components/gantry/settings/SettingsRow';
import { Advanced } from '@/features/settings/Advanced';
import { Data } from '@/features/settings/Data';
import { General } from '@/features/settings/General';
import { GuardDecisions, JudgeModelRow } from '@/features/settings/GuardDecisions';
import { Guardrails } from '@/features/settings/Guardrails';
import { Providers } from '@/features/settings/Providers';
import { isSection, type Section } from '@/features/settings/sections';
import { isTauri } from '@/lib/ipc/client';
import { useAppInfo } from '@/lib/ipc/hooks/useAppInfo';
import { useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { type Density, type ThemePref, useUiStore } from '@/lib/stores/uiStore';
import { Segmented } from '@/components/ui/radio-group';

/** One settings section's rows, inside the dialog that titles it (15 A18). */
export function SettingsBody({ section }: { section: string }) {
  const id: Section = isSection(section) ? section : 'general';
  return (
    <>
      {id === 'general' && <General />}
      {id === 'appearance' && <Appearance />}
      {id === 'providers' && <Providers />}
      {id === 'guard' && <Guard />}
      {id === 'data' && <Data />}
      {id === 'advanced' && <Advanced />}
      {id === 'about' && <About />}
    </>
  );
}

/**
 * Settings → Guard & guardrails (11 §2): the floor of 04 §5, then the record of what the guard
 * of 04 §6 has decided on the user's behalf.
 */
function Guard() {
  return (
    <div className="flex flex-col gap-8">
      <Guardrails />
      <section>
        <h2 className="mb-1 text-title font-medium text-fg">Guard</h2>
        <p className="mb-3 text-body text-fg-2">
          In Auto mode, a small fast model of the provider your chat already uses decides each call
          that changes something, so a long task runs without interrupting you. It cannot reach past
          the guardrails above, and when it cannot decide — a timeout, an answer it could not give —
          the question comes to you instead. Turn it off per chat with the mode chip, or for new
          chats in General.
        </p>
        <div className="mb-4 divide-y divide-line-subtle border-y border-line-subtle">
          <JudgeModelRow />
        </div>
        <p className="mb-4 text-meta text-fg-3">
          Marking a decision right or wrong is kept with it, for tuning the guard's instructions in
          a later release. It does not change what the guard does today.
        </p>
        <GuardDecisions />
      </section>
    </div>
  );
}

/** The UI store applies the change at once; the settings table is the truth it mirrors (11 §3). */
function Appearance() {
  const theme = useUiStore((s) => s.theme);
  const density = useUiStore((s) => s.density);
  const setThemeLocal = useUiStore((s) => s.setTheme);
  const setDensityLocal = useUiStore((s) => s.setDensity);
  const settings = useSettings();
  const update = useUpdateSettings();
  const persist = (next: { theme: ThemePref; density: Density }) => {
    if (!isTauri()) return;
    update.mutate({ appearance: { ...(settings.data?.appearance ?? {}), ...next } });
  };
  const setTheme = (t: ThemePref) => {
    setThemeLocal(t);
    persist({ theme: t, density });
  };
  const setDensity = (d: Density) => {
    setDensityLocal(d);
    persist({ theme, density: d });
  };
  return (
    <div className="divide-y divide-line-subtle">
      <SettingsRow label="Theme" hint="Follow the system, or pick one.">
        <Segmented<ThemePref>
          aria-label="Theme"
          value={theme}
          onValueChange={setTheme}
          options={[
            ['system', 'System'],
            ['light', 'Light'],
            ['dark', 'Dark'],
          ]}
        />
      </SettingsRow>
      <SettingsRow label="Density" hint="Row heights and gutters in lists and settings.">
        <Segmented<Density>
          aria-label="Density"
          value={density}
          onValueChange={setDensity}
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

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex min-h-(--row) items-center justify-between gap-6 py-3">
      <dt className="text-ui font-medium text-fg">{label}</dt>
      <dd className="text-ui text-fg-2">{children}</dd>
    </div>
  );
}
