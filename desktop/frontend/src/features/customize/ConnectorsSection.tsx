import {
  ArrowClockwiseIcon,
  MagnifyingGlassIcon,
  PlusIcon,
  SlidersHorizontalIcon,
  TerminalIcon,
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
import type { CatalogEntryDto, ConnectorInstanceDto, InstanceId } from '@/bindings';
import { AddCustomServer } from '@/features/connectors/AddCustomServer';
import { effectiveValues, missingRequired } from '@/features/connectors/config';
import { ConfigForm } from '@/features/connectors/ConfigForm';
import { useInstallFlow } from '@/features/connectors/install';
import { InstallDialog } from '@/features/connectors/InstallDialog';
import { isTauri } from '@/lib/ipc/client';
import {
  useCatalog,
  useConnectorConfig,
  useConnectorLogs,
  useConnectorMutations,
  useConnectors,
} from '@/lib/ipc/hooks/connectors';
import { describe } from '@/lib/errors';
import { useUiStore } from '@/lib/stores/uiStore';
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
  /** The entry whose install needs something from the user; the fallback dialog, not the path. */
  const [asking, setAsking] = useState<{ entry: CatalogEntryDto; reason?: string } | null>(null);
  const [adding, setAdding] = useState(false);
  const catalog = useCatalog();
  const connectors = useConnectors();
  const { connect, setEnabled, remove } = useConnectorMutations();
  const { runInstall: install, busyId: busy } = useInstallFlow();

  // Something opened this section *at* a connector — the palette, so far. Seed the search with
  // it and stand on the tab where that connector can actually be acted on, whether the dialog
  // was already open or is opening now. Adjusted during render, which is how the composer's
  // prefill nonce does the same thing.
  const find = useUiStore((s) => s.customizeFind);
  const [applied, setApplied] = useState<string | null>(find);
  if (find !== applied) {
    setApplied(find);
    if (find !== null) {
      setQuery(find);
      const owned = (connectors.data ?? []).some((i) =>
        i.name.toLowerCase().includes(find.toLowerCase()),
      );
      setTab(owned ? 'yours' : 'discover');
    }
  }

  /** The one-click install, with the dialog as the fallback when a server needs more (03 §11). */
  const runInstall = async (entry: CatalogEntryDto) => {
    try {
      await install(entry);
    } catch (err) {
      setAsking({ entry, reason: describe(err) });
    }
  };

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
              <CatalogRow
                key={entry.id}
                entry={entry}
                busy={busy === entry.id}
                onInstall={() => void runInstall(entry)}
              />
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
                if (entry) void runInstall(entry);
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

      {asking && (
        <InstallDialog
          entry={asking.entry}
          instance={(connectors.data ?? []).find((i) => i.catalog_id === asking.entry.id)}
          reason={asking.reason}
          onClose={() => setAsking(null)}
        />
      )}
      {adding && <AddCustomServer onClose={() => setAdding(false)} />}
    </>
  );
}

/** One catalog entry: what it is, what it needs, and the one button that installs it. */
function CatalogRow({
  entry,
  busy,
  onInstall,
}: {
  entry: CatalogEntryDto;
  busy: boolean;
  onInstall: () => void;
}) {
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
      <Button
        variant={installed ? 'ghost' : 'secondary'}
        size="sm"
        disabled={busy}
        onClick={onInstall}
      >
        {busy ? 'Connecting…' : installed ? 'Installed' : 'Install'}
      </Button>
    </div>
  );
}

/**
 * The `user_config` answers for a connector that is already installed (docs/plan/03 §11 step 2).
 *
 * The form existed only inside the install dialog, which left two states it could not reach: a
 * connector installed *before* its manifest asked for anything, and one whose answer has to
 * change later. `web` was the first of both — it shipped as a manifest with no fields, gained a
 * bring-your-own-key search key, and the key had nowhere to be typed. The tool was built, the
 * vault held it, and no interface could turn it on.
 *
 * Only rendered when the manifest asks for something, so an ordinary connector's row is
 * unchanged, and the query runs when a row is opened rather than once per row in the list. A
 * server added by hand has no manifest and so no form; the caller checks that before mounting
 * this.
 */
