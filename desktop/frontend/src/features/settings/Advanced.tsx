import { useState } from 'react';

import type { AdvancedSettings } from '@/bindings';
import { SettingsGroup, SettingsRow } from '@/components/gantry/settings/SettingsRow';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { isTauri } from '@/lib/ipc/client';
import { useSecretStoreStatus, useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { advancedDefaults } from '@/lib/settingsDefaults';

/**
 * Settings → Advanced (11 §2): output cap, developer mode and where the master key lives. Tool
 * limits and log level join with the tool loop (M3).
 */
export function Advanced() {
  const settings = useSettings();
  const update = useUpdateSettings();
  const store = useSecretStoreStatus();
  if (!isTauri()) {
    return (
      <p className="text-body text-fg-2">
        Not running inside the Gantry window, so there are no settings to edit. They show here in
        the app.
      </p>
    );
  }
  if (!settings.data) return <p className="text-body text-fg-3">Loading…</p>;
  const advanced = advancedDefaults(settings.data);
  const patch = (next: Partial<AdvancedSettings>) =>
    update.mutate({ advanced: { ...advanced, ...next } });

  return (
    <div className="flex flex-col gap-8">
      <SettingsGroup title="Model requests">
        <SettingsRow
          label="Maximum reply length"
          hint="Tokens per assistant message; models with a lower limit use theirs."
        >
          <TokenInput
            value={advanced.max_output_tokens}
            onCommit={(v) => patch({ max_output_tokens: v })}
          />
        </SettingsRow>
      </SettingsGroup>
      <SettingsGroup title="Developer">
        <SettingsRow
          label="Developer mode"
          hint="Adds “View system prompt” to a chat's menu, showing the frozen prompt and the notes appended since."
        >
          <Switch
            aria-label="Developer mode"
            checked={advanced.developer_mode}
            onCheckedChange={(v) => patch({ developer_mode: v })}
          />
        </SettingsRow>
        <SettingsRow
          label="Secret store"
          hint={
            store.data?.kind === 'os_store'
              ? `The master key that encrypts your API keys is held by ${store.data.backend}.`
              : store.data?.kind === 'file_fallback'
                ? `No system keyring answered; the master key is in a file only your user can read: ${store.data.path}`
                : '…'
          }
        >
          <span className="text-meta text-fg-3">
            {store.data?.kind === 'os_store' ? 'OS keyring' : store.data ? 'File' : ''}
          </span>
        </SettingsRow>
      </SettingsGroup>
    </div>
  );
}

function TokenInput({ value, onCommit }: { value: number; onCommit: (v: number) => void }) {
  const [draft, setDraft] = useState(String(value));
  const [seen, setSeen] = useState(value);
  if (seen !== value) {
    setSeen(value);
    setDraft(String(value));
  }
  const commit = () => {
    const n = Math.round(Number(draft));
    if (!Number.isFinite(n) || n < 256 || n > 200_000) {
      setDraft(String(value));
      return;
    }
    if (n !== value) onCommit(n);
  };
  return (
    <Input
      type="number"
      inputMode="numeric"
      min={256}
      max={200000}
      step={256}
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
      }}
      aria-label="Maximum reply length in tokens"
      className="w-28 text-right tnum"
    />
  );
}
