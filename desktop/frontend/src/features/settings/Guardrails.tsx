import { PlusIcon, TrashIcon, WarningIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import type { GuardrailKind, GuardrailRule } from '@/bindings';
import { SettingsGroup, SettingsRow } from '@/components/gantry/settings/SettingsRow';
import { Badge } from '@/components/ui/badge';
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
import { isTauri } from '@/lib/ipc/client';
import { useGuardrails, useSettings, useUpdateSettings } from '@/lib/ipc/hooks/settings';
import { guardrailDefaults } from '@/lib/settingsDefaults';

/** The four kinds, in the order they are shown: what stops, what asks, then what is watched. */
const GROUPS: { kind: GuardrailKind; title: string; hint: string; badge: string }[] = [
  {
    kind: 'deny',
    title: 'Never run',
    hint: 'These never run, in any mode. The model is told a guardrail stopped it.',
    badge: 'Blocked',
  },
  {
    kind: 'confirm',
    title: 'Always ask',
    hint: 'These ask before they run, even in Auto with the guard off.',
    badge: 'Asks',
  },
  {
    kind: 'path',
    title: 'Sensitive paths',
    hint: 'Files worth a question before they are read or written, wherever they are named.',
    badge: 'Asks',
  },
  {
    kind: 'secret',
    title: 'Secrets',
    hint: 'A key in a call’s arguments is one question before it goes into a file or over the network.',
    badge: 'Asks',
  },
];

const VARIANT: Record<GuardrailKind, 'bad' | 'warn'> = {
  deny: 'bad',
  confirm: 'warn',
  path: 'warn',
  secret: 'warn',
};

/**
 * Settings → Guard & guardrails (11 §2), the guardrail half. The judge that guards Auto mode
 * joins it with M8.
 *
 * What is stored here is the *difference* from the list the app ships with: which of its rules
 * are switched off, and which are the user's own. A copy of the whole list would freeze on the
 * day it was made, and the next release's new rule would never reach the machine that needed
 * it most.
 */
export function Guardrails() {
  const settings = useSettings();
  const info = useGuardrails();
  const update = useUpdateSettings();

  if (!isTauri()) {
    return (
      <p className="text-body text-fg-2">
        Not running inside the Gantry window, so there are no guardrails to edit. They show here in
        the app.
      </p>
    );
  }
  if (!settings.data || !info.data) return <p className="text-body text-fg-3">Loading…</p>;

  const g = guardrailDefaults(settings.data);
  const shipped = info.data.shipped;
  const problems = new Map(
    info.data.problems.map((p) => {
      const i = p.indexOf(': ');
      return [p.slice(0, i), p.slice(i + 2)] as const;
    }),
  );
  const patch = (next: Partial<typeof g>) => update.mutate({ guardrails: { ...g, ...next } });
  const toggle = (id: string, on: boolean) =>
    patch({ disabled: on ? g.disabled.filter((d) => d !== id) : [...g.disabled, id] });
  const remove = (id: string) => patch({ custom: g.custom.filter((r) => r.id !== id) });
  const add = (rule: GuardrailRule) => patch({ custom: [...g.custom, rule] });

  return (
    <div className="flex flex-col gap-8">
      <SettingsGroup title="The floor">
        <SettingsRow
          label="Guardrails"
          hint="A short list of things that stop or ask whatever the permission mode says. It is not a sandbox: a command that runs can do anything you can."
        >
          <Switch
            aria-label="Guardrails"
            checked={g.enabled}
            onCheckedChange={(v) => patch({ enabled: v })}
          />
        </SettingsRow>
        {!g.enabled && (
          <p className="py-3 text-body text-warn">
            Nothing is stopped and nothing is asked about beyond what the chat’s permission mode
            does. In Auto with the guard off, that means everything runs.
          </p>
        )}
      </SettingsGroup>

      {GROUPS.map(({ kind, title, hint, badge }) => {
        const theirs = shipped.filter((r) => r.kind === kind);
        const mine = g.custom.filter((r) => r.kind === kind);
        return (
          <SettingsGroup key={kind} title={title}>
            <p className="pb-2 text-meta text-fg-2">{hint}</p>
            {theirs.map((rule) => (
              <Rule
                key={rule.id}
                rule={rule}
                badge={badge}
                problem={problems.get(rule.id)}
                enabled={g.enabled && !g.disabled.includes(rule.id)}
                dimmed={!g.enabled}
                onToggle={(on) => toggle(rule.id, on)}
              />
            ))}
            {mine.map((rule) => (
              <Rule
                key={rule.id}
                rule={rule}
                badge={badge}
                problem={problems.get(rule.id)}
                enabled={g.enabled}
                dimmed={!g.enabled}
                onRemove={() => remove(rule.id)}
              />
            ))}
            <AddRule kind={kind} taken={[...shipped, ...g.custom].map((r) => r.id)} onAdd={add} />
          </SettingsGroup>
        );
      })}
    </div>
  );
}

/** One rule: its pattern, why it exists, and either a switch (ours) or a bin (theirs). */
function Rule({
  rule,
  badge,
  problem,
  enabled,
  dimmed,
  onToggle,
  onRemove,
}: {
  rule: GuardrailRule;
  badge: string;
  problem?: string;
  enabled: boolean;
  dimmed: boolean;
  onToggle?: (on: boolean) => void;
  onRemove?: () => void;
}) {
  return (
    <div className="flex min-h-(--row) items-center gap-3 py-2">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <code className="selectable min-w-0 truncate font-mono text-mono text-fg">
            {rule.pattern}
          </code>
          {enabled && !problem && <Badge variant={VARIANT[rule.kind]}>{badge}</Badge>}
          {problem && (
            <Badge variant="bad">
              <WarningIcon />
              Not valid
            </Badge>
          )}
        </div>
        <div className="text-meta text-fg-2">{problem ?? rule.reason}</div>
      </div>
      {onToggle && (
        <Switch
          aria-label={rule.id}
          disabled={dimmed}
          checked={enabled}
          onCheckedChange={onToggle}
        />
      )}
      {onRemove && (
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Remove ${rule.id}`}
          onClick={onRemove}
          className="text-fg-2"
        >
          <TrashIcon />
        </Button>
      )}
    </div>
  );
}

/** A rule of the user's own. The id is made from the reason, so the row has a name to show. */
function AddRule({
  kind,
  taken,
  onAdd,
}: {
  kind: GuardrailKind;
  taken: string[];
  onAdd: (rule: GuardrailRule) => void;
}) {
  const [pattern, setPattern] = useState('');
  const [reason, setReason] = useState('');
  const [asKind, setAsKind] = useState<GuardrailKind>(kind);
  const ready = pattern.trim().length > 0 && reason.trim().length > 0;

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    onAdd({
      id: freeId(reason, taken),
      kind: asKind,
      pattern: pattern.trim(),
      reason: reason.trim(),
    });
    setPattern('');
    setReason('');
    setAsKind(kind);
  };

  return (
    <form className="flex flex-wrap items-center gap-2 py-3" onSubmit={submit}>
      <Input
        value={pattern}
        onChange={(e) => setPattern(e.target.value)}
        placeholder={kind === 'path' ? '**/secrets/**' : 'a regular expression'}
        aria-label={`New ${kind} pattern`}
        className="min-w-40 flex-1 font-mono text-mono"
      />
      <Input
        value={reason}
        onChange={(e) => setReason(e.target.value)}
        placeholder="why it matters"
        aria-label={`Why the new ${kind} rule matters`}
        className="min-w-40 flex-1"
      />
      {kind === 'confirm' && (
        <Select
          value={asKind}
          onValueChange={(v) => v && setAsKind(v as GuardrailKind)}
          items={[
            { value: 'confirm', label: 'Asks' },
            { value: 'deny', label: 'Blocked' },
          ]}
        >
          <SelectTrigger aria-label="What the new rule does">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="confirm">Asks</SelectItem>
            <SelectItem value="deny">Blocked</SelectItem>
          </SelectContent>
        </Select>
      )}
      <Button type="submit" variant="secondary" disabled={!ready}>
        <PlusIcon />
        Add
      </Button>
    </form>
  );
}

/** A readable id that no other rule has taken. */
function freeId(reason: string, taken: string[]): string {
  const base =
    reason
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '')
      .split('-')
      .slice(0, 4)
      .join('-') || 'rule';
  if (!taken.includes(base)) return base;
  let n = 2;
  while (taken.includes(`${base}-${n}`)) n += 1;
  return `${base}-${n}`;
}
