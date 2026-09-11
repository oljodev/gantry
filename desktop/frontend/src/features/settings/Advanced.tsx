import type { AdvancedSettings } from '@/bindings';
import { SettingsGroup, SettingsRow } from '@/components/gantry/settings/SettingsRow';
import { NumberInput } from '@/components/ui/number-input';
import { Switch } from '@/components/ui/switch';
import { isTauri } from '@/lib/ipc/client';
import { useSecretStoreStatus, useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { advancedDefaults } from '@/lib/settingsDefaults';

/**
 * Settings → Advanced (11 §2): output cap, the tool round cap, developer mode and where the
 * master key lives.
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
          <NumberInput
            aria-label="Maximum reply length in tokens"
            value={advanced.max_output_tokens}
            min={256}
            max={200_000}
            step={256}
            onCommit={(v) => patch({ max_output_tokens: v })}
          />
        </SettingsRow>
        <SettingsRow
          label="Tool rounds per reply"
          hint="How many times one reply may call tools and continue before Gantry stops it."
        >
          <NumberInput
            aria-label="Tool rounds per reply"
            value={advanced.max_tool_rounds}
            min={1}
            max={500}
            step={1}
            onCommit={(v) => patch({ max_tool_rounds: v })}
          />
        </SettingsRow>
        <SettingsRow
          label="Tool result size"
          hint="How much of one tool result the model is given, in kilobytes. Both ends are kept; the whole output stays in the activity row."
        >
          <NumberInput
            aria-label="Tool result size in kilobytes"
            value={advanced.max_result_kb}
            min={1}
            max={1024}
            step={1}
            onCommit={(v) => patch({ max_result_kb: v })}
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
