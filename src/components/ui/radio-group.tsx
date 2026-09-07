import { Radio as RadioPrimitive } from '@base-ui/react/radio';
import { RadioGroup as RadioGroupPrimitive } from '@base-ui/react/radio-group';

import { cn } from '@/lib/utils';

function RadioGroup({ className, ...props }: RadioGroupPrimitive.Props) {
  return (
    <RadioGroupPrimitive
      data-slot="radio-group"
      className={cn('grid w-full gap-2', className)}
      {...props}
    />
  );
}

function RadioGroupItem({ className, ...props }: RadioPrimitive.Root.Props) {
  return (
    <RadioPrimitive.Root
      data-slot="radio-group-item"
      className={cn(
        'relative flex size-4 shrink-0 items-center justify-center rounded-full border border-line-strong bg-raised transition-colors duration-(--dur-1) after:absolute after:-inset-2 data-checked:border-accent data-checked:bg-accent data-disabled:pointer-events-none data-disabled:border-line data-disabled:bg-hover',
        className,
      )}
      {...props}
    >
      <RadioPrimitive.Indicator
        data-slot="radio-group-indicator"
        className="size-1.5 rounded-full bg-fg-on-accent data-unchecked:hidden"
      />
    </RadioPrimitive.Root>
  );
}

/**
 * Segmented control: a radio group rendered as a row of buttons in a raised track. Used for
 * Theme, Density and the diff view toggle (15 §7).
 */
function Segmented<T extends string>({
  value,
  onValueChange,
  options,
  className,
  'aria-label': ariaLabel,
}: {
  value: T;
  onValueChange: (value: T) => void;
  options: readonly (readonly [T, string])[];
  className?: string;
  'aria-label'?: string;
}) {
  return (
    <RadioGroupPrimitive
      value={value}
      onValueChange={(v) => onValueChange(v as T)}
      aria-label={ariaLabel}
      data-slot="segmented"
      className={cn('inline-flex rounded-2 border border-line bg-raised p-0.5', className)}
    >
      {options.map(([v, label]) => (
        <RadioPrimitive.Root
          key={v}
          value={v}
          data-slot="segmented-item"
          className="h-(--control-sm) rounded-1 px-3 text-ui text-fg-2 transition-colors duration-(--dur-1) hover:text-fg data-checked:bg-selected data-checked:text-fg"
        >
          {label}
        </RadioPrimitive.Root>
      ))}
    </RadioGroupPrimitive>
  );
}

export { RadioGroup, RadioGroupItem, Segmented };
