import { ArrowsClockwiseIcon, CaretDownIcon, PlusIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import type {
  ErrorDto,
  KeyStatus as KeyStatusDto,
  ProviderRow as ProviderRowDto,
} from '@/bindings';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { toast } from '@/components/ui/toast';
import { isTauri } from '@/lib/ipc/client';
import { useModelCatalog, useProviderMutations, useProviders } from '@/lib/ipc/hooks/providers';
import { useSecretStoreStatus } from '@/lib/ipc/hooks/settings';
import { cn } from '@/lib/utils';

/** What a key looks like, so the field hints at the right one (never validated). */
const KEY_HINT: Record<string, string> = {
  openrouter: 'sk-or-…',
  anthropic: 'sk-ant-…',
  openai: 'sk-…',
  google: 'AIza…',
  xai: 'xai-…',
};

function describe(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err)
    return String((err as { message: unknown }).message);
  return String(err);
}

function money(v: number | null | undefined) {
  return v === null || v === undefined ? '—' : `$${v.toFixed(v < 1 ? 3 : 2)}`;
}

/** Settings → Providers & models (11 §4): a row per provider, keys write-only, models below. */
export function Providers() {
  const providers = useProviders();
  const store = useSecretStoreStatus();
  const catalog = useModelCatalog();
  const { setKey, clearKey, test, refreshModels, update, addCustom, remove } =
    useProviderMutations();
  const [adding, setAdding] = useState<ProviderRowDto | null>(null);
  const [addingEndpoint, setAddingEndpoint] = useState(false);

  if (!isTauri()) {
    return (
      <p className="text-body text-fg-2">
        Not running inside the Gantry window, so there is no backend to hold a key. Providers show
        here in the app.
      </p>
    );
  }

  const rows = providers.data ?? [];

  const onTest = async (p: ProviderRowDto) => {
    const r = await test.mutateAsync(p.id);
    if (r.ok) {
      const i = r.info;
      toast.add({
        title: `${p.label}: key works`,
        description: i
          ? [
              i.label && `Key "${i.label}"`,
              `used ${money(i.usage_usd)}`,
              i.limit_usd !== null && `limit ${money(i.limit_usd)}`,
            ]
              .filter(Boolean)
              .join(' · ')
          : undefined,
        type: 'success',
      });
    } else {
      toast.add({
        title: `${p.label}: ${errorTitle(r.error)}`,
        description: r.error?.message,
        type: 'error',
      });
    }
  };

  const onRefresh = async () => {
    for (const p of rows.filter((r) => r.key.present && r.available)) {
      try {
        const models = await refreshModels.mutateAsync(p.id);
        toast.add({ title: `${p.label}: ${models.length} models`, type: 'success' });
      } catch (err) {
        toast.add({
          title: `${p.label}: could not refresh`,
          description: describe(err),
          type: 'error',
        });
      }
    }
  };

  return (
    <div className="flex flex-col gap-8">
      <section>
        <p className="mb-4 text-body text-fg-2">
          Bring your own keys. They are encrypted with a key held by your operating system and never
          shown again after you save them.
        </p>
        {store.data?.kind === 'file_fallback' && (
          <p className="mb-4 rounded-3 border border-warn/30 bg-warn-subtle px-3 py-2 text-ui text-warn">
            No system keyring answered, so the master key lives in a file only your user can read:{' '}
            <code className="selectable text-mono">{store.data.path}</code>
          </p>
        )}
        <div className="divide-y divide-line-subtle border-y border-line-subtle">
          {rows.map((p) => (
            <ProviderRow
              key={p.id}
              provider={p}
              busy={test.isPending && test.variables === p.id}
              onAdd={() => setAdding(p)}
              onClear={() => {
                clearKey.mutate(p.id, {
                  onSuccess: () => toast.add({ title: `${p.label} key removed`, type: 'success' }),
                  onError: (err) =>
                    toast.add({
                      title: 'Could not remove the key',
                      description: describe(err),
                      type: 'error',
                    }),
                });
              }}
              onTest={() => void onTest(p)}
              onRemoveEndpoint={
                p.custom
                  ? () =>
                      remove.mutate(p.id, {
                        onSuccess: () =>
                          toast.add({ title: `${p.label} removed`, type: 'success' }),
                        onError: (err) =>
                          toast.add({
                            title: 'Could not remove the endpoint',
                            description: describe(err),
                            type: 'error',
                          }),
                      })
                  : undefined
              }
            />
          ))}
        </div>
        <Button variant="ghost" size="sm" className="mt-2" onClick={() => setAddingEndpoint(true)}>
          <PlusIcon />
          Add custom endpoint
        </Button>
        {store.data?.kind === 'os_store' && (
          <p className="mt-3 text-meta text-fg-3">Master key in {store.data.backend}.</p>
        )}
      </section>

      <section>
        <div className="flex items-center justify-between">
          <h2 className="text-title font-medium text-fg">Models</h2>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void onRefresh()}
            disabled={refreshModels.isPending}
          >
            <ArrowsClockwiseIcon className={cn(refreshModels.isPending && 'animate-spin')} />
            Refresh lists
          </Button>
        </div>
        <p className="mt-1 text-meta text-fg-2">
          From the providers with a key. Context window and prices per million tokens where known.
        </p>
        <ModelTable catalog={catalog.providers} />
      </section>

      <AddEndpointDialog
        open={addingEndpoint}
        onClose={() => setAddingEndpoint(false)}
        onSave={async (label, baseUrl) => {
          try {
            await addCustom.mutateAsync({ label, base_url: baseUrl });
            toast.add({
              title: `${label} added`,
              description: 'Add a key if it needs one.',
              type: 'success',
            });
          } catch (err) {
            toast.add({
              title: 'Could not add the endpoint',
              description: describe(err),
              type: 'error',
            });
          }
        }}
      />
      <AddKeyDialog
        provider={adding}
        onClose={() => setAdding(null)}
        onSave={async (key, baseUrl) => {
          if (!adding) return;
          try {
            if (baseUrl !== undefined && baseUrl !== (adding.base_url ?? '')) {
              await update.mutateAsync({
                providerId: adding.id,
                update: { base_url: baseUrl, default_model: null },
              });
            }
            const status = await setKey.mutateAsync({ providerId: adding.id, key });
            toast.add({
              title: 'Key saved',
              description: `${adding.label} · ····${status.hint ?? ''}`,
              type: 'success',
            });
          } catch (err) {
            toast.add({
              title: 'Could not save the key',
              description: describe(err),
              type: 'error',
            });
          }
        }}
      />
    </div>
  );
}

