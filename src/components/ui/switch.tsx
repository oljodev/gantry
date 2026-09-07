import { Switch as SwitchPrimitive } from '@base-ui/react/switch';

import { cn } from '@/lib/utils';

/** 32×18 track; the thumb is the one fully round control besides status dots (15 §5). */
function Switch({ className, ...props }: SwitchPrimitive.Root.Props) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        'group/switch relative inline-flex h-4.5 w-8 shrink-0 items-center rounded-full border border-transparent p-0.5 transition-colors duration-(--dur-1) after:absolute after:-inset-2 data-checked:bg-accent data-unchecked:bg-line-strong data-disabled:pointer-events-none data-disabled:bg-hover',
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="pointer-events-none block size-3.5 rounded-full bg-surface transition-transform duration-(--dur-1) ease-out data-checked:translate-x-3.5 data-unchecked:translate-x-0 group-data-disabled/switch:bg-fg-disabled"
      />
    </SwitchPrimitive.Root>
  );
}

export { Switch };
