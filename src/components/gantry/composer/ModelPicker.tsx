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
import { PROVIDER_LABEL, providers } from '@/fixtures/settings';
import type { ModelRef } from '@/fixtures/types';

/** Provider › model, in a popover on the command list; providers without a key are dimmed (15 §9). */
export function ModelPicker({
  value,
  onChange,
}: {
  value: ModelRef;
  onChange: (m: ModelRef) => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button
            variant="ghost"
            size="sm"
            aria-label={`Model: ${value.label}`}
            className="gap-1"
          />
        }
      >
        {value.label}
        <CaretDownIcon className="size-3 opacity-70" />
      </PopoverTrigger>
      <PopoverContent className="w-80 p-0">
        <Command>
          <CommandInput placeholder="Search models…" />
          <CommandList>
            <CommandEmpty>No model matches.</CommandEmpty>
            {providers.map((p) => (
              <CommandGroup key={p.id} heading={PROVIDER_LABEL[p.id]}>
                {p.models.length === 0 ? (
                  <CommandItem disabled value={`${p.id}-none`}>
                    <span className="text-fg-disabled">No key</span>
                    <span className="ml-auto text-meta text-fg-3">Settings › Providers</span>
                  </CommandItem>
                ) : (
                  p.models.map((m) => (
                    <CommandItem
                      key={m.id}
                      value={`${p.id} ${m.label}`}
                      onSelect={() => {
                        onChange({ provider: p.id, id: m.id, label: m.label });
                        setOpen(false);
                      }}
                    >
                      {m.label}
                      <span className="ml-auto flex items-center gap-2 text-meta text-fg-3 tnum">
                        {m.context}
                        {m.id === value.id && <CheckIcon className="text-fg" />}
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
