import { Button as ButtonPrimitive } from '@base-ui/react/button';
import { cva, type VariantProps } from 'class-variance-authority';

import { cn } from '@/lib/utils';

/**
 * Button (docs/plan/15 §8): primary · secondary · ghost · danger, sizes sm 24 / md 28 / lg 32,
 * icon-only variants. Tokens only; the focus ring is the global one.
 */
const buttonVariants = cva(
  'inline-flex shrink-0 select-none items-center justify-center gap-1.5 whitespace-nowrap rounded-2 border border-transparent text-ui font-medium transition-colors duration-(--dur-1) ease-out active:translate-y-px disabled:pointer-events-none disabled:text-fg-disabled [&_svg]:pointer-events-none [&_svg]:shrink-0',
  {
    variants: {
      variant: {
        primary: 'bg-accent text-fg-on-accent hover:bg-accent-hover disabled:bg-hover',
        secondary: 'border-line bg-raised text-fg hover:border-line-strong hover:bg-hover',
        ghost:
          'text-fg-2 hover:bg-hover hover:text-fg aria-expanded:bg-hover aria-expanded:text-fg',
        danger: 'bg-bad-subtle text-bad hover:bg-bad hover:text-fg-on-accent',
      },
      size: {
        sm: 'h-(--control-sm) px-2 text-meta [&_svg]:size-3.5',
        md: 'h-(--control-md) px-2.5 [&_svg]:size-4',
        lg: 'h-(--control-lg) px-3 [&_svg]:size-4',
        'icon-sm': 'size-(--control-sm) [&_svg]:size-3.5',
        'icon-md': 'size-(--control-md) [&_svg]:size-4',
        'icon-lg': 'size-(--control-lg) [&_svg]:size-4',
      },
    },
    defaultVariants: { variant: 'secondary', size: 'md' },
  },
);

function Button({
  className,
  variant,
  size,
  ...props
}: ButtonPrimitive.Props & VariantProps<typeof buttonVariants>) {
  return (
    <ButtonPrimitive
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
