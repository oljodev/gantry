import { useState } from 'react';

import { CaretDownIcon, CheckIcon } from '@phosphor-icons/react';
import { Combobox as ComboboxPrimitive } from '@base-ui/react/combobox';

import { menuPopup } from '@/components/ui/menu-styles';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { typeaheadLabel } from '@/lib/typeahead';
import { cn } from '@/lib/utils';

/** One thing that can be picked: what is stored, what it is called, and why you would pick it. */
export interface PickerOption {
  value: string;
  label: string;
  /** The price, the date, the small print — shown muted, to the right. */
  detail?: string;
}

/**
 * The length at which a menu stops being something you read and starts being something you
 * search. Below it, a plain `Select` is faster: the list is short enough to see at once and a
 * text field in front of it is a thing to dismiss.
 *
 * Fifty-three image models is what put the number here (03 §5): a menu of long, similar ids with
 * no way to type into it is the reason "I find it difficult to choose the model I want".
 */
export const SEARCH_FROM = 9;

/**
 * Pick one of a list, with a filter box once the list is long (15 §8).
 *
 * One control for the two places a long list is chosen from — a connector's settings form and
 * the permission card's `choices` (04 §7) — because they are the same question asked at
 * different moments, and a model menu that behaved differently in the two would be two things to
 * learn. Below [`SEARCH_FROM`] options it renders the ordinary `Select`, so the short menus
 * elsewhere are unchanged.
 */
export function OptionPicker({
  value,
  options,
  onChange,
  label,
  disabled = false,
  className,
  id,
}: {
  value: string;
  options: PickerOption[];
  onChange: (value: string) => void;
  /** What the control is called, for a screen reader. */
  label: string;
  disabled?: boolean;
  className?: string;
  id?: string;
}) {
  const current = options.find((o) => o.value === value);
  // `null` means "show the value"; a string is what is being typed right now.
  const [query, setQuery] = useState<string | null>(null);

  if (options.length < SEARCH_FROM) {
    return (
      <Select value={value} onValueChange={(next) => onChange(next as string)} disabled={disabled}>
        <SelectTrigger id={id} aria-label={label} className={cn('w-full', className)}>
          <SelectValue placeholder={options[0]?.label}>
            <span className="truncate">{current?.label ?? value}</span>
          </SelectValue>
        </SelectTrigger>
        <SelectContent className="max-w-[min(34rem,80vw)]">
          {options.map((option) => (
            // The row is two elements, so the text to type is named rather than read off it.
            <SelectItem
              key={option.value}
              value={option.value}
              label={typeaheadLabel(option.label)}
            >
              <span className="min-w-0 flex-1 truncate">{option.label}</span>
              {option.detail && (
                <span className="shrink-0 text-meta text-fg-3">{option.detail}</span>
              )}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  }

  // The item values are the stored strings rather than objects, so what the input shows while
  // you type is the thing that will be saved, and the detail is looked up when a row is drawn.
  const detail = new Map(options.map((o) => [o.value, o.detail]));
  return (
    <ComboboxPrimitive.Root
      items={options.map((o) => o.value)}
      value={value}
      inputValue={query ?? value}
      disabled={disabled}
      onInputValueChange={setQuery}
      // Closing without choosing puts the value back in the box. A combobox normally leaves the
      // half-typed query there, which is right when the text *is* the answer; here the text is a
      // search for one of a fixed list, and a box reading "zzz" under a model that is still
      // `flux.2-pro` is a lie anybody would read later.
      onOpenChange={(open) => {
        if (!open) setQuery(null);
      }}
      onValueChange={(next) => {
        if (typeof next === 'string' && next !== '') {
          setQuery(null);
          onChange(next);
        }
      }}
    >
      <ComboboxPrimitive.InputGroup
        className={cn(
          'relative flex h-(--control-md) w-full items-center rounded-2 border border-line bg-raised pr-7 pl-2.5 transition-colors duration-(--dur-1) focus-within:border-line-strong hover:border-line-strong',
          className,
        )}
      >
        <ComboboxPrimitive.Input
          id={id}
          aria-label={label}
          placeholder={current?.label ?? 'Type to search'}
          className="h-full w-full min-w-0 truncate border-0 bg-transparent text-ui text-fg outline-none placeholder:text-fg-3"
        />
        <ComboboxPrimitive.Trigger
          aria-label={`Open ${label}`}
          className="absolute right-1.5 flex size-5 items-center justify-center text-fg-3"
        >
          <CaretDownIcon className="size-4" />
        </ComboboxPrimitive.Trigger>
      </ComboboxPrimitive.InputGroup>
      <ComboboxPrimitive.Portal>
        <ComboboxPrimitive.Positioner sideOffset={6} className="isolate z-50">
          {/* Wider than the trigger when it needs to be: the id is the thing being chosen, and a
              menu that truncates it at the same point on every row is a menu of one answer. */}
          <ComboboxPrimitive.Popup
            className={cn(
              menuPopup,
              'w-max min-w-(--anchor-width) max-w-[min(38rem,var(--available-width))]',
            )}
          >
            <ComboboxPrimitive.Empty className="px-2 py-3 text-meta text-fg-3">
              Nothing matches that.
            </ComboboxPrimitive.Empty>
            <ComboboxPrimitive.List className="max-h-[min(20rem,var(--available-height))] overflow-y-auto">
              {(item: string) => (
                <ComboboxPrimitive.Item
                  key={item}
                  value={item}
                  className="relative flex h-(--control-md) w-full cursor-default select-none items-center gap-2 rounded-2 pr-8 pl-2 text-ui text-fg outline-none data-highlighted:bg-hover"
                >
                  <span className="min-w-0 flex-1 truncate">{item}</span>
                  {detail.get(item) && (
                    <span className="shrink-0 text-meta text-fg-3">{detail.get(item)}</span>
                  )}
                  <ComboboxPrimitive.ItemIndicator className="pointer-events-none absolute right-2 flex size-4 items-center justify-center">
                    <CheckIcon className="size-4" />
                  </ComboboxPrimitive.ItemIndicator>
                </ComboboxPrimitive.Item>
              )}
            </ComboboxPrimitive.List>
          </ComboboxPrimitive.Popup>
        </ComboboxPrimitive.Positioner>
      </ComboboxPrimitive.Portal>
    </ComboboxPrimitive.Root>
  );
}
