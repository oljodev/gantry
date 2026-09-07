import { BugIcon, FileTextIcon, MagnifyingGlassIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { Composer } from '@/components/gantry/composer/Composer';
import { Kbd } from '@/components/ui/kbd';
import type { Mode, ModelRef } from '@/fixtures/types';

const PROMPTS: { icon: React.ReactNode; title: string; text: string }[] = [
  {
    icon: <BugIcon />,
    title: 'Fix a failing test',
    text: 'Add a folder, then ask why a test fails and let the agent fix it.',
  },
  {
    icon: <MagnifyingGlassIcon />,
    title: 'Research a question',
    text: 'Compare options with sources you can check.',
  },
  {
    icon: <FileTextIcon />,
    title: 'Draft a document',
    text: 'Turn notes into a page you can edit in the panel.',
  },
];

/** The empty chat (15 A20, §8): the hero line, three prompt cards, the composer, a hint. */
export function Welcome() {
  const [mode, setMode] = useState<Mode>('auto_edit');
  const [guard, setGuard] = useState(true);
  const [model, setModel] = useState<ModelRef>({
    provider: 'anthropic',
    id: 'claude-opus-5',
    label: 'Claude Opus 5',
  });
  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-1 flex-col items-center justify-center px-6">
        <div className="w-full max-w-(--measure)">
          <h1 className="text-center text-hero font-semibold tracking-[-0.01em] text-fg">
            What should we work on?
          </h1>
          <div className="mt-8 grid grid-cols-3 gap-3">
            {PROMPTS.map((p) => (
              <button
                key={p.title}
                type="button"
                className="flex flex-col gap-2 rounded-3 border border-line-subtle bg-raised p-3 text-left transition-colors duration-(--dur-1) hover:border-line-strong hover:bg-hover"
              >
                <span className="text-fg-2 [&_svg]:size-4">{p.icon}</span>
                <span className="text-ui font-medium text-fg">{p.title}</span>
                <span className="text-meta text-fg-2">{p.text}</span>
              </button>
            ))}
          </div>
        </div>
      </div>
      <Composer
        mode={mode}
        guard={guard}
        model={model}
        roots={[]}
        onModeChange={setMode}
        onGuardChange={setGuard}
        onModelChange={setModel}
      />
      <div className="flex h-8 items-center justify-center gap-1.5 text-meta text-fg-3">
        Add files, folders and connectors with <Kbd>+</Kbd> · search anything with <Kbd>⌘</Kbd>
        <Kbd>K</Kbd>
      </div>
    </div>
  );
}
