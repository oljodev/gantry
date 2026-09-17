import { useState } from 'react';

import { SettingsGroup, SettingsRow } from '@/components/gantry/settings/SettingsRow';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import type { AgentType, Mode, ModelRule, OpenField } from '@/bindings';

const MODES: { value: Mode | 'inherit'; label: string }[] = [
  { value: 'inherit', label: 'Same as the chat' },
  { value: 'manual', label: 'Manual' },
  { value: 'auto_edit', label: 'Auto-edit' },
  { value: 'plan', label: 'Plan' },
  { value: 'auto', label: 'Auto' },
];

/** A slug from what was typed, so a new type gets an id without being asked for one. */
function slug(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
    .slice(0, 40);
}

/**
 * One agent type's form (docs/plan/18 §3).
 *
 * Every field that can be opened carries a **the caller decides** box beside it. That is the
 * whole idea of the library in one control: a researcher with strict rules has every box clear,
 * and a general agent has them ticked, and the two are the same record.
 */
export function AgentEditor({
  agent,
  namespaces,
  rules,
  onSave,
  onCancel,
}: {
  agent: AgentType;
  /** The tool namespaces installed on this machine. */
  namespaces: string[];
  /** The models the user has listed, which a type may pin one of (18 §5). */
  rules: ModelRule[];
  onSave: (agent: AgentType) => void;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState(agent);
  const isNew = agent.id === '';
  const set = (patch: Partial<AgentType>) => setDraft((d) => ({ ...d, ...patch }));
  const opens = (field: OpenField) => draft.open.includes(field);
  const toggleOpen = (field: OpenField, on: boolean) =>
    set({
      open: on ? [...draft.open, field] : draft.open.filter((f) => f !== field),
    });

  const modelValue =
    draft.model.kind === 'named'
      ? `${draft.model.model.provider}/${draft.model.model.model}`
      : draft.model.kind;

  const problem =
    draft.name.trim() === ''
      ? 'It needs a name.'
      : draft.description.trim() === ''
        ? 'It needs a description — that is what the model reads when it chooses.'
        : null;

  return (
    <>
      <div className="mb-4 flex items-center gap-3">
        <h2 className="text-title font-medium text-fg">{isNew ? 'New sub agent' : draft.name}</h2>
        <div className="ml-auto flex items-center gap-2">
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
          <Button
            disabled={problem !== null}
            onClick={() => onSave({ ...draft, id: isNew ? slug(draft.name) : draft.id })}
          >
            Save
          </Button>
        </div>
      </div>
      {problem && <p className="mb-3 text-meta text-bad">{problem}</p>}

      <SettingsGroup>
        <SettingsRow
          label="Name"
          hint={isNew ? `It will be called ${slug(draft.name) || '…'}` : draft.id}
        >
          <Input
            aria-label="Name"
            className="w-64"
            value={draft.name}
            onChange={(e) => set({ name: e.target.value })}
          />
        </SettingsRow>
        <SettingsRow
          label="Description"
          hint="One line, read by the model that is choosing which sub agent to start."
          stacked
        >
          <Input
            aria-label="Description"
            value={draft.description}
            onChange={(e) => set({ description: e.target.value })}
          />
        </SettingsRow>
        <SettingsRow
          label="Instructions"
          hint="The standing rules it works under. It cannot ask anybody anything, so say what to do when it is stuck."
          stacked
        >
          <div className="flex flex-col gap-2">
            <Textarea
              aria-label="Instructions"
              rows={8}
              value={draft.instructions}
              onChange={(e) => set({ instructions: e.target.value })}
            />
            <OpenBox
              field="instructions"
              label="Let the caller write these instead"
              checked={opens('instructions')}
              onChange={toggleOpen}
            />
          </div>
        </SettingsRow>
      </SettingsGroup>

      <div className="mt-6">
        <SettingsGroup title="What it may do">
          <SettingsRow label="Model" hint="From the models you listed above.">
            <div className="flex items-center gap-3">
              <OpenBox
                field="model"
                label="Caller picks"
                checked={opens('model')}
                onChange={toggleOpen}
              />
              <Select
                value={modelValue}
                onValueChange={(picked) => {
                  const v = picked ?? 'inherit';
                  if (v === 'inherit' || v === 'rules') {
                    set({ model: { kind: v } });
                    return;
                  }
                  const at = v.indexOf('/');
                  set({
                    model: {
                      kind: 'named',
                      model: { provider: v.slice(0, at), model: v.slice(at + 1) },
                    },
                  });
                }}
              >
                <SelectTrigger aria-label="Model" className="w-56">
                  <SelectValue>
                    {(v: string) =>
                      v === 'inherit'
                        ? "The caller's own model"
                        : v === 'rules'
                          ? 'Let it pick from my list'
                          : v.slice(v.indexOf('/') + 1)
                    }
                  </SelectValue>
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="inherit">The caller&apos;s own model</SelectItem>
                  <SelectItem value="rules">Let it pick from my list</SelectItem>
                  {rules.map((r) => (
                    <SelectItem
                      key={`${r.model.provider}/${r.model.model}`}
                      value={`${r.model.provider}/${r.model.model}`}
                    >
                      {r.model.model}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </SettingsRow>

          <SettingsRow
            label="Tools"
            hint="Which connectors it may use. Inherit gives it the same ones as the chat that started it."
            stacked
          >
            <div className="flex flex-col gap-2">
              <div className="flex flex-wrap gap-x-4 gap-y-1.5">
                <Namespace
                  id="inherit"
                  label="The chat's own"
                  list={draft.connectors}
                  onChange={(connectors) => set({ connectors })}
                />
                {namespaces.map((n) => (
                  <Namespace
                    key={n}
                    id={n}
                    label={n}
                    list={draft.connectors}
                    onChange={(connectors) => set({ connectors })}
                  />
                ))}
              </div>
              <OpenBox
                field="connectors"
                label="Let the caller choose the tools"
                checked={opens('connectors')}
                onChange={toggleOpen}
              />
            </div>
          </SettingsRow>

          <SettingsRow
            label="May change files"
            hint="Off means it is shown no tool that writes, runs or deletes anything."
          >
            <div className="flex items-center gap-3">
              <OpenBox
                field="write"
                label="Caller decides"
                checked={opens('write')}
                onChange={toggleOpen}
              />
              <Switch
                checked={draft.write_files}
                aria-label="May change files"
                onCheckedChange={(write_files) => set({ write_files })}
              />
            </div>
          </SettingsRow>

          <SettingsRow
            label="Permission mode"
            hint="Never wider than the chat that started it, whatever is chosen here."
          >
            <div className="flex items-center gap-3">
              <OpenBox
                field="mode"
                label="Caller decides"
                checked={opens('mode')}
                onChange={toggleOpen}
              />
              <Select
                value={draft.mode ?? 'inherit'}
                onValueChange={(v) => set({ mode: v === 'inherit' ? null : (v as Mode) })}
              >
                <SelectTrigger aria-label="Permission mode" className="w-44">
                  <SelectValue>
                    {(v: string) => MODES.find((m) => m.value === v)?.label ?? v}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {MODES.map((m) => (
                    <SelectItem key={m.value} value={m.value}>
                      {m.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </SettingsRow>

          <SettingsRow label="Guard" hint="Whether the judge answers its calls in Auto.">
            <Select
              value={draft.guard === null ? 'inherit' : draft.guard ? 'on' : 'off'}
              onValueChange={(v) => set({ guard: v === 'inherit' ? null : v === 'on' })}
            >
              <SelectTrigger aria-label="Guard" className="w-44">
                <SelectValue>
                  {(v: string) =>
                    v === 'inherit' ? 'Same as the chat' : v === 'on' ? 'On' : 'Off'
                  }
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="inherit">Same as the chat</SelectItem>
                <SelectItem value="on">On</SelectItem>
                <SelectItem value="off">Off</SelectItem>
              </SelectContent>
            </Select>
          </SettingsRow>

          <SettingsRow
            label="Reads your memory"
            hint="What Gantry has remembered about how you work (never in an incognito chat)."
          >
            <div className="flex items-center gap-3">
              <OpenBox
                field="memory"
                label="Caller decides"
                checked={opens('memory')}
                onChange={toggleOpen}
              />
              <Switch
                checked={draft.memory}
                aria-label="Reads your memory"
                onCheckedChange={(memory) => set({ memory })}
              />
            </div>
          </SettingsRow>

          <SettingsRow
            label="Loads skills"
            hint="Your playbooks, matched against the task it is given."
          >
            <div className="flex items-center gap-3">
              <OpenBox
                field="skills"
                label="Caller decides"
                checked={opens('skills')}
                onChange={toggleOpen}
              />
              <Switch
                checked={draft.skills}
                aria-label="Loads skills"
                onCheckedChange={(skills) => set({ skills })}
              />
            </div>
          </SettingsRow>
        </SettingsGroup>
      </div>

      <p className="mt-6 max-w-prose text-meta text-fg-3">
        A ticked box means the model that starts this sub agent may set that field in the call.
        Everything unticked is settled here, and a call that tries to set it is refused with a
        message saying so.
      </p>
    </>
  );
}

function OpenBox({
  field,
  label,
  checked,
  onChange,
}: {
  field: OpenField;
  label: string;
  checked: boolean;
  onChange: (field: OpenField, on: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-meta text-fg-2">
      <Checkbox checked={checked} onCheckedChange={(on) => onChange(field, on === true)} />
      {label}
    </label>
  );
}

function Namespace({
  id,
  label,
  list,
  onChange,
}: {
  id: string;
  label: string;
  list: string[];
  onChange: (list: string[]) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-ui text-fg">
      <Checkbox
        checked={list.includes(id)}
        onCheckedChange={(on) =>
          onChange(on === true ? [...list, id] : list.filter((n) => n !== id))
        }
      />
      {label}
    </label>
  );
}
