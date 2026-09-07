import { CaretDownIcon, CheckIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { Button } from '@/components/ui/button';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import type { ModelRef } from '@/fixtures/types';
import { modelLabel, useModelCatalog } from '@/lib/ipc/hooks/providers';

function context(n: number | null) {
  if (n === null) return '';
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
  return `${Math.round(n / 1000)}K`;
}

/** Provider › model, in a popover on the command list; providers without a key are dimmed (15 §9). */
export function ModelPicker({
  value,
  onChange,
}: {
  value: ModelRef;
  onChange: (m: ModelRef) => void;
}) {
  const [open, setOpen] = useState(false);
  const { providers } = useModelCatalog();
  const label = modelLabel(providers, value);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button variant="ghost" size="sm" aria-label={`Model: ${label}`} className="gap-1" />
        }
      >
        <span className="max-w-56 truncate">{label}</span>
        <CaretDownIcon className="size-3 opacity-70" />
      </PopoverTrigger>
      <PopoverContent className="w-96 p-0">
        <Command>
          <CommandInput placeholder="Search models…" />
          <CommandList className="max-h-80">
            <CommandEmpty>No model matches.</CommandEmpty>
            {providers.length === 0 && (
              <CommandGroup heading="Providers">
                <CommandItem disabled value="none">
                  <span className="text-fg-disabled">No provider configured</span>
                  <span className="ml-auto text-meta text-fg-3">Settings › Providers</span>
                </CommandItem>
              </CommandGroup>
            )}
            {providers.map((p) => (
              <CommandGroup key={p.id} heading={p.label}>
                {!p.available ? (
                  <CommandItem disabled value={`${p.id}-unavailable`}>
                    <span className="text-fg-disabled">No client in this build</span>
                  </CommandItem>
                ) : !p.hasKey ? (
                  <CommandItem disabled value={`${p.id}-none`}>
                    <span className="text-fg-disabled">No key</span>
                    <span className="ml-auto text-meta text-fg-3">Settings › Providers</span>
                  </CommandItem>
                ) : p.loading ? (
                  <CommandItem disabled value={`${p.id}-loading`}>
                    <span className="text-fg-3">Loading models…</span>
                  </CommandItem>
                ) : (
                  p.models.map((m) => (
                    <CommandItem
                      key={m.id}
                      value={`${p.id} ${m.id} ${m.display_name}`}
                      onSelect={() => {
                        onChange({ provider: p.id, model: m.id });
                        setOpen(false);
                      }}
                    >
                      <span className="truncate">{m.display_name}</span>
                      <span className="ml-auto flex shrink-0 items-center gap-2 text-meta text-fg-3 tnum">
                        {context(m.context_window)}
                        {m.id === value.model && p.id === value.provider && (
                          <CheckIcon className="text-fg" />
                        )}
                      </span>
                    </CommandItem>
                  ))
                )}
              </CommandGroup>
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
