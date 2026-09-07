import { Checkbox as CheckboxPrimitive } from '@base-ui/react/checkbox';
import { CheckIcon, MinusIcon } from '@phosphor-icons/react';

import { cn } from '@/lib/utils';

function Checkbox({ className, ...props }: CheckboxPrimitive.Root.Props) {
  return (
    <CheckboxPrimitive.Root
      data-slot="checkbox"
      className={cn(
        'peer relative flex size-4 shrink-0 items-center justify-center rounded-1 border border-line-strong bg-raised transition-colors duration-(--dur-1) after:absolute after:-inset-2 data-checked:border-accent data-checked:bg-accent data-checked:text-fg-on-accent data-indeterminate:border-accent data-indeterminate:bg-accent data-indeterminate:text-fg-on-accent data-disabled:pointer-events-none data-disabled:border-line data-disabled:bg-hover data-disabled:text-fg-disabled',
        className,
      )}
      {...props}
    >
      <CheckboxPrimitive.Indicator
        data-slot="checkbox-indicator"
        className="grid place-content-center data-unchecked:hidden"
        keepMounted
      >
        {props.indeterminate ? (
          <MinusIcon size={12} weight="bold" />
        ) : (
          <CheckIcon size={12} weight="bold" />
        )}
      </CheckboxPrimitive.Indicator>
    </CheckboxPrimitive.Root>
  );
}

export { Checkbox };
