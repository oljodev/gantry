import type { ReactNode } from 'react';

import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { TierLabel } from '@/components/gantry/TierLabel';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { connectorName } from '@/fixtures/connectors';
import type { Permission } from '@/fixtures/types';
import { useState } from 'react';

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

export function PermissionCard({
  permission,
  onDecide,
}: {
  permission: Permission;
  onDecide?: (scope: string | 'deny') => void;
}) {
  const [scope, setScope] = useState(permission.scopes[0]?.id ?? 'once');
  const label = permission.scopes.find((s) => s.id === scope)?.label ?? 'Allow';
  return (
    <InteractionCard
      mark={<ConnectorMark id={permission.connector} name={connectorName(permission.connector)} />}
      title={permission.title}
      tier={permission.tier}
      actions={
        <>
          <Button variant="primary" onClick={() => onDecide?.(scope)}>
            {label.replace(/ for this chat$/, '')}
          </Button>
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
          <Button
            variant="ghost"
            className="ml-auto text-bad hover:bg-bad-subtle"
            onClick={() => onDecide?.('deny')}
          >
            Deny
          </Button>
        </>
      }
    >
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-ui">
        {Object.entries(permission.args).map(([k, v]) => (
          <div key={k} className="contents">
            <dt className="font-mono text-mono text-fg-3">{k}</dt>
            <dd className="selectable min-w-0 truncate text-fg">{v}</dd>
          </div>
        ))}
      </dl>
      {permission.note && <p className="mt-2 text-meta text-fg-3">{permission.note}</p>}
    </InteractionCard>
  );
}
