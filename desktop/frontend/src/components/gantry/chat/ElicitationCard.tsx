import { useState } from 'react';

import type { ElicitationAction, ElicitationField } from '@/bindings';
import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { InteractionCard } from '@/components/gantry/chat/InteractionCard';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import type { ElicitationAsk } from '@/fixtures/types';

export interface ElicitationAnswer {
  action: ElicitationAction;
  values: Record<string, unknown>;
}

/**
 * A server asking for something mid-call (docs/plan/03 §6, MCP's MRTR).
 *
 * The turn has stopped and is waiting, exactly as it does for a permission prompt, which is why
 * this wears the same shell. What it is *not* is a permission: nobody is deciding whether the
 * call may happen — it is happening, and it needs an answer to carry on.
 *
 * Three actions, because the protocol distinguishes three and a server may act on the
 * difference. **Send** returns the form. **Not this time** declines: no answer, carry on without
 * it. **Stop** cancels: the user is not answering, and the server should give up rather than ask
 * again. Collapsing the last two into one button would throw away a distinction the server asked
 * for.
 */
export function ElicitationCard({
  ask,
  onAnswer,
  pending = false,
}: {
  ask: ElicitationAsk;
  onAnswer?: (answer: ElicitationAnswer) => void;
  pending?: boolean;
}) {
  const [values, setValues] = useState<Record<string, unknown>>(() => defaults(ask.fields));
  const missing = ask.fields
    .filter((f) => f.required)
    .filter((f) => {
      const v = values[f.key];
      return v === undefined || v === '' || v === null;
    })
    .map((f) => f.title);

  return (
    <InteractionCard
      mark={<ConnectorMark id={ask.connector} name={ask.connectorName} />}
      title={`${ask.connectorName} needs an answer`}
      actions={
        <>
          <Button
            variant="primary"
            disabled={pending || missing.length > 0}
            title={missing.length > 0 ? `Still needed: ${missing.join(', ')}` : undefined}
            onClick={() => onAnswer?.({ action: 'accept', values })}
          >
            Send
          </Button>
          <Button
            variant="secondary"
            disabled={pending}
            onClick={() => onAnswer?.({ action: 'decline', values: {} })}
          >
            Not this time
          </Button>
          <Button
            variant="ghost"
            disabled={pending}
            className="ml-auto"
            onClick={() => onAnswer?.({ action: 'cancel', values: {} })}
          >
            Stop
          </Button>
        </>
      }
    >
      {/* The server's own words, and said to be the server's: this text comes from a third
          party and is not Gantry speaking. */}
      <p className="selectable">{ask.message}</p>
      {ask.fields.length > 0 && (
        <div className="mt-3 flex flex-col gap-3">
          {ask.fields.map((field) => (
            <Field
              key={field.key}
              field={field}
              value={values[field.key]}
              onChange={(next) => setValues((v) => ({ ...v, [field.key]: next }))}
            />
          ))}
        </div>
      )}
    </InteractionCard>
  );
}

function Field({
  field,
  value,
  onChange,
}: {
  field: ElicitationField;
  value: unknown;
  onChange: (value: unknown) => void;
}) {
  const label = (
    <div className="flex flex-col gap-0.5">
      <label htmlFor={field.key} className="text-ui text-fg">
        {field.title}
        {field.required && <span className="pl-1 text-fg-3">required</span>}
      </label>
      {field.description && <p className="text-meta text-fg-2">{field.description}</p>}
    </div>
  );

  if (field.kind === 'boolean') {
    return (
      <div className="flex items-start justify-between gap-3">
        {label}
        <Switch checked={value === true} onCheckedChange={onChange} />
      </div>
    );
  }

  if (field.kind === 'enum') {
    return (
      <div className="flex flex-col gap-1.5">
        {label}
        <Select value={typeof value === 'string' ? value : ''} onValueChange={onChange}>
          <SelectTrigger id={field.key} aria-label={field.title}>
            <SelectValue placeholder="Choose one" />
          </SelectTrigger>
          <SelectContent>
            {field.options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
    );
  }

  const numeric = field.kind === 'number' || field.kind === 'integer';
  return (
    <div className="flex flex-col gap-1.5">
      {label}
      <Input
        id={field.key}
        type={numeric ? 'number' : inputType(field.format)}
        step={field.kind === 'integer' ? 1 : undefined}
        value={value === undefined || value === null ? '' : String(value)}
        onChange={(e) => {
          const text = e.target.value;
          // A number field sends a number, or nothing at all: "" is not zero, and a server that
          // asked for an integer should not be handed an empty string wearing one's clothes.
          if (!numeric) return onChange(text);
          if (text === '') return onChange(undefined);
          const parsed = Number(text);
          onChange(Number.isNaN(parsed) ? undefined : parsed);
        }}
      />
    </div>
  );
}

/** What the browser should offer: a date picker for a date, a keyboard for an email. */
function inputType(format: string | null | undefined): string {
  switch (format) {
    case 'email':
      return 'email';
    case 'uri':
      return 'url';
    case 'date':
      return 'date';
    case 'date-time':
      return 'datetime-local';
    default:
      return 'text';
  }
}

/** A boolean starts false rather than absent: a switch is always showing one of its two states. */
function defaults(fields: ElicitationField[]): Record<string, unknown> {
  const values: Record<string, unknown> = {};
  for (const field of fields) {
    if (field.kind === 'boolean') values[field.key] = false;
  }
  return values;
}
