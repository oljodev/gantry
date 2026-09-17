import { PlusIcon, TrashIcon } from '@phosphor-icons/react';
import { useMemo, useState } from 'react';

import { SettingsGroup, SettingsRow } from '@/components/gantry/settings/SettingsRow';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { NumberInput } from '@/components/ui/number-input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { ModelPicker } from '@/components/gantry/composer/ModelPicker';
import { AgentEditor } from '@/features/customize/AgentEditor';
import { useAgentTypeMutations, useAgentTypes } from '@/lib/ipc/hooks/agents';
import { useConnectors } from '@/lib/ipc/hooks/connectors';
import { useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import type { AgentType, ModelRule, SubAgentSettings } from '@/bindings';

/** What the **Add a model** button holds before anything is chosen: no model at all. */
const NO_MODEL = { provider: '', model: '' };

/** A blank type, in the shape a new one starts as: nothing fixed, nothing decided. */
function blank(): AgentType {
  return {
    id: '',
    name: '',
    description: '',
    instructions: '',
    model: { kind: 'inherit' },
    connectors: ['inherit'],
    mode: null,
    guard: null,
    write_files: false,
    memory: false,
    skills: false,
    open: [],
    builtin: false,
    enabled: true,
  };
}

/**
 * Sub agents inside Customize (docs/plan/18 §10 phase B, 11 §2).
 *
 * Two halves, in the order the questions come up: **how they run** — who answers a card, how
 * many at once, which models they may choose between — and then **what they are**, which is the
 * library. The library is the longer half but the settings are the ones a person changes on the
 * day they first meet the feature, so they are on top.
 */
export function SubAgentsSection() {
  const types = useAgentTypes();
  const { save, remove, reset, setEnabled } = useAgentTypeMutations();
  const settings = useSettings();
  const updateSettings = useUpdateSettings();
  const installed = useConnectors();
  const [editing, setEditing] = useState<AgentType | null>(null);

  const prefs = settings.data?.subagents;
  const setPrefs = (patch: Partial<SubAgentSettings>) => {
    if (!prefs) return;
    updateSettings.mutate({ subagents: { ...prefs, ...patch } });
  };

  /** The namespaces a type may be given, plus the one that means "whatever the chat has". */
  const namespaces = useMemo(
    () => (installed.data ?? []).filter((c) => c.enabled).map((c) => c.namespace),
    [installed.data],
  );

  const rules = prefs?.model_rules ?? [];
  const setRules = (next: ModelRule[]) => setPrefs({ model_rules: next });

  if (editing) {
    return (
      <AgentEditor
        agent={editing}
        namespaces={namespaces}
        rules={rules}
        onCancel={() => setEditing(null)}
        onSave={(agent) => {
          save.mutate(agent, { onSuccess: () => setEditing(null) });
        }}
      />
    );
  }

  return (
    <>
      <div className="mb-1 flex items-center gap-3">
        <h2 className="text-title font-medium text-fg">Sub agents</h2>
        <Button
          variant="secondary"
          className="ml-auto"
          onClick={() => setEditing(blank())}
          disabled={!prefs}
        >
          <PlusIcon />
          New sub agent
        </Button>
      </div>
      <p className="mb-5 max-w-prose text-meta text-fg-2">
        A model you are talking to can hand part of a job to another model and wait for its report.
        You never speak to a sub agent, and it cannot see your conversation — the one thing it can
        stop you for is permission.
      </p>

      <SettingsGroup title="How they run">
        <SettingsRow
          label="When a sub agent needs permission"
          hint="A card in this chat names the agent that asked. The guard answers in your place if you would rather not be stopped by something you are not watching."
        >
          <Select
            value={prefs?.permission ?? 'ask'}
            onValueChange={(v) => setPrefs({ permission: v as SubAgentSettings['permission'] })}
            disabled={!prefs}
          >
            <SelectTrigger aria-label="Who answers" className="w-52">
              <SelectValue>
                {(v: SubAgentSettings['permission']) =>
                  v === 'guard' ? 'The guard answers' : 'Ask me'
                }
              </SelectValue>
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="ask">Ask me</SelectItem>
              <SelectItem value="guard">The guard answers</SelectItem>
            </SelectContent>
          </Select>
        </SettingsRow>
        <SettingsRow
          label="Running at once"
          hint="More than this wait their turn rather than being refused."
        >
          <NumberInput
            aria-label="Running at once"
            value={prefs?.max_concurrent ?? 3}
            min={1}
            max={8}
            onCommit={(max_concurrent) => setPrefs({ max_concurrent })}
          />
        </SettingsRow>
        <SettingsRow
          label="Most in one reply"
          hint="Past this the model is told the limit and carries on without another."
        >
          <NumberInput
            aria-label="Most in one reply"
            value={prefs?.max_per_turn ?? 10}
            min={1}
            max={50}
            onCommit={(max_per_turn) => setPrefs({ max_per_turn })}
          />
        </SettingsRow>
        <SettingsRow
          label="In incognito chats"
          hint="A sub agent's transcript is a row in the database, which is the thing incognito promises not to leave."
        >
          <Switch
            checked={prefs?.in_incognito ?? false}
            onCheckedChange={(in_incognito) => setPrefs({ in_incognito })}
            disabled={!prefs}
          />
        </SettingsRow>
        <SettingsRow
          label="Keep transcripts for"
          hint="Days. Zero keeps them for as long as the chat that started them."
        >
          <NumberInput
            aria-label="Keep transcripts for"
            value={prefs?.keep_days ?? 0}
            min={0}
            max={365}
            onCommit={(keep_days) => setPrefs({ keep_days })}
          />
        </SettingsRow>
      </SettingsGroup>

      <section className="mt-6">
        <h2 className="mb-1 text-title font-medium text-fg">Models to choose from</h2>
        <p className="mb-3 max-w-prose text-meta text-fg-2">
          Name a model and say what it is for. The model doing the delegating reads your words and
          picks. With none listed, every sub agent runs the model you are talking to.
        </p>
        <div className="flex flex-col gap-2">
          {rules.map((rule, i) => (
            <div key={`${rule.model.provider}/${rule.model.model}`} className="flex gap-2">
              {/* The same button and the same dialog the composer uses (15 §7). A field you can
                  type into is a field that can hold something that is not a model. */}
              <ModelPicker
                variant="secondary"
                className="w-64 shrink-0"
                value={rule.model}
                onChange={(model) => {
                  const next = [...rules];
                  next[i] = { ...rule, model };
                  setRules(next);
                }}
              />
              <Input
                aria-label="When to use it"
                value={rule.when}
                placeholder="research and long documents"
                onChange={(e) => {
                  const next = [...rules];
                  next[i] = { ...rule, when: e.target.value };
                  setRules(next);
                }}
              />
              <Button
                variant="ghost"
                size="icon-md"
                aria-label="Remove"
                className="text-bad hover:bg-bad-subtle"
                onClick={() => setRules(rules.filter((_, at) => at !== i))}
              >
                <TrashIcon />
              </Button>
            </div>
          ))}
          <div>
            {/* Picking a model is adding it. A model already on the list is left where it is,
                with whatever was written beside it. */}
            <ModelPicker
              variant="secondary"
              label="Add a model"
              value={NO_MODEL}
              onChange={(model) => {
                if (
                  rules.some(
                    (r) => r.model.provider === model.provider && r.model.model === model.model,
                  )
                ) {
                  return;
                }
                setRules([...rules, { model, when: '' }]);
              }}
            >
              <PlusIcon />
              Add a model
            </ModelPicker>
          </div>
        </div>
      </section>

      <section className="mt-6">
        <h2 className="mb-1 text-title font-medium text-fg">The library</h2>
        <p className="mb-3 max-w-prose text-meta text-fg-2">
          What a model may start. Each one decides some things for itself and leaves the rest to
          whoever starts it — a researcher with strict rules, a general agent you brief per job.
        </p>
        <div className="flex flex-col gap-2">
          {(types.data ?? []).map((agent) => (
            <div
              key={agent.id}
              className="rounded-3 border border-line-subtle bg-base px-4 py-3 transition-colors duration-(--dur-1) hover:border-line"
            >
              <div className="flex items-start gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="text-ui font-medium text-fg">{agent.name}</span>
                    <code className="text-meta text-fg-3">{agent.id}</code>
                  </div>
                  <p className="mt-0.5 text-meta text-fg-2">{agent.description}</p>
                  <p className="mt-1 text-meta text-fg-3">{summary(agent)}</p>
                </div>
                <Switch
                  checked={agent.enabled}
                  aria-label={`Offer ${agent.name}`}
                  onCheckedChange={(enabled) => setEnabled.mutate({ id: agent.id, enabled })}
                />
              </div>
              <div className="mt-3 flex items-center gap-2">
                <Button variant="secondary" size="sm" onClick={() => setEditing(agent)}>
                  Edit
                </Button>
                {agent.builtin ? (
                  <Button variant="ghost" size="sm" onClick={() => reset.mutate(agent.id)}>
                    Reset
                  </Button>
                ) : (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-bad hover:bg-bad-subtle"
                    onClick={() => remove.mutate(agent.id)}
                  >
                    <TrashIcon />
                    Delete
                  </Button>
                )}
              </div>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}

/** The one line under a row: what this type settled and what it leaves to the parent. */
function summary(agent: AgentType): string {
  const parts: string[] = [];
  parts.push(agent.write_files ? 'can change files' : 'reads only');
  parts.push(
    agent.connectors.includes('inherit')
      ? "the chat's own tools"
      : agent.connectors.length > 0
        ? agent.connectors.join(', ')
        : 'no tools',
  );
  if (agent.model.kind === 'named') parts.push(agent.model.model.model);
  if (agent.model.kind === 'rules') parts.push('picks its own model');
  parts.push(
    agent.open.length === 0
      ? 'decides everything itself'
      : `the caller sets ${agent.open.join(', ')}`,
  );
  return parts.join(' · ');
}
