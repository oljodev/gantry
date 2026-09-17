import { FolderOpenIcon } from '@phosphor-icons/react';

import type { UserConfigField } from '@/bindings';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { OptionPicker } from '@/components/ui/option-picker';
import { Switch } from '@/components/ui/switch';
import { pickFolder } from '@/lib/folders';

/**
 * Step 2 of the install (docs/plan/03 §11): the keys a connector asks the user to fill in — a
 * host for a self-hosted server, a workspace id, a folder to work in — before anything runs.
 *
 * A `sensitive` field arrives empty even on a connector that has one saved, and that is not an
 * oversight: the value is in the vault and the backend never sends it back (06 §5). The field
 * says so rather than looking like a blank the user forgot to fill, because "leave it alone" and
 * "it is gone" are different things and only one of them needs acting on.
 */
export function ConfigForm({
  fields,
  values,
  hasSaved,
  onChange,
}: {
  fields: UserConfigField[];
  values: Record<string, string>;
  /** Whether this instance has been configured before, so a blank secret means "kept". */
  hasSaved?: boolean;
  onChange: (key: string, value: string) => void;
}) {
  return (
    <div className="flex flex-col gap-3">
      {fields.map((field) => (
        <Field
          key={field.key}
          field={field}
          value={values[field.key] ?? ''}
          hasSaved={hasSaved ?? false}
          onChange={(next) => onChange(field.key, next)}
        />
      ))}
    </div>
  );
}

function Field({
  field,
  value,
  hasSaved,
  onChange,
}: {
  field: UserConfigField;
  value: string;
  hasSaved: boolean;
  onChange: (value: string) => void;
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

  // A `select` whose options nobody could supply falls back to a box: a menu of nothing is a
  // control that cannot be answered, where a typed value at least reaches the connector.
  const options = field.options ?? [];
  if (field.type === 'select' && options.length > 0) {
    const current = options.find((o) => o.value === value);
    return (
      <div className="flex flex-col gap-1.5">
        {label}
        <OptionPicker
          id={field.key}
          label={field.title}
          value={value}
          options={options.map((o) => ({
            value: o.value,
            label: o.label,
            detail: o.detail ?? undefined,
          }))}
          onChange={onChange}
        />
        {current?.detail && <p className="text-meta text-fg-3">{current.detail}</p>}
      </div>
    );
  }

  if (field.type === 'boolean') {
    return (
      <div className="flex items-start justify-between gap-3">
        {label}
        <Switch checked={value === 'true'} onCheckedChange={(on) => onChange(String(on))} />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-1.5">
      {label}
      <div className="flex items-center gap-2">
        <Input
          id={field.key}
          value={value}
          type={field.sensitive ? 'password' : field.type === 'number' ? 'number' : 'text'}
          placeholder={field.sensitive && hasSaved ? 'Saved — leave blank to keep it' : field.title}
          onChange={(e) => onChange(e.target.value)}
        />
        {(field.type === 'directory' || field.type === 'file') && (
          <Button
            variant="secondary"
            onClick={() => {
              void pickFolder().then((path) => path && onChange(path));
            }}
          >
            <FolderOpenIcon />
            Choose
          </Button>
        )}
      </div>
    </div>
  );
}
