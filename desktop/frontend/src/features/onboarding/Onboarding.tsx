import { CheckIcon, FolderPlusIcon, XIcon } from '@phosphor-icons/react';
import { useNavigate } from '@tanstack/react-router';
import { useState } from 'react';

import type { ProviderId } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Segmented } from '@/components/ui/radio-group';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { folderName, pickFolder } from '@/lib/folders';
import { chatDefaults } from '@/lib/settingsDefaults';
import { useProviderMutations, useProviders } from '@/lib/ipc/hooks/providers';
import { useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { type ThemePref, useUiStore } from '@/lib/stores/uiStore';
import { cn } from '@/lib/utils';

const STEPS = ['key', 'theme', 'folder'] as const;

/**
 * Three full-window steps on first launch (15 A20): add a key, pick a theme, optionally add a
 * folder. Reached from the gate in `providers.tsx`, and left by finishing or skipping — either
 * way the gate does not fire again.
 *
 * Every step writes where the rest of the app reads it. That sounds like it goes without saying,
 * and for twelve milestones it did not: the key was discarded on Continue under a label
 * promising it was stored, the theme was written only to the local mirror and overwritten by the
 * backend a second later, and the folder button had no handler at all.
 */
export function Onboarding() {
  const [step, setStep] = useState(0);
  const navigate = useNavigate();
  const finishOnboarding = useUiStore((s) => s.finishOnboarding);
  const finish = () => {
    finishOnboarding();
    void navigate({ to: '/chat' });
  };
  const next = () => (step < STEPS.length - 1 ? setStep(step + 1) : finish());
  const current = STEPS[step];

  return (
    <div className="flex h-full flex-col items-center justify-center bg-surface px-6 pt-(--title-strip)">
      <div className="flex w-full max-w-md flex-col gap-6">
        {current === 'key' && <KeyStep />}
        {current === 'theme' && <ThemeStep />}
        {current === 'folder' && <FolderStep />}

        <div className="mt-2 flex items-center gap-3">
          <div
            className="flex items-center gap-1.5"
            aria-label={`Step ${step + 1} of ${STEPS.length}`}
          >
            {STEPS.map((s, i) => (
              <span
                key={s}
                className={cn('size-1.5 rounded-full', i === step ? 'bg-fg' : 'bg-line-strong')}
              />
            ))}
          </div>
          <div className="ml-auto flex items-center gap-2">
            <Button variant="ghost" onClick={next}>
              {current === 'key' ? 'Skip for now' : 'Skip'}
            </Button>
            <Button variant="primary" onClick={next}>
              {step === STEPS.length - 1 ? 'Start' : 'Continue'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * The key, stored where the label says it is.
 *
 * The provider is a choice rather than a fixed field, and it opens on whichever one the default
 * model belongs to. Asking for an Anthropic key while the first message goes to OpenRouter is a
 * setup step that ends in a failed message, which is worse than no setup step.
 */
function KeyStep() {
  const providers = useProviders();
  const settings = useSettings();
  const { setKey } = useProviderMutations();
  const rows = (providers.data ?? []).filter((p) => p.available && !p.custom);
  const fallback = chatDefaults(settings.data).default_model?.provider ?? rows[0]?.id ?? '';
  const [provider, setProvider] = useState<ProviderId | ''>('');
  const chosen = provider || fallback;
  const [value, setValue] = useState('');
  const row = rows.find((p) => p.id === chosen);
  const saved = row?.key.present === true;

  const save = () => {
    const key = value.trim();
    if (!key || !chosen) return;
    setKey.mutate({ providerId: chosen, key }, { onSuccess: () => setValue('') });
  };

  return (
    <>
      <div>
        <h1 className="text-hero font-semibold tracking-[-0.01em] text-fg">Add a provider key</h1>
        <p className="mt-2 text-body text-fg-2">
          Gantry talks to the models with your own keys. Start with one; add the others any time in
          Settings.
        </p>
      </div>
      <label className="flex flex-col gap-1.5">
        <span className="text-ui font-medium text-fg">Provider</span>
        {/* `items` is what makes the closed control read "OpenRouter" rather than `openrouter`:
            the value is an id, and Base UI's Value shows the raw one unless it is given the
            labels to look it up in. */}
        <Select
          value={chosen}
          onValueChange={(v) => setProvider(v as ProviderId)}
          items={rows.map((p) => ({ value: p.id, label: p.label }))}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {rows.map((p) => (
              <SelectItem key={p.id} value={p.id}>
                {p.label}
                {p.key.present && ' · key set'}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </label>
      <label className="flex flex-col gap-1.5">
        <span className="text-ui font-medium text-fg">API key</span>
        <div className="flex gap-2">
          <Input
            type="password"
            size="lg"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && save()}
            placeholder={saved ? `Set ····${row?.key.hint ?? ''}` : 'Paste your key'}
            className="font-mono"
          />
          <Button
            variant="secondary"
            size="lg"
            onClick={save}
            disabled={value.trim().length === 0 || setKey.isPending}
          >
            {setKey.isPending ? 'Saving…' : 'Save'}
          </Button>
        </div>
        <span className="flex items-center gap-1.5 text-meta text-fg-3">
          {setKey.isError ? (
            <>
              <XIcon className="size-3.5 text-bad" />
              <span className="text-bad">That key was not accepted. Check it and try again.</span>
            </>
          ) : saved ? (
            <>
              <CheckIcon className="size-3.5 text-good" />
              Stored encrypted on this machine, and sent only to {row?.label}.
            </>
          ) : (
            <>Stored encrypted on this machine, and sent only to the provider it belongs to.</>
          )}
        </span>
      </label>
    </>
  );
}

/** The theme, written to settings rather than only to the local mirror that the backend wins. */
function ThemeStep() {
  const settings = useSettings();
  const update = useUpdateSettings();
  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);

  const choose = (next: ThemePref) => {
    // The mirror first so the window changes under the click, then the store that outlives it.
    setTheme(next);
    const appearance = settings.data?.appearance;
    if (appearance) update.mutate({ appearance: { ...appearance, theme: next } });
  };

  return (
    <>
      <div>
        <h1 className="text-hero font-semibold tracking-[-0.01em] text-fg">Pick a theme</h1>
        <p className="mt-2 text-body text-fg-2">
          Follow the system, or choose one. You can change it later in Settings.
        </p>
      </div>
      <Segmented<ThemePref>
        aria-label="Theme"
        value={theme}
        onValueChange={choose}
        options={[
          ['system', 'System'],
          ['light', 'Light'],
          ['dark', 'Dark'],
        ]}
        className="self-start"
      />
    </>
  );
}

/**
 * The folder, held until there is a chat to attach it to.
 *
 * No chat exists yet, and making one here would leave an empty chat behind every time somebody
 * opened this screen and changed their mind. It waits in the same place the welcome screen
 * already keeps folders chosen from the `+` menu before the first message.
 */
function FolderStep() {
  const roots = useUiStore((s) => s.pendingRoots);
  const setRoots = useUiStore((s) => s.setPendingRoots);

  const add = () => {
    void pickFolder('Choose a folder to work in').then((path) => {
      if (path && !roots.includes(path)) setRoots([...roots, path]);
    });
  };

  return (
    <>
      <div>
        <h1 className="text-hero font-semibold tracking-[-0.01em] text-fg">
          Add a folder to work in
        </h1>
        <p className="mt-2 text-body text-fg-2">
          Optional. A folder lets the agent read, edit and run commands there, under the permissions
          you set per chat.
        </p>
      </div>
      <div className="flex flex-col gap-2">
        <Button variant="secondary" size="lg" className="self-start" onClick={add}>
          <FolderPlusIcon />
          {roots.length > 0 ? 'Add another…' : 'Choose a folder…'}
        </Button>
        {roots.length > 0 && (
          <ul className="flex flex-col gap-1">
            {roots.map((root) => (
              <li key={root} className="flex items-center gap-2 text-meta text-fg-2">
                <CheckIcon className="size-3.5 shrink-0 text-good" />
                <span className="min-w-0 truncate" title={root}>
                  {folderName(root)}
                </span>
                <button
                  type="button"
                  aria-label={`Remove ${folderName(root)}`}
                  onClick={() => setRoots(roots.filter((r) => r !== root))}
                  className="ml-auto text-fg-3 transition-colors duration-(--dur-1) hover:text-fg"
                >
                  <XIcon className="size-3.5" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </>
  );
}