function ConnectorSettings({ instance }: { instance: ConnectorInstanceDto }) {
  const form = useConnectorConfig(instance.catalog_id, instance.id);
  const { setConfig } = useConnectorMutations();
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fields = form.data?.fields ?? [];
  const stored = form.data?.values ?? {};
  const values = effectiveValues(fields, stored, answers);
  // A sensitive answer is never sent back (06 §5), so "has been configured" is judged by the
  // public answers — which is also what tells the form that an empty secret means "keep".
  const configured = Object.keys(stored).length > 0;
  const missing = missingRequired(fields, values, configured);
  const dirty = Object.keys(answers).length > 0;

  if (fields.length === 0) return null;

  const save = async () => {
    setError(null);
    try {
      await setConfig.mutateAsync({ instanceId: instance.id, values });
      setAnswers({});
      setSaved(true);
    } catch (err) {
      setError(describe(err));
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-1.5 text-meta text-fg-3">
        <SlidersHorizontalIcon className="size-3.5" />
        Settings
      </div>
      <ConfigForm
        fields={fields}
        values={values}
        hasSaved={configured}
        onChange={(key, value) => {
          setSaved(false);
          setAnswers((a) => ({ ...a, [key]: value }));
        }}
      />
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          disabled={!dirty || missing.length > 0 || setConfig.isPending}
          onClick={() => void save()}
        >
          {setConfig.isPending ? 'Saving…' : 'Save'}
        </Button>
        {missing.length > 0 && (
          <span className="text-meta text-fg-3">{missing.join(', ')} still needed</span>
        )}
        {saved && !dirty && <span className="text-meta text-fg-2">Saved</span>}
      </div>
      {error && <span className="selectable text-meta text-bad">{error}</span>}
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
  const [log, setLog] = useState(false);
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
          {/* Settings first: what a connector stores for you is the part you came here to
              change, where its namespace and its tool list are the part you came to read. */}
          {instance.catalog_id && <ConnectorSettings instance={instance} />}
          <code className="selectable mt-3 block break-all font-mono text-mono text-fg-3">
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
            {/* Only a local process has a stderr to show; a remote server explains itself
                over HTTP, and its answer is already in `last_error`. */}
            {instance.kind === 'mcp-stdio' && (
              <Button variant="secondary" size="sm" onClick={() => setLog((l) => !l)}>
                <TerminalIcon />
                {log ? 'Hide log' : 'Show log'}
              </Button>
            )}
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
          {log && <ServerLog instanceId={instance.id} />}
        </div>
      )}
    </div>
  );
}

/**
 * The server's own words. A stdio server talks MCP on stdout, so everything it wants a person to
 * read goes to stderr — and when a spawn fails, that is the only place the reason exists.
 */
function ServerLog({ instanceId }: { instanceId: InstanceId }) {
  const logs = useConnectorLogs(instanceId);
  const lines = logs.data ?? [];
  return (
    <div className="mt-3">
      <div className="flex items-center justify-between pb-1">
        <span className="text-micro font-medium uppercase tracking-[0.04em] text-fg-3">stderr</span>
        <Button variant="ghost" size="sm" onClick={() => void logs.refetch()}>
          <ArrowClockwiseIcon />
          Refresh
        </Button>
      </div>
      {lines.length === 0 ? (
        <p className="text-meta text-fg-3">
          Nothing yet. A server that has not started, or one that started cleanly, writes nothing
          here.
        </p>
      ) : (
        <pre className="selectable max-h-64 overflow-auto rounded-2 border border-line-subtle bg-inset px-3 py-2 font-mono text-mono text-fg-2">
          {lines.join('\n')}
        </pre>
      )}
    </div>
  );
}
