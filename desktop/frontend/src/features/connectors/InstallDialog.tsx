import { ArrowSquareOutIcon, CheckCircleIcon } from '@phosphor-icons/react';
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
import type { AuthType, CatalogEntryDto, ConnectorInstanceDto } from '@/bindings';
import { useConnectorMutations } from '@/lib/ipc/hooks/connectors';
import { openExternal } from '@/lib/clipboard';
import { describe } from '@/lib/errors';
import { cn } from '@/lib/utils';

/**
 * The fallback when one click was not enough (docs/plan/03 §11). Installing does everything it
 * can on its own; this appears only when a server will not let Gantry in without something the
 * user has to fetch — which, in this catalog, is GitHub and only GitHub.
 *
 * The quickest way out goes first. For a server offering both, that is the token: pasting one
 * takes a minute, where the sign-in first needs an OAuth application to exist at all.
 */
export function InstallDialog({
  entry,
  instance,
  reason,
  onClose,
}: {
  entry: CatalogEntryDto;
  instance?: ConnectorInstanceDto;
  /** What the one-click attempt said when it could not finish by itself. */
  reason?: string;
  onClose: () => void;
}) {
  const { install, authorize, setToken } = useConnectorMutations();
  const [current, setCurrent] = useState<ConnectorInstanceDto | undefined>(instance);
  const [method, setMethod] = useState<AuthType>(entry.auth_alternate ?? entry.auth);
  const [token, setTokenValue] = useState('');
  const [clientId, setClientId] = useState('');
  const [error, setError] = useState<string | null>(null);

  const busy = install.isPending || authorize.isPending || setToken.isPending;
  const connected = current?.auth_state === 'authorized' && current.tools.length > 0;
  const choices: AuthType[] = entry.auth_alternate
    ? [entry.auth_alternate, entry.auth]
    : [entry.auth];
  const tokenSetupUrl = entry.auth_alternate_setup_url ?? entry.auth_setup_url;

  const run = async (step: (instance: ConnectorInstanceDto) => Promise<ConnectorInstanceDto>) => {
    setError(null);
    try {
      const target = current ?? (await install.mutateAsync(entry.id));
      setCurrent(target);
      setCurrent(await step(target));
    } catch (err) {
      setError(describe(err));
    }
  };

  const signIn = () =>
    run((i) => authorize.mutateAsync({ instanceId: i.id, clientId: clientId.trim() || undefined }));

  const submitToken = () =>
    run(async (i) => {
      const value = token.trim();
      setTokenValue('');
      return setToken.mutateAsync({ instanceId: i.id, token: value });
    });

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <div className="flex items-center gap-2.5">
            <span className="flex size-8 shrink-0 items-center justify-center rounded-2 bg-inset">
              <ConnectorMark id={entry.id} name={entry.name} size={18} />
            </span>
            <DialogTitle>Connect {entry.name}</DialogTitle>
          </div>
          <DialogDescription>
            {connected
              ? entry.description
              : `${entry.name} will not let an application in on its own. Do one of these once; Gantry remembers it.`}
          </DialogDescription>
        </DialogHeader>

        {connected ? (
          <div className="flex items-start gap-2 rounded-3 border border-good/30 bg-good-subtle px-3 py-2 text-ui text-fg">
            <CheckCircleIcon className="mt-0.5 size-4 shrink-0 text-good" />
            <span>
              Connected. {current?.tools.length} tool{current?.tools.length === 1 ? '' : 's'}{' '}
              available
              {current?.server ? `, ${current.server.name} ${current.server.version}` : ''}.
            </span>
          </div>
        ) : (
          <>
            {choices.length > 1 && (
              <div role="radiogroup" aria-label="How to connect" className="flex gap-1">
                {choices.map((id) => (
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
                    {methodLabel(id)}
                  </button>
                ))}
              </div>
            )}

            {method === 'oauth2' ? (
              <>
                <p className="text-meta text-fg-2">{entry.auth_instructions}</p>
                {entry.auth_setup_url && (
                  <Button
                    variant="secondary"
                    className="self-start"
                    onClick={() => void openExternal(entry.auth_setup_url ?? '', false)}
                  >
                    <ArrowSquareOutIcon />
                    Create the app on {hostOf(entry.auth_setup_url)}
                  </Button>
                )}
                <Input
                  value={clientId}
                  onChange={(e) => setClientId(e.target.value)}
                  placeholder="Client id"
                  aria-label="OAuth client id"
                />
                <p className="text-meta text-fg-3">
                  Callback:{' '}
                  <code className="selectable font-mono">http://127.0.0.1:17321/callback</code>
                </p>
              </>
            ) : (
              <>
                <p className="text-meta text-fg-2">
                  {entry.auth_alternate_instructions ?? entry.auth_instructions}
                </p>
                {tokenSetupUrl && (
                  <Button
                    variant="secondary"
                    className="self-start"
                    onClick={() => void openExternal(tokenSetupUrl, false)}
                  >
                    <ArrowSquareOutIcon />
                    Create a token on {hostOf(tokenSetupUrl)}
                  </Button>
                )}
                <Input
                  type="password"
                  value={token}
                  onChange={(e) => setTokenValue(e.target.value)}
                  placeholder="Paste the token"
                  aria-label="Token"
                />
              </>
            )}

            {/* Why the one-click path stopped, in the server's own words. */}
            {reason && !error && <p className="text-meta text-fg-3">{reason}</p>}
          </>
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
              onClick={() => void openExternal(entry.homepage ?? '', false)}
            >
              <ArrowSquareOutIcon />
              About
            </Button>
          )}
          <Button variant="secondary" onClick={onClose}>
            {connected ? 'Done' : 'Cancel'}
          </Button>
          {!connected && (
            <Button
              disabled={
                busy || (method === 'oauth2' ? clientId.trim() === '' : token.trim() === '')
              }
              onClick={() => (method === 'oauth2' ? void signIn() : void submitToken())}
            >
              {busy ? 'Connecting…' : method === 'oauth2' ? 'Sign in' : 'Connect'}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** "github.com", for a button that says where it is about to take you. */
function hostOf(url: string | null | undefined): string {
  try {
    return new URL(url ?? '').hostname.replace(/^www\./, '');
  } catch {
    return 'the web';
  }
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
