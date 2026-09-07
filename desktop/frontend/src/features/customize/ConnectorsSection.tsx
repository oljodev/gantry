import { MagnifyingGlassIcon, PlusIcon } from '@phosphor-icons/react';
import { useMemo, useState } from 'react';

import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { EmptyState } from '@/components/gantry/EmptyState';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { connectors, type ConnectorEntry } from '@/fixtures/connectors';
import { cn } from '@/lib/utils';

type Tab = 'discover' | 'yours';

/**
 * Connectors inside the Customize dialog (03 §10, 15 A18): the catalog under **Discover**, what
 * you have installed under **Yours**, and the door to a server of your own. The rows read from
 * the fixture catalog until the real one lands with the install flow.
 */
export function ConnectorsSection({ onAddCustom }: { onAddCustom?: () => void }) {
  const [tab, setTab] = useState<Tab>('discover');
  const [query, setQuery] = useState('');

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return connectors.filter((c) => {
      if (tab === 'yours' && !c.installed) return false;
      return !q || c.name.toLowerCase().includes(q) || c.does.toLowerCase().includes(q);
    });
  }, [tab, query]);

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
        <Button variant="secondary" onClick={onAddCustom}>
          <PlusIcon />
          Add
        </Button>
      </div>

      <div role="tablist" aria-label="Connectors" className="mb-4 flex items-center gap-1">
        {(
          [
            ['discover', 'Discover'],
            ['yours', 'Your connectors'],
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

      {shown.length === 0 ? (
        <EmptyState
          className="mt-10"
          icon={<MagnifyingGlassIcon />}
          title={tab === 'yours' ? 'No connectors installed' : 'No connector matches'}
          hint={
            tab === 'yours'
              ? 'Install one from Discover, or add a server of your own.'
              : 'Try another word, or request the one you are missing.'
          }
          action={
            tab === 'yours' ? (
              <Button variant="secondary" onClick={() => setTab('discover')}>
                Discover connectors
              </Button>
            ) : (
              <Button variant="secondary" onClick={() => setQuery('')}>
                Clear search
              </Button>
            )
          }
        />
      ) : (
        <div className="grid grid-cols-1 gap-2 lg:grid-cols-2">
          {shown.map((c) => (
            <ConnectorRow key={c.id} connector={c} />
          ))}
        </div>
      )}

      <div className="mt-6 flex items-center justify-between gap-4 rounded-3 border border-line-subtle bg-base px-4 py-3">
        <div>
          <div className="text-ui font-medium text-fg">Missing one?</div>
          <div className="text-meta text-fg-2">
            Tell us which service you want to connect and what you would use it for.
          </div>
        </div>
        <Button variant="secondary">Request a connector</Button>
      </div>
    </>
  );
}

/** One catalog row: mark, name, what it does, and what it would take to use it. */
function ConnectorRow({ connector }: { connector: ConnectorEntry }) {
  return (
    <div className="flex items-start gap-3 rounded-3 border border-line-subtle bg-raised p-3">
      <span className="flex size-8 shrink-0 items-center justify-center rounded-2 bg-inset">
        <ConnectorMark id={connector.id} name={connector.name} size={18} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-ui font-medium text-fg">{connector.name}</div>
        <div className="line-clamp-2 text-meta text-fg-2">{connector.does}</div>
        {/* A badge sits under the line rather than beside the name, which it would squeeze out. */}
        {connector.status === 'needs_reconnect' && (
          <Badge variant="warn" className="mt-1.5">
            Reconnect
          </Badge>
        )}
        {connector.status === 'runtime_missing' && (
          <Badge variant="warn" className="mt-1.5">
            Runtime missing
          </Badge>
        )}
      </div>
      <Button variant={connector.installed ? 'ghost' : 'secondary'} size="sm">
        {connector.installed ? 'Installed' : 'Install'}
      </Button>
    </div>
  );
}
