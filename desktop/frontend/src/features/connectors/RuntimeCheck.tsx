import {
  ArrowClockwiseIcon,
  ArrowSquareOutIcon,
  CheckCircleIcon,
  CopyIcon,
  WarningCircleIcon,
} from '@phosphor-icons/react';
import { useState } from 'react';

import type { RuntimeStatus } from '@/bindings';
import { Button } from '@/components/ui/button';
import { copyText, openExternal } from '@/lib/clipboard';
import { cn } from '@/lib/utils';

/**
 * Step 1 of a local server's install (docs/plan/03 §11): what this connector runs on, whether
 * this machine has it, and what to do when it does not.
 *
 * There is no **Install anyway**, and its absence is the design. An instance whose runtime is
 * missing is a row in the Connectors list that fails every call with an error about `npx` — a
 * sentence that means nothing to the person reading it and cannot be acted on from where they
 * are reading it. The only useful thing to say is this, here, before anything is created.
 *
 * **Check again** exists because the fix happens in another window: the user leaves, runs the
 * command in their own terminal, and comes back. A dialog that had already decided would send
 * them round the install again to find out whether it worked.
 */
export function RuntimeCheck({
  statuses,
  checking,
  onCheckAgain,
}: {
  statuses: RuntimeStatus[];
  checking: boolean;
  onCheckAgain: () => void;
}) {
  return (
    <div className="flex flex-col gap-2">
      {statuses.map((status) => (
        <RuntimeRow key={status.name} status={status} />
      ))}
      <Button variant="secondary" className="self-start" disabled={checking} onClick={onCheckAgain}>
        <ArrowClockwiseIcon className={cn(checking && 'animate-spin')} />
        {checking ? 'Checking…' : 'Check again'}
      </Button>
    </div>
  );
}

function RuntimeRow({ status }: { status: RuntimeStatus }) {
  return (
    <div
      className={cn(
        'flex flex-col gap-2 rounded-3 border px-3 py-2.5',
        status.ok ? 'border-good/30 bg-good-subtle' : 'border-bad/30 bg-bad-subtle',
      )}
    >
      <div className="flex items-start gap-2 text-ui text-fg">
        {status.ok ? (
          <CheckCircleIcon className="mt-0.5 size-4 shrink-0 text-good" />
        ) : (
          <WarningCircleIcon className="mt-0.5 size-4 shrink-0 text-bad" />
        )}
        <div className="flex min-w-0 flex-col gap-0.5">
          <span>
            {status.name} {status.required}
            {status.found ? ` · found ${status.found}` : ''}
          </span>
          {/* Where it was found settles "but I have Node installed" without an argument: the
              PATH searched is the login shell's, and a version manager's shim is on it. */}
          {status.ok && status.path && (
            <span className="truncate font-mono text-micro text-fg-3" title={status.path}>
              {status.path}
            </span>
          )}
          {!status.ok && status.problem && (
            <span className="text-meta text-fg-2">{status.problem}</span>
          )}
        </div>
      </div>
      {!status.ok && status.install.length > 0 && (
        <div className="flex flex-wrap gap-1.5 pl-6">
          {status.install.map((hint) => (
            <Hint key={hint.label} label={hint.label} command={hint.command} url={hint.url} />
          ))}
        </div>
      )}
    </div>
  );
}

/** One way in: a command to copy and paste into a terminal, or a page to open. */
function Hint({
  label,
  command,
  url,
}: {
  label: string;
  command?: string | null;
  url?: string | null;
}) {
  const [copied, setCopied] = useState(false);
  if (command) {
    return (
      <button
        type="button"
        title={command}
        onClick={() => {
          void copyText(command).then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          });
        }}
        className="flex h-(--control-sm) max-w-full items-center gap-1.5 rounded-2 border border-line bg-raised px-2 text-meta text-fg-2 transition-colors duration-(--dur-1) hover:text-fg"
      >
        <CopyIcon className="size-3 shrink-0" />
        <span className="truncate font-mono">{copied ? 'Copied' : command}</span>
      </button>
    );
  }
  return (
    <button
      type="button"
      onClick={() => void openExternal(url ?? '', false)}
      className="flex h-(--control-sm) items-center gap-1.5 rounded-2 border border-line bg-raised px-2 text-meta text-fg-2 transition-colors duration-(--dur-1) hover:text-fg"
    >
      <ArrowSquareOutIcon className="size-3 shrink-0" />
      {label}
    </button>
  );
}
