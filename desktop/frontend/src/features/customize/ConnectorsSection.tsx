import {
  ArrowClockwiseIcon,
  MagnifyingGlassIcon,
  PlusIcon,
  TrashIcon,
  WarningCircleIcon,
} from '@phosphor-icons/react';
import { useMemo, useState } from 'react';

import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { EmptyState } from '@/components/gantry/EmptyState';
import { TierLabel } from '@/components/gantry/TierLabel';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import type { CatalogEntryDto, ConnectorInstanceDto } from '@/bindings';
import { AddCustomServer } from '@/features/connectors/AddCustomServer';
import { InstallDialog } from '@/features/connectors/InstallDialog';
import { isTauri } from '@/lib/ipc/client';
import { useCatalog, useConnectorMutations, useConnectors } from '@/lib/ipc/hooks/connectors';
import { cn } from '@/lib/utils';

type Tab = 'discover' | 'yours';

/**
 * Connectors inside the Customize dialog (docs/plan/03 §10, 15 A18): the catalog under
 * **Discover**, what is installed under **Your connectors**, and the door to a server of your
 * own. Nothing installs itself: every row here waits for a click (03 §11).
 */
export function ConnectorsSection() {
  const [tab, setTab] = useState<Tab>('discover');
  const [query, setQuery] = useState('');
  const [installing, setInstalling] = useState<CatalogEntryDto | null>(null);
  const [adding, setAdding] = useState(false);
  const catalog = useCatalog();
  const connectors = useConnectors();
  const { connect, setEnabled, remove } = useConnectorMutations();

  const entries = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (catalog.data ?? []).filter(
      (c) =>
        !q ||
        c.name.toLowerCase().includes(q) ||
        c.description.toLowerCase().includes(q) ||
        c.keywords.some((k) => k.toLowerCase().includes(q)),
    );
  }, [catalog.data, query]);

  const installed = useMemo(() => {
    const q = query.trim().toLowerCase();
    return (connectors.data ?? []).filter((i) => !q || i.name.toLowerCase().includes(q));
  }, [connectors.data, query]);

  return (
    <>
      <div className="mb-4 flex items-center gap-3">
        <h2 className="text-title font-medium text-fg">Connectors</h2>
        <div className="relative ml-auto w-56">
          <MagnifyingGlassIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-fg-3" />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search connectors"
            aria-label="Search connectors"
            className="h-(--control-md) w-full rounded-2 border border-line bg-surface pr-2 pl-8 text-ui text-fg placeholder:text-fg-3 focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-focus"
          />
        </div>
        <Button variant="secondary" onClick={() => setAdding(true)} disabled={!isTauri()}>
          <PlusIcon />
          Add
        </Button>
      </div>

      <div role="tablist" aria-label="Connectors" className="mb-4 flex items-center gap-1">
        {(
          [
            ['discover', 'Discover'],
            ['yours', `Your connectors${installed.length > 0 ? ` (${installed.length})` : ''}`],
          ] as [Tab, string][]
        ).map(([id, label]) => (
          <button
            key={id}
            type="button"
            role="tab"
            aria-selected={tab === id}
            onClick={() => setTab(id)}
            className={cn(
              'h-(--control-sm) rounded-full px-3 text-meta transition-colors duration-(--dur-1)',
              tab === id ? 'bg-selected text-fg' : 'text-fg-2 hover:bg-hover hover:text-fg',
            )}
          >
            {label}
          </button>
        ))}
      </div>

      {!isTauri() ? (
        <p className="text-body text-fg-2">
          Not running inside the Gantry window, so there is no catalog to read. Connectors show here
          in the app.
        </p>
      ) : tab === 'discover' ? (
        entries.length === 0 ? (
          <EmptyState
            className="mt-10"
            icon={<MagnifyingGlassIcon />}
            title="No connector matches"
            hint="Try another word, or request the one you are missing."
            action={
              <Button variant="secondary" onClick={() => setQuery('')}>
                Clear search
              </Button>
            }
          />
        ) : (
          <div className="grid grid-cols-1 gap-2 lg:grid-cols-2">
            {entries.map((entry) => (
              <CatalogRow key={entry.id} entry={entry} onInstall={() => setInstalling(entry)} />
            ))}
          </div>
        )
      ) : installed.length === 0 ? (
        <EmptyState
          className="mt-10"
          icon={<MagnifyingGlassIcon />}
          title="No connectors installed"
          hint="Install one from Discover, or add a server of your own."
          action={
            <Button variant="secondary" onClick={() => setTab('discover')}>
              Discover connectors
            </Button>
          }
        />
      ) : (
        <div className="flex flex-col gap-2">
          {installed.map((instance) => (
            <InstalledRow
              key={instance.id}
              instance={instance}
              onReconnect={() => connect.mutate(instance.id)}
              onToggle={(enabled) => setEnabled.mutate({ instanceId: instance.id, enabled })}
              onRemove={() => remove.mutate(instance.id)}
              onFinishSetup={() => {
                const entry = (catalog.data ?? []).find((c) => c.id === instance.catalog_id);
                if (entry) setInstalling(entry);
              }}
            />
          ))}
        </div>
      )}

      <div className="mt-6 flex items-center justify-between gap-4 rounded-3 border border-line-subtle bg-base px-4 py-3">
        <div>
          <div className="text-ui font-medium text-fg">Missing one?</div>
          <div className="text-meta text-fg-2">
            Any MCP server works: add it with the URL or the command that starts it.
          </div>
        </div>
        <Button variant="secondary" onClick={() => setAdding(true)} disabled={!isTauri()}>
          Add a server
        </Button>
      </div>

      {installing && (
        <InstallDialog
          entry={installing}
          instance={(connectors.data ?? []).find((i) => i.catalog_id === installing.id)}
          onClose={() => setInstalling(null)}
        />
      )}
      {adding && <AddCustomServer onClose={() => setAdding(false)} />}
    </>
  );
}

