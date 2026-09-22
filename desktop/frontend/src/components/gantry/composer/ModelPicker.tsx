import { CaretDownIcon } from '@phosphor-icons/react';
import { type ReactNode, useState } from 'react';

import { Button } from '@/components/ui/button';
import { ModelDialog } from '@/features/models/ModelDialog';
import type { ModelRef } from '@/fixtures/types';
import { modelLabel, useModelCatalog } from '@/lib/ipc/hooks/providers';
import { cn } from '@/lib/utils';

/**
 * The button that chooses a model. It opens the model dialog (15 §7) rather than a drop-up: the
 * list is a catalog to be filtered and compared, not a menu to be scrolled.
 *
 * The composer's own control, and the one every other place that picks a model uses too — a
 * settings form choosing between four hundred models is asking the same question the composer
 * asks, and a second control for it would be a second thing to learn and a worse one.
 */
export function ModelPicker({
  value,
  onChange,
  variant = 'ghost',
  className,
  label: name = 'Model',
  children,
}: {
  /** `null` until the user has picked one. Gantry proposes no model of its own: the button
   * then reads "Select model" rather than naming something nobody chose. */
  value: ModelRef | null;
  onChange: (m: ModelRef) => void;
  /** `secondary` for a form row, where the control has to look like a control. */
  variant?: 'ghost' | 'secondary';
  className?: string;
  /** What the button is called, for a screen reader; the model's name follows it. */
  label?: string;
  /** What the button says instead of the model's name, for one that adds rather than shows. */
  children?: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const { providers } = useModelCatalog();
  const label = modelLabel(providers, value);
  return (
    <>
      <Button
        variant={variant}
        size="sm"
        aria-label={`${name}: ${label}`}
        className={cn('gap-1', variant === 'secondary' && 'justify-between', className)}
        onClick={() => setOpen(true)}
      >
        {children ?? (
          <>
            <span className={cn('max-w-56 truncate', value === null && 'text-fg-2')}>{label}</span>
            <CaretDownIcon className="size-3 opacity-70" />
          </>
        )}
      </Button>
      {open && (
        <ModelDialog open onClose={() => setOpen(false)} value={value} onChange={onChange} />
      )}
    </>
  );
}
