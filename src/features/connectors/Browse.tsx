import { MagnifyingGlassIcon, PlusIcon } from '@phosphor-icons/react';
import { useMemo, useState } from 'react';

import { ConnectorTile } from '@/components/gantry/connectors/ConnectorTile';
import { EmptyState } from '@/components/gantry/EmptyState';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { CATEGORIES, type ConnectorCategory, connectors } from '@/fixtures/connectors';
import { cn } from '@/lib/utils';

type Filter = 'all' | 'installed' | ConnectorCategory;

/**
 * Browse (03 §10, 15 §7): title, search, category pills, count, a grid of tiles, and the
 * request-a-connector entry the plan asks for in the app (03 §11). Detail and install arrive in M9.
 */
export function Browse() {
  const [filter, setFilter] = useState<Filter>('all');
  const [query, setQuery] = useState('');

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return connectors.filter((c) => {
      if (filter === 'installed' && !c.installed) return false;
      if (filter !== 'all' && filter !== 'installed' && c.category !== filter) return false;
      return !q || c.name.toLowerCase().includes(q) || c.does.toLowerCase().includes(q);
    });
  }, [filter, query]);

  const pills: [Filter, string][] = [
    ['all', 'All'],
    ['installed', 'Installed'],
    ...CATEGORIES.map((c) => [c.id, c.label] as [Filter, string]),
  ];

  return (
    <div className="h-full overflow-y-auto pt-(--title-strip)">
      <div className="mx-auto flex w-full max-w-5xl flex-col px-6 pt-8 pb-8">
        <div className="flex items-start justify-between gap-6">
          <div>
            <h1 className="text-page font-semibold text-fg">Connectors</h1>
            <p className="mt-1 text-body text-fg-2">
              Give the agent tools: your files, your services, any MCP server. Nothing is installed
              until you say so.
            </p>
          </div>
          <Button variant="secondary">
            <PlusIcon />
            Add custom server
          </Button>
        </div>

        <div className="mt-6 flex flex-wrap items-center gap-2">
          <div className="relative w-64">
            <MagnifyingGlassIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-fg-3" />
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search connectors…"
              aria-label="Search connectors"
              className="pl-8"
            />
          </div>
          <div
            role="radiogroup"
            aria-label="Category"
            className="flex flex-wrap items-center gap-1"
          >
            {pills.map(([id, label]) => (
              <button
                key={id}
                type="button"
                role="radio"
                aria-checked={filter === id}
                onClick={() => setFilter(id)}
                className={cn(
                  'h-(--control-sm) rounded-full border px-2.5 text-meta transition-colors duration-(--dur-1)',
                  filter === id
                    ? 'border-fg bg-fg text-surface'
                    : 'border-line text-fg-2 hover:border-line-strong hover:text-fg',
                )}
              >
                {label}
              </button>
            ))}
          </div>
          <span className="ml-auto text-meta text-fg-3 tnum">
            {shown.length} of {connectors.length}
          </span>
        </div>

        {shown.length === 0 ? (
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
          <div className="mt-5 grid grid-cols-2 gap-3 md:grid-cols-3 lg:grid-cols-4">
            {shown.map((c) => (
              <ConnectorTile key={c.id} connector={c} />
            ))}
          </div>
        )}

        <div className="mt-8 flex items-center justify-between gap-4 rounded-3 border border-line-subtle bg-base px-4 py-3">
          <div>
            <div className="text-ui font-medium text-fg">Missing one?</div>
            <div className="text-meta text-fg-2">
              Tell us which service you want to connect and what you would use it for.
            </div>
          </div>
          <Button variant="secondary">Request a connector</Button>
        </div>
      </div>
    </div>
  );
}
