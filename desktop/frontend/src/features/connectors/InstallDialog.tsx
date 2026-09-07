import { ArrowSquareOutIcon, CheckCircleIcon, WarningCircleIcon } from '@phosphor-icons/react';
import { useState } from 'react';

import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { toast } from '@/components/ui/toast';
import type { AuthType, CatalogEntryDto, ConnectorInstanceDto } from '@/bindings';
import { useConnectorMutations } from '@/lib/ipc/hooks/connectors';
import { openExternal } from '@/lib/clipboard';
import { cn } from '@/lib/utils';

/**
 * Installing one catalog entry (docs/plan/03 §11): what it will connect to, then the credential
 * it needs, then the tools it found. A server that needs nothing shows the last step at once.
 *
 * The two credentials a server may accept are offered side by side, because which one suits
 * depends on what the user has: GitHub takes a sign-in if you have an OAuth application to name,
 * and a token if you do not.
 */
export function InstallDialog({
  entry,
  instance,
  onClose,
}: {
  entry: CatalogEntryDto;
  /** Set when the entry is already installed and only the credential is missing. */
  instance?: ConnectorInstanceDto;
  onClose: () => void;
}) {
  const { install, authorize, setToken, connect } = useConnectorMutations();
  const [current, setCurrent] = useState<ConnectorInstanceDto | undefined>(instance);
  const [method, setMethod] = useState<AuthType>(entry.auth);
  const [token, setTokenValue] = useState('');
  const [clientId, setClientId] = useState('');
  const [error, setError] = useState<string | null>(null);

  const busy = install.isPending || authorize.isPending || setToken.isPending || connect.isPending;
  const needsCredential = entry.auth !== 'none';
  const connected = current?.auth_state === 'authorized' && current.tools.length > 0;

  /** Installs if needed and hands back the instance to authorize. */
  const ensure = async (): Promise<ConnectorInstanceDto> => {
    if (current) return current;
    const created = await install.mutateAsync(entry.id);
    setCurrent(created);
    return created;
  };

  const run = async (step: (instance: ConnectorInstanceDto) => Promise<ConnectorInstanceDto>) => {
    setError(null);
    try {
      const instance = await ensure();
      setCurrent(await step(instance));
    } catch (err) {
      setError(String(err));
    }
  };

  const signIn = () =>
    run((i) =>
      authorize.mutateAsync({
        instanceId: i.id,
        clientId: clientId.trim() || undefined,
      }),
    );

  const submitToken = () =>
    run(async (i) => {
      const value = token.trim();
      setTokenValue('');
      return setToken.mutateAsync({ instanceId: i.id, token: value });
    });

  const installOnly = () =>
    run(async (i) => {
      await connect.mutateAsync(i.id);
      return i;
    });

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <div className="flex items-center gap-2.5">
            <span className="flex size-8 shrink-0 items-center justify-center rounded-2 bg-inset">
              <ConnectorMark id={entry.id} name={entry.name} size={18} />
            </span>
            <DialogTitle>{entry.name}</DialogTitle>
          </div>
          <DialogDescription>{entry.description}</DialogDescription>
        </DialogHeader>

        {/* What it will talk to, before anything runs (03 §11, the command preview). */}
        <div className="rounded-3 border border-line-subtle bg-inset px-3 py-2">
          <div className="text-meta text-fg-3">
            {entry.kind === 'mcp-remote' ? 'Connects to' : 'Runs'}
          </div>
          <code className="selectable break-all font-mono text-mono text-fg-2">
            {entry.preview}
          </code>
        </div>

        {entry.requires.length > 0 && (
          <div className="flex items-start gap-2 rounded-3 border border-line-subtle px-3 py-2 text-meta text-fg-2">
            <WarningCircleIcon className="mt-0.5 size-4 shrink-0 text-warn" />
            <span>
              Needs {entry.requires.map((r) => `${r.name} ${r.version}`).join(', ')} on this
              machine.
            </span>
          </div>
        )}

        {connected ? (
          <div className="flex items-start gap-2 rounded-3 border border-good/30 bg-good-subtle px-3 py-2 text-ui text-fg">
            <CheckCircleIcon className="mt-0.5 size-4 shrink-0 text-good" />
            <span>
              Connected. {current?.tools.length} tool
              {current?.tools.length === 1 ? '' : 's'} available
              {current?.server ? `, ${current.server.name} ${current.server.version}` : ''}.
            </span>
          </div>
        ) : needsCredential ? (
          <>
            {entry.auth_alternate && (
              <div role="radiogroup" aria-label="How to connect" className="flex gap-1">
                {[
                  [entry.auth, methodLabel(entry.auth)] as const,
                  [entry.auth_alternate, methodLabel(entry.auth_alternate)] as const,
                ].map(([id, label]) => (
                  <button
                    key={id}
                    type="button"
                    role="radio"
                    aria-checked={method === id}
                    onClick={() => setMethod(id)}
                    className={cn(
                      'h-(--control-sm) rounded-full px-3 text-meta transition-colors duration-(--dur-1)',
                      method === id
                        ? 'bg-selected text-fg'
                        : 'text-fg-2 hover:bg-hover hover:text-fg',
                    )}
                  >
                    {label}
                  </button>
                ))}
              </div>
            )}
            <p className="text-meta text-fg-2">
              {method === entry.auth ? entry.auth_instructions : entry.auth_alternate_instructions}
            </p>
            {method === 'oauth2' ? (
              <>
                <Input
                  value={clientId}
                  onChange={(e) => setClientId(e.target.value)}
                  placeholder="OAuth client id (only if the server asks for one)"
                  aria-label="OAuth client id"
                />
                <p className="text-meta text-fg-3">
                  The callback to register is{' '}
                  <code className="selectable font-mono">http://127.0.0.1:17321/callback</code>.
                </p>
              </>
            ) : (
              <Input
                type="password"
                value={token}
                onChange={(e) => setTokenValue(e.target.value)}
                placeholder="Paste the token"
                aria-label="Token"
              />
            )}
          </>
        ) : (
          <p className="text-meta text-fg-2">
            This server needs no account. Installing connects and reads its tool list.
          </p>
        )}

        {error && (
          <p role="alert" className="text-meta text-bad">
            {error}
          </p>
        )}

        <DialogFooter>
          {entry.homepage && (
            <Button
              variant="ghost"
              className="mr-auto"
              onClick={() => void openExternal(entry.homepage ?? '')}
            >
              <ArrowSquareOutIcon />
              About
            </Button>
          )}
          <Button variant="secondary" onClick={onClose}>
            {connected ? 'Done' : 'Cancel'}
          </Button>
          {!connected &&
            (needsCredential ? (
              <Button
                disabled={busy || (method !== 'oauth2' && token.trim() === '')}
                onClick={() => {
                  if (method === 'oauth2') {
                    toast.add({
                      title: 'Finish in your browser',
                      description: 'Gantry is waiting for the sign-in to come back.',
                    });
                    void signIn();
                  } else {
                    void submitToken();
                  }
                }}
              >
                {busy ? 'Connecting…' : method === 'oauth2' ? 'Sign in' : 'Connect'}
              </Button>
            ) : (
              <Button disabled={busy} onClick={() => void installOnly()}>
                {busy ? 'Installing…' : 'Install'}
              </Button>
            ))}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function methodLabel(auth: AuthType): string {
  switch (auth) {
    case 'oauth2':
      return 'Sign in';
    case 'api_key':
      return 'Use a key';
    case 'headers':
      return 'Use a token';
    case 'none':
      return 'No account';
  }
}
