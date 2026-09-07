import { type ReactNode, useEffect, useState } from 'react';

import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { TierLabel } from '@/components/gantry/TierLabel';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Kbd } from '@/components/ui/kbd';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { connectorName } from '@/fixtures/connectors';
import type { Permission } from '@/fixtures/types';

/**
 * The shared shell for every pending decision (04 §7, 15 §8): level 1, a 2 px accent bar,
 * a title with the connector mark, the request, a tier label, then the actions row.
 */
export function InteractionCard({
  mark,
  title,
  tier,
  children,
  actions,
}: {
  mark?: ReactNode;
  title: ReactNode;
  tier?: Permission['tier'];
  children?: ReactNode;
  actions: ReactNode;
}) {
  return (
    <div
      role="group"
      aria-label="Needs your decision"
      className="my-3 flex flex-col gap-3 rounded-3 border border-line-subtle border-l-2 border-l-accent bg-raised p-3 pl-4"
    >
      <div className="flex items-center gap-2">
        {mark}
        <div className="min-w-0 flex-1 truncate text-ui font-medium text-fg">{title}</div>
        {tier && <TierLabel tier={tier} />}
      </div>
      {children && <div className="text-body text-fg-2">{children}</div>}
      <div className="flex flex-wrap items-center gap-2">{actions}</div>
    </div>
  );
}

/** What the card reports back: allow with the chosen scope, or deny with an optional message. */
export type PermissionAnswer =
  { kind: 'allow'; scope: string } | { kind: 'deny'; message?: string };

/**
 * A permission prompt (04 §7). `Y` allows once and `N` denies while `hotkeys` is set (the
 * first pending card of the open chat); typing in a field never triggers them. Grants
 * ("for this chat") arrive with M7; until then the only scope is "once".
 */
export function PermissionCard({
  permission,
  onDecide,
  hotkeys = false,
  pending = false,
}: {
  permission: Permission;
  onDecide?: (answer: PermissionAnswer) => void;
  hotkeys?: boolean;
  /** A decision was sent and the card waits for the turn to move on. */
  pending?: boolean;
}) {
  const [scope, setScope] = useState(permission.scopes[0]?.id ?? 'once');
  const [denying, setDenying] = useState(false);
  const [message, setMessage] = useState('');
  const label = permission.scopes.find((s) => s.id === scope)?.label ?? 'Allow';
  const allow = () => onDecide?.({ kind: 'allow', scope });
  const deny = (text?: string) =>
    onDecide?.({ kind: 'deny', message: text?.trim() ? text.trim() : undefined });

  useEffect(() => {
    if (!hotkeys || !onDecide || pending) return;
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === 'y' || e.key === 'Y') {
        e.preventDefault();
        allow();
      } else if (e.key === 'n' || e.key === 'N') {
        e.preventDefault();
        deny();
      }
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  });

  const name = permission.connectorName ?? connectorName(permission.connector);
  return (
    <InteractionCard
      mark={<ConnectorMark id={permission.connector} name={name} />}
      title={permission.title}
      tier={permission.tier}
      actions={
        denying ? (
          <form
            className="flex w-full items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              deny(message);
            }}
          >
            <Input
              autoFocus
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              placeholder="Tell the model why (optional)"
              aria-label="Reason for denying"
              className="min-w-0 flex-1"
              onKeyDown={(e) => {
                if (e.key === 'Escape') setDenying(false);
              }}
            />
            <Button type="submit" variant="secondary" className="text-bad" disabled={pending}>
              Deny
            </Button>
            <Button type="button" variant="ghost" onClick={() => setDenying(false)}>
              Back
            </Button>
          </form>
        ) : (
          <>
            <Button variant="primary" onClick={allow} disabled={pending}>
              {label.replace(/ for this chat$/, '')}
              {hotkeys && <Kbd className="ml-1.5">Y</Kbd>}
            </Button>
            {permission.scopes.length > 1 && (
              <Select
                value={scope}
                onValueChange={(v) => setScope(v as string)}
                items={permission.scopes.map((s) => ({ value: s.id, label: s.label }))}
              >
                <SelectTrigger aria-label="Scope" className="min-w-0">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {permission.scopes.map((s) => (
                    <SelectItem key={s.id} value={s.id}>
                      {s.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
            <Button
              variant="ghost"
              className="ml-auto text-bad hover:bg-bad-subtle"
              onClick={() => deny()}
              disabled={pending}
            >
              Deny
              {hotkeys && <Kbd className="ml-1.5">N</Kbd>}
            </Button>
            <Button
              variant="ghost"
              className="text-fg-2"
              onClick={() => setDenying(true)}
              disabled={pending}
            >
              Deny with a message…
            </Button>
          </>
        )
      }
    >
      {Object.keys(permission.args).length > 0 ? (
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-ui">
          {Object.entries(permission.args).map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="font-mono text-mono text-fg-3">{k}</dt>
              <dd className="selectable min-w-0 truncate text-fg">{v}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p className="text-ui text-fg-3">No arguments.</p>
      )}
      {permission.why && (
        <p className="mt-2 text-meta text-fg-2">
          <span className="text-fg-3">Why: </span>
          {permission.why}
        </p>
      )}
      {permission.note && <p className="mt-2 text-meta text-fg-3">{permission.note}</p>}
    </InteractionCard>
  );
}