/** One catalog entry: what it is, what it needs, and the one button that installs it. */
function CatalogRow({ entry, onInstall }: { entry: CatalogEntryDto; onInstall: () => void }) {
  const installed = entry.installed.length > 0;
  return (
    <div className="flex items-start gap-3 rounded-3 border border-line-subtle bg-raised p-3">
      <span className="flex size-8 shrink-0 items-center justify-center rounded-2 bg-inset">
        <ConnectorMark id={entry.id} name={entry.name} size={18} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-ui font-medium text-fg">{entry.name}</div>
        <div className="line-clamp-2 text-meta text-fg-2">{entry.description}</div>
        <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
          <Badge variant="outline">{entry.kind === 'mcp-remote' ? 'Remote' : 'Local'}</Badge>
          {entry.auth !== 'none' && <Badge variant="neutral">Sign in</Badge>}
          {entry.requires.map((r) => (
            <Badge key={r.name} variant="warn">
              {r.name} {r.version}
            </Badge>
          ))}
        </div>
      </div>
      <Button variant={installed ? 'ghost' : 'secondary'} size="sm" onClick={onInstall}>
        {installed ? 'Installed' : 'Install'}
      </Button>
    </div>
  );
}

/** One installed instance: its state, its tools, and what to do when it stops working. */
function InstalledRow({
  instance,
  onReconnect,
  onToggle,
  onRemove,
  onFinishSetup,
}: {
  instance: ConnectorInstanceDto;
  onReconnect: () => void;
  onToggle: (enabled: boolean) => void;
  onRemove: () => void;
  onFinishSetup: () => void;
}) {
  const [open, setOpen] = useState(false);
  const needsSetup =
    instance.auth !== 'none' &&
    (instance.auth_state === 'unconfigured' || instance.auth_state === 'expired');
  return (
    <div className="rounded-3 border border-line-subtle bg-raised">
      <div className="flex items-start gap-3 p-3">
        <span className="flex size-8 shrink-0 items-center justify-center rounded-2 bg-inset">
          <ConnectorMark
            id={instance.catalog_id ?? instance.namespace}
            name={instance.name}
            size={18}
          />
        </span>
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          className="min-w-0 flex-1 text-left"
          aria-expanded={open}
        >
          <div className="truncate text-ui font-medium text-fg">{instance.name}</div>
          <div className="truncate text-meta text-fg-2">
            {instance.tools.length > 0
              ? `${instance.tools.length} tool${instance.tools.length === 1 ? '' : 's'}`
              : 'No tools yet'}
            {instance.server && ` · ${instance.server.name} ${instance.server.version}`}
            {instance.server && ` · MCP ${instance.server.protocol}`}
          </div>
        </button>
        <div className="flex shrink-0 items-center gap-1.5">
          {needsSetup && (
            <Button size="sm" onClick={onFinishSetup}>
              {instance.auth_state === 'expired' ? 'Reconnect' : 'Connect'}
            </Button>
          )}
          {instance.auth_state === 'error' && <Badge variant="warn">Error</Badge>}
          <Switch
            checked={instance.enabled}
            onCheckedChange={onToggle}
            aria-label={`Enable ${instance.name}`}
          />
        </div>
      </div>
      {instance.last_error && (
        <div className="flex items-start gap-2 border-t border-line-subtle px-3 py-2 text-meta text-bad">
          <WarningCircleIcon className="mt-0.5 size-3.5 shrink-0" />
          <span className="selectable">{instance.last_error}</span>
        </div>
      )}
      {open && (
        <div className="border-t border-line-subtle p-3">
          <code className="selectable block break-all font-mono text-mono text-fg-3">
            {instance.namespace}
          </code>
          {instance.tools.length > 0 && (
            <ul className="mt-2 flex flex-col gap-1">
              {instance.tools.map((tool) => (
                <li key={tool.name} className="flex items-baseline gap-2">
                  <code className="font-mono text-mono text-fg">{tool.name}</code>
                  <TierLabel tier={tool.tier} />
                  <span className="min-w-0 flex-1 truncate text-meta text-fg-2">
                    {tool.description}
                  </span>
                </li>
              ))}
            </ul>
          )}
          <div className="mt-3 flex items-center gap-2">
            <Button variant="secondary" size="sm" onClick={onReconnect}>
              <ArrowClockwiseIcon />
              Refresh tools
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="text-bad hover:bg-bad-subtle"
              onClick={onRemove}
            >
              <TrashIcon />
              Remove
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
