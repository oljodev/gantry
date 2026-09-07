import { useNavigate } from '@tanstack/react-router';
import { FolderPlusIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Segmented } from '@/components/ui/radio-group';
import { type ThemePref, useUiStore } from '@/lib/stores/uiStore';
import { cn } from '@/lib/utils';

const STEPS = ['key', 'theme', 'folder'] as const;

/** Three full-window steps on first launch (15 A20): add a key, pick a theme, optionally add a folder. */
export function Onboarding() {
  const [step, setStep] = useState(0);
  const [key, setKey] = useState('');
  const theme = useUiStore((s) => s.theme);
  const setTheme = useUiStore((s) => s.setTheme);
  const navigate = useNavigate();
  const finish = () => void navigate({ to: '/chat' });
  const next = () => (step < STEPS.length - 1 ? setStep(step + 1) : finish());
  const current = STEPS[step];

  return (
    <div className="flex h-full flex-col items-center justify-center bg-surface px-6 pt-(--title-strip)">
      <div className="flex w-full max-w-md flex-col gap-6">
        {current === 'key' && (
          <>
            <div>
              <h1 className="text-hero font-semibold tracking-[-0.01em] text-fg">
                Add a provider key
              </h1>
              <p className="mt-2 text-body text-fg-2">
                Gantry talks to the models with your own keys. Start with one; add the others any
                time in Settings.
              </p>
            </div>
            <label className="flex flex-col gap-1.5">
              <span className="text-ui font-medium text-fg">Anthropic API key</span>
              <Input
                type="password"
                size="lg"
                value={key}
                onChange={(e) => setKey(e.target.value)}
                placeholder="sk-ant-…"
                className="font-mono"
              />
              <span className="text-meta text-fg-3">
                Stored encrypted on this machine and sent only to Anthropic.
              </span>
            </label>
          </>
        )}
        {current === 'theme' && (
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
              onValueChange={setTheme}
              options={[
                ['system', 'System'],
                ['light', 'Light'],
                ['dark', 'Dark'],
              ]}
              className="self-start"
            />
          </>
        )}
        {current === 'folder' && (
          <>
            <div>
              <h1 className="text-hero font-semibold tracking-[-0.01em] text-fg">
                Add a folder to work in
              </h1>
              <p className="mt-2 text-body text-fg-2">
                Optional. A folder lets the agent read, edit and run commands there, under the
                permissions you set per chat.
              </p>
            </div>
            <Button variant="secondary" size="lg" className="self-start">
              <FolderPlusIcon />
              Choose a folder…
            </Button>
          </>
        )}

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
            <Button
              variant="primary"
              onClick={next}
              disabled={current === 'key' && key.trim().length > 0 && key.trim().length < 8}
            >
              {step === STEPS.length - 1 ? 'Start' : 'Continue'}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