function errorTitle(err: ErrorDto | null): string {
  if (!err) return 'test failed';
  if (err.kind === 'provider') {
    switch (err.provider_kind) {
      case 'auth':
        return 'invalid key';
      case 'insufficient_credits':
        return 'no credit left';
      case 'network':
        return 'network error';
      case 'rate_limited':
        return 'rate limited';
      default:
        return 'test failed';
    }
  }
  return 'test failed';
}

function ModelTable({ catalog }: { catalog: ReturnType<typeof useModelCatalog>['providers'] }) {
  const rows = catalog.flatMap((p) => p.models.map((m) => ({ p, m })));
  if (rows.length === 0) {
    return <p className="mt-3 text-meta text-fg-3">No models yet. Add a key, then refresh.</p>;
  }
  return (
    <div className="mt-3 max-h-96 overflow-y-auto">
      <table className="w-full border-collapse text-ui">
        <thead className="sticky top-0 bg-surface">
          <tr className="text-left text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
            <th className="py-1.5 pr-3 font-medium">Model</th>
            <th className="py-1.5 pr-3 font-medium">Provider</th>
            <th className="py-1.5 pr-3 text-right font-medium">Context</th>
            <th className="py-1.5 text-right font-medium">In / out</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-line-subtle">
          {rows.map(({ p, m }) => (
            <tr key={`${p.id}/${m.id}`} className="h-(--row)">
              <td className="pr-3 text-fg">
                <span className="block truncate" title={m.id}>
                  {m.display_name}
                </span>
              </td>
              <td className="pr-3 text-fg-2">{p.label}</td>
              <td className="pr-3 text-right text-fg-2 tnum">{contextLabel(m.context_window)}</td>
              <td className="text-right text-fg-2 tnum">
                {m.pricing
                  ? `${price(m.pricing.input_per_mtok)} / ${price(m.pricing.output_per_mtok)}`
                  : '—'}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function contextLabel(n: number | null) {
  if (n === null) return '—';
  return n >= 1_000_000
    ? `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`
    : `${Math.round(n / 1000)}K`;
}

function price(v: number | null) {
  if (v === null) return '—';
  return v === 0 ? 'free' : `$${v < 1 ? v.toFixed(2) : v.toFixed(v < 10 ? 2 : 0)}`;
}

function ProviderRow({
  provider,
  busy,
  onAdd,
  onClear,
  onTest,
  onRemoveEndpoint,
}: {
  provider: ProviderRowDto;
  busy: boolean;
  onAdd: () => void;
  onClear: () => void;
  onTest: () => void;
  onRemoveEndpoint?: () => void;
}) {
  const { key } = provider;
  return (
    <div className="flex min-h-(--row) items-center gap-4 py-2.5">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-ui font-medium text-fg">{provider.label}</span>
          {provider.custom && <Badge>Custom</Badge>}
          {!provider.available && <Badge variant="bad">No client</Badge>}
          <KeyStatus status={key} />
        </div>
        {provider.base_url && (
          <div className="mt-0.5 font-mono text-mono text-fg-3">{provider.base_url}</div>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-1">
        {key.present ? (
          <>
            <Button variant="ghost" size="sm" onClick={onTest} disabled={busy}>
              {busy ? 'Testing…' : 'Test'}
            </Button>
            <Button variant="secondary" size="sm" onClick={onAdd}>
              Replace key
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="text-bad hover:bg-bad-subtle"
              onClick={onClear}
            >
              Remove
            </Button>
          </>
        ) : (
          <>
            {provider.custom && (
              <Button variant="ghost" size="sm" onClick={onTest} disabled={busy}>
                {busy ? 'Testing…' : 'Test'}
              </Button>
            )}
            <Button variant="secondary" size="sm" onClick={onAdd}>
              Add key
            </Button>
          </>
        )}
        {onRemoveEndpoint && (
          <Button
            variant="ghost"
            size="sm"
            className="text-bad hover:bg-bad-subtle"
            onClick={onRemoveEndpoint}
          >
            Remove endpoint
          </Button>
        )}
      </div>
    </div>
  );
}

/** A user-supplied OpenAI-compatible endpoint (11 §4): Ollama, LM Studio, a proxy. */
function AddEndpointDialog({
  open,
  onClose,
  onSave,
}: {
  open: boolean;
  onClose: () => void;
  onSave: (label: string, baseUrl: string) => Promise<void>;
}) {
  const [label, setLabel] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [saving, setSaving] = useState(false);
  const valid = label.trim().length > 0 && /^https?:\/\/\S+$/.test(baseUrl.trim());
  const close = () => {
    setLabel('');
    setBaseUrl('');
    onClose();
  };
  const save = async () => {
    if (!valid) return;
    setSaving(true);
    try {
      await onSave(label.trim(), baseUrl.trim());
    } finally {
      setSaving(false);
      close();
    }
  };
  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add a custom endpoint</DialogTitle>
          <DialogDescription>
            Any OpenAI-compatible Chat Completions server: Ollama, LM Studio, a proxy. A key is
            optional and can be added afterwards.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-ui font-medium text-fg">Name</span>
            <Input
              autoFocus
              value={label}
              onChange={(e) => setLabel(e.target.value)}
              placeholder="Local Ollama"
            />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-ui font-medium text-fg">Base URL</span>
            <Input
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && valid) void save();
              }}
              placeholder="http://localhost:11434/v1"
              className="font-mono"
            />
          </label>
        </div>
        <DialogFooter>
          <DialogClose render={<Button variant="secondary" />}>Close</DialogClose>
          <Button variant="primary" disabled={!valid || saving} onClick={() => void save()}>
            {saving ? 'Adding…' : 'Add endpoint'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function KeyStatus({ status }: { status: KeyStatusDto }) {
  if (!status.present) return <Badge>No key</Badge>;
  if (status.invalid) return <Badge variant="bad">Invalid ····{status.hint}</Badge>;
  return <Badge variant="good">Set ····{status.hint}</Badge>;
}

/** Never echoes the value: a password field, cleared on save, nothing kept in state (11 §4). */
function AddKeyDialog({
  provider,
  onClose,
  onSave,
}: {
  provider: ProviderRowDto | null;
  onClose: () => void;
  onSave: (key: string, baseUrl: string | undefined) => Promise<void>;
}) {
  const [value, setValue] = useState('');
  const [showUrl, setShowUrl] = useState(false);
  const [baseUrl, setBaseUrl] = useState<string | undefined>(undefined);
  const [saving, setSaving] = useState(false);
  const close = () => {
    setValue('');
    setShowUrl(false);
    setBaseUrl(undefined);
    onClose();
  };
  const save = async () => {
    setSaving(true);
    const key = value;
    setValue('');
    try {
      await onSave(key, baseUrl);
    } finally {
      setSaving(false);
      close();
    }
  };
  return (
    <Dialog open={provider !== null} onOpenChange={(open) => !open && close()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {provider?.key.present ? 'Replace' : 'Add'} the {provider?.label} key
          </DialogTitle>
          <DialogDescription>
            Stored encrypted on this machine. Gantry sends it only to {provider?.label}.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-ui font-medium text-fg">API key</span>
            <Input
              type="password"
              size="lg"
              autoFocus
              autoComplete="off"
              value={value}
              onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' && value.trim().length >= 8) void save();
              }}
              placeholder={KEY_HINT[provider?.id ?? ''] ?? 'sk-…'}
              className="font-mono"
            />
          </label>
          {provider?.base_url !== null && provider?.base_url !== undefined && (
            <button
              type="button"
              onClick={() => setShowUrl((s) => !s)}
              className="flex items-center gap-1 self-start text-meta text-fg-2 hover:text-fg"
            >
              <CaretDownIcon
                className={cn(
                  'size-3 transition-transform duration-(--dur-1)',
                  showUrl && 'rotate-180',
                )}
              />
              Base URL
            </button>
          )}
          {showUrl && (
            <Input
              value={baseUrl ?? provider?.base_url ?? ''}
              onChange={(e) => setBaseUrl(e.target.value)}
              aria-label="Base URL"
              className="font-mono"
            />
          )}
        </div>
        <DialogFooter>
          <DialogClose render={<Button variant="secondary" />}>Close</DialogClose>
          <Button
            variant="primary"
            disabled={value.trim().length < 8 || saving}
            onClick={() => void save()}
          >
            {saving ? 'Saving…' : 'Save key'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
