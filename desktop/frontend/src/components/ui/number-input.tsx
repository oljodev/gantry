import { MinusIcon, PlusIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { cn } from '@/lib/utils';

/**
 * A bounded integer field (15 §8): the value with a `−` and a `+` at `control-md`, no native
 * spinner. Typing edits a draft; Enter or blur commits it clamped to the range; the arrow
 * keys step. A draft that is not a number returns to the last value.
 */
function NumberInput({
  value,
  onCommit,
  min = 0,
  max = Number.MAX_SAFE_INTEGER,
  step = 1,
  className,
  ...props
}: {
  value: number;
  onCommit: (value: number) => void;
  min?: number;
  max?: number;
  step?: number;
  className?: string;
} & Pick<React.ComponentProps<'input'>, 'aria-label' | 'disabled' | 'id'>) {
  const [draft, setDraft] = useState(String(value));
  // A new value from outside replaces the draft; adjusting state during render avoids an extra pass.
  const [seen, setSeen] = useState(value);
  if (seen !== value) {
    setSeen(value);
    setDraft(String(value));
  }
  const clamp = (n: number) => Math.min(max, Math.max(min, Math.round(n)));
  const set = (n: number) => {
    const next = clamp(n);
    setDraft(String(next));
    if (next !== value) onCommit(next);
  };
  const commit = () => {
    const n = Number(draft);
    if (draft.trim() === '' || !Number.isFinite(n)) setDraft(String(value));
    else set(n);
  };
  const disabled = props.disabled === true;
  return (
    <div
      className={cn(
        'inline-flex h-(--control-md) items-stretch overflow-hidden rounded-2 border border-line bg-raised transition-colors duration-(--dur-1) focus-within:border-line-strong hover:border-line-strong',
        disabled && 'pointer-events-none text-fg-disabled',
        className,
      )}
    >
      <Step label="Decrease" onClick={() => set(value - step)} disabled={disabled || value <= min}>
        <MinusIcon />
      </Step>
      <input
        {...props}
        type="text"
        inputMode="numeric"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
          else if (e.key === 'ArrowUp') {
            e.preventDefault();
            set(value + step);
          } else if (e.key === 'ArrowDown') {
            e.preventDefault();
            set(value - step);
          }
        }}
        className="w-16 min-w-0 bg-transparent text-center text-ui text-fg tnum outline-none focus-visible:outline-none"
      />
      <Step label="Increase" onClick={() => set(value + step)} disabled={disabled || value >= max}>
        <PlusIcon />
      </Step>
    </div>
  );
}

function Step({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      tabIndex={-1}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className="flex w-7 shrink-0 items-center justify-center text-fg-3 transition-colors duration-(--dur-1) hover:bg-hover hover:text-fg disabled:pointer-events-none disabled:text-fg-disabled [&_svg]:size-3.5"
    >
      {children}
    </button>
  );
}

export { NumberInput };
