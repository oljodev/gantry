import { CaretDownIcon, ShieldCheckIcon } from '@phosphor-icons/react';

import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import type { Mode } from '@/fixtures/types';
import { MODE_HINT, MODE_LABEL } from '@/lib/modes';
import { shortcutLabel } from '@/lib/shortcuts';
import { cn } from '@/lib/utils';

/**
 * The permission mode chip in the composer (15 A13): neutral at rest, accent-tinted when the
 * mode is Auto; the guard state is a suffix. Shift+Tab cycles modes.
 */
export function ModeChip({
  mode,
  guard,
  onModeChange,
  onGuardChange,
}: {
  mode: Mode;
  guard: boolean;
  onModeChange: (m: Mode) => void;
  onGuardChange: (g: boolean) => void;
}) {
  const auto = mode === 'auto';
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="ghost"
            size="sm"
            aria-label={`Permission mode: ${MODE_LABEL[mode]}${auto ? (guard ? ', guarded' : ', unguarded') : ''}`}
            className={cn(
              'gap-1 font-medium',
              auto &&
                'bg-accent-subtle text-accent-text hover:bg-accent-subtle hover:text-accent-text',
            )}
          />
        }
      >
        {auto && <ShieldCheckIcon className={cn(!guard && 'opacity-40')} />}
        {MODE_LABEL[mode]}
        {auto && (
          <span className="font-normal opacity-80">· {guard ? 'guarded' : 'unguarded'}</span>
        )}
        <CaretDownIcon className="size-3 opacity-70" />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="w-72">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Permission mode</DropdownMenuLabel>
          <DropdownMenuRadioGroup value={mode} onValueChange={(v) => onModeChange(v as Mode)}>
            {(Object.keys(MODE_LABEL) as Mode[]).map((m) => (
              <DropdownMenuRadioItem key={m} value={m} className="h-auto items-start py-1.5">
                <span className="flex flex-col">
                  <span>{MODE_LABEL[m]}</span>
                  <span className="text-meta text-fg-3">{MODE_HINT[m]}</span>
                </span>
                {m === mode && (
                  <DropdownMenuShortcut>{shortcutLabel('shift+Tab')}</DropdownMenuShortcut>
                )}
              </DropdownMenuRadioItem>
            ))}
          </DropdownMenuRadioGroup>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuCheckboxItem checked={guard} onCheckedChange={onGuardChange} disabled={!auto}>
          Guard Auto with the judge
        </DropdownMenuCheckboxItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
