import { ArrowsClockwiseIcon, CaretDownIcon } from '@phosphor-icons/react';
import { useState } from 'react';

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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { toast } from '@/components/ui/toast';
import { type ProviderState, providers as fixture } from '@/fixtures/settings';
import { cn } from '@/lib/utils';

/** Settings → Providers & models (11 §4): a row per provider, keys write-only, models below. */
export function Providers() {
  const [providers, setProviders] = useState(fixture);
  const [adding, setAdding] = useState<ProviderState | null>(null);

  const setKey = (id: ProviderState['id'], hint: string) => {
    setProviders((ps) => ps.map((p) => (p.id === id ? { ...p, key: { present: true, hint } } : p)));
    toast.add({
      title: 'Key saved',
      description: `${providers.find((p) => p.id === id)?.name} · ····${hint}`,
      type: 'success',
    });
  };
  const clearKey = (id: ProviderState['id']) =>
    setProviders((ps) => ps.map((p) => (p.id === id ? { ...p, key: { present: false } } : p)));

  return (
    <div className="flex flex-col gap-8">
      <section>
        <p className="mb-4 text-body text-fg-2">
          Bring your own keys. They are encrypted with a key held by your operating system and never
          shown again after you save them.
        </p>
        <div className="divide-y divide-line-subtle border-y border-line-subtle">
          {providers.map((p) => (
            <ProviderRow
              key={p.id}
              provider={p}
              onAdd={() => setAdding(p)}
              onClear={() => clearKey(p.id)}
            />
          ))}
        </div>
      </section>

      <section>
        <div className="flex items-center justify-between">
          <h2 className="text-title font-medium text-fg">Models</h2>
          <Button variant="ghost" size="sm">
            <ArrowsClockwiseIcon />
            Refresh lists
          </Button>
        </div>
        <p className="mt-1 text-meta text-fg-2">
          From the providers with a key. Context window and prices per million tokens where known.
        </p>
        <table className="mt-3 w-full border-collapse text-ui">
          <thead>
            <tr className="text-left text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
              <th className="py-1.5 pr-3 font-medium">Model</th>
              <th className="py-1.5 pr-3 font-medium">Provider</th>
              <th className="py-1.5 pr-3 text-right font-medium">Context</th>
              <th className="py-1.5 text-right font-medium">In / out</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-line-subtle">
            {providers.flatMap((p) =>
              p.models.map((m) => (
                <tr key={m.id} className="h-(--row)">
                  <td className="pr-3 text-fg">{m.label}</td>
                  <td className="pr-3 text-fg-2">{p.name}</td>
                  <td className="pr-3 text-right text-fg-2 tnum">{m.context}</td>
                  <td className="text-right text-fg-2 tnum">{m.price ?? '—'}</td>
                </tr>
              )),
            )}
          </tbody>
        </table>
      </section>

      <AddKeyDialog
        provider={adding}
        onClose={() => setAdding(null)}
        onSave={(hint) => adding && setKey(adding.id, hint)}
      />
    </div>
  );
}

function ProviderRow({
  provider,
  onAdd,
  onClear,
}: {
  provider: ProviderState;
  onAdd: () => void;
  onClear: () => void;
}) {
  const { key } = provider;
  return (
    <div className="flex min-h-(--row) items-center gap-4 py-2.5">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-ui font-medium text-fg">{provider.name}</span>
          <KeyStatus status={key} />
        </div>
        {provider.baseUrl && (
          <div className="mt-0.5 font-mono text-mono text-fg-3">{provider.baseUrl}</div>
        )}
      </div>
      {provider.models.length > 0 && (
        <Select
          defaultValue={provider.defaultModel}
          items={provider.models.map((m) => ({ value: m.id, label: m.label }))}
        >
          <SelectTrigger size="sm" aria-label={`Default model for ${provider.name}`}>
            <SelectValue placeholder="Default model" />
          </SelectTrigger>
          <SelectContent>
            {provider.models.map((m) => (
              <SelectItem key={m.id} value={m.id}>
                {m.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
      <div className="flex shrink-0 items-center gap-1">
        {key.present ? (
          <>
            <Button variant="ghost" size="sm">
              Test
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
          <Button variant="secondary" size="sm" onClick={onAdd}>
            Add key
          </Button>
        )}
      </div>
    </div>
  );
}

export function KeyStatus({ status }: { status: ProviderState['key'] }) {
  if (!status.present) return <Badge>No key</Badge>;
  if (status.invalid) return <Badge variant="bad">Invalid ····{status.hint}</Badge>;
  return <Badge variant="good">Set ····{status.hint}</Badge>;
}

/** Never echoes the value: the field is a password field and the dialog keeps nothing (11 §4). */
function AddKeyDialog({
  provider,
  onClose,
  onSave,
}: {
  provider: ProviderState | null;
  onClose: () => void;
  onSave: (hint: string) => void;
}) {
  const [value, setValue] = useState('');
  const [showUrl, setShowUrl] = useState(false);
  const save = () => {
    onSave(value.slice(-4));
    setValue('');
    onClose();
  };
  return (
    <Dialog open={provider !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {provider?.key.present ? 'Replace' : 'Add'} the {provider?.name} key
          </DialogTitle>
          <DialogDescription>
            Stored encrypted on this machine. Gantry sends it only to {provider?.name}.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-ui font-medium text-fg">API key</span>
            <Input
              type="password"
              size="lg"
              autoFocus
              value={value}
              onChange={(e) => setValue(e.target.value)}
              placeholder={provider?.id === 'anthropic' ? 'sk-ant-…' : 'sk-…'}
              className="font-mono"
            />
          </label>
          {provider?.baseUrl !== undefined && (
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
            <Input defaultValue={provider?.baseUrl} aria-label="Base URL" className="font-mono" />
          )}
        </div>
        <DialogFooter>
          <DialogClose render={<Button variant="secondary" />}>Close</DialogClose>
          <Button variant="primary" disabled={value.trim().length < 8} onClick={save}>
            Save key
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
