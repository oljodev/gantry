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
import { ConfigForm } from '@/features/connectors/ConfigForm';
import { effectiveValues, missingRequired } from '@/features/connectors/config';
import { RuntimeCheck } from '@/features/connectors/RuntimeCheck';
import {
  useConnectorConfig,
  useConnectorMutations,
  useRuntimeCheck,
} from '@/lib/ipc/hooks/connectors';
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
  const { install, authorize, setToken, setConfig } = useConnectorMutations();
  const [current, setCurrent] = useState<ConnectorInstanceDto | undefined>(instance);
  const [method, setMethod] = useState<AuthType>(entry.auth_alternate ?? entry.auth);
  const [token, setTokenValue] = useState('');
  const [clientId, setClientId] = useState('');
  const [error, setError] = useState<string | null>(null);
  // 03 §11 step 1: a local server runs on something, and nothing else in this dialog matters
  // until it is here. Asked for only when the manifest asks for a runtime, which is never for a
  // remote server.
  const runtimes = useRuntimeCheck(entry.requires.length > 0 ? entry.id : null);
  const statuses = runtimes.data ?? [];
  const blocked = statuses.some((r) => !r.ok);
  // Step 2: the keys this connector asks for. Comes after the runtime check, because a form
  // filled in for a server that cannot start is work thrown away.
  const form = useConnectorConfig(entry.id, current?.id);
  const fields = form.data?.fields ?? [];
  const [answers, setAnswers] = useState<Record<string, string>>({});
  const saved = form.data?.values ?? {};
  const values = effectiveValues(fields, saved, answers);
  const configured = current !== undefined && Object.keys(saved).length > 0;
  const needsConfig =
    !blocked && fields.length > 0 && (!configured || Object.keys(answers).length > 0);
  const missing = missingRequired(fields, values, configured);

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

  /** Save the answers, then let the ordinary path carry on from the connector it just made. */
  const saveConfig = () =>
    run(async (i) => {
      const next = await setConfig.mutateAsync({ instanceId: i.id, values });
      setAnswers({});
      return next;
    });

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
            {blocked
              ? `${entry.name} runs as a program on this machine, and needs something that is not here yet.`
              : needsConfig
                ? `${entry.name} needs a few details before it can run.`
                : connected
                  ? entry.description
                  : entry.auth_needs_client_id
                    ? `${entry.name} will not let an application in on its own. Do one of these once; Gantry remembers it.`
                    : `Sign in to ${entry.name} in your browser. Gantry keeps the token on this machine.`}
          </DialogDescription>
        </DialogHeader>

        {blocked ? (
          <RuntimeCheck
            statuses={statuses}
            checking={runtimes.isFetching}
            onCheckAgain={() => void runtimes.refetch()}
          />
        ) : needsConfig ? (
          <ConfigForm
            fields={fields}
            values={values}
            hasSaved={configured}
            onChange={(key, value) => setAnswers((a) => ({ ...a, [key]: value }))}
          />
        ) : connected ? (
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
                {entry.auth_needs_client_id && (
                  <>
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
                )}
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
          {needsConfig ? (
            <Button
              disabled={busy || missing.length > 0}
              title={missing.length > 0 ? `Still needed: ${missing.join(', ')}` : undefined}
              onClick={() => void saveConfig()}
            >
              {setConfig.isPending ? 'Saving…' : 'Save and connect'}
            </Button>
          ) : (
            !connected &&
            !blocked && (
              <Button
                disabled={
                  busy ||
                  (method === 'oauth2'
                    ? entry.auth_needs_client_id && clientId.trim() === ''
                    : token.trim() === '')
                }
                onClick={() => (method === 'oauth2' ? void signIn() : void submitToken())}
              >
                {busy ? 'Connecting…' : method === 'oauth2' ? 'Sign in' : 'Connect'}
              </Button>
            )
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
