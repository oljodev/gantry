import type { ReactNode } from 'react';

import { SettingsRow } from '@/components/gantry/settings/SettingsRow';
import { Advanced } from '@/features/settings/Advanced';
import { Data } from '@/features/settings/Data';
import { General } from '@/features/settings/General';
import { Providers } from '@/features/settings/Providers';
import { isSection, type Section } from '@/features/settings/sections';
import { isTauri } from '@/lib/ipc/client';
import { useAppInfo } from '@/lib/ipc/hooks/useAppInfo';
import { useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { type Density, type ThemePref, useUiStore } from '@/lib/stores/uiStore';
import { Segmented } from '@/components/ui/radio-group';

const ARRIVES: Partial<Record<Section, string>> = { guard: 'M7' };

/** One settings section's rows, inside the dialog that titles it (15 A18). */
export function SettingsBody({ section }: { section: string }) {
  const id: Section = isSection(section) ? section : 'general';
  return (
    <>
      {id === 'general' && <General />}
      {id === 'appearance' && <Appearance />}
      {id === 'providers' && <Providers />}
      {id === 'data' && <Data />}
      {id === 'advanced' && <Advanced />}
      {id === 'about' && <About />}
      {ARRIVES[id] && (
        <p className="text-body text-fg-2">This section arrives with milestone {ARRIVES[id]}.</p>
      )}
    </>
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
