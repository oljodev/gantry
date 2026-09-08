import { CaretDownIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { Button } from '@/components/ui/button';
import { ModelDialog } from '@/features/models/ModelDialog';
import type { ModelRef } from '@/fixtures/types';
import { modelLabel, useModelCatalog } from '@/lib/ipc/hooks/providers';

/**
 * The composer's model button. It opens the model dialog (15 §7) rather than a drop-up: the
 * list is a catalog to be filtered and compared, not a menu to be scrolled.
 */
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
    <>
      <Button
        variant="ghost"
        size="sm"
        aria-label={`Model: ${label}`}
        className="gap-1"
        onClick={() => setOpen(true)}
      >
        <span className="max-w-56 truncate">{label}</span>
        <CaretDownIcon className="size-3 opacity-70" />
      </Button>
      {open && (
        <ModelDialog open onClose={() => setOpen(false)} value={value} onChange={onChange} />
      )}
    </>
  );
}
