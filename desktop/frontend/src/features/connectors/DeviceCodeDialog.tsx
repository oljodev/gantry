import { ArrowSquareOutIcon, CheckIcon, CopyIcon } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { events } from '@/bindings';
import { copyText, openExternal } from '@/lib/clipboard';
import { isTauri } from '@/lib/ipc/client';
import { useConnectors } from '@/lib/ipc/hooks/connectors';

interface Prompt {
  instanceId: string;
  connector: string;
  userCode: string;
  verificationUri: string;
}

/**
 * The device sign-in (RFC 8628, docs/plan/03 §7): a server that will not take a client without a
 * secret asks the user to type a code on its own page instead. The browser is already open on
 * that page; this holds the code where it can be read and copied while it waits.
 *
 * It closes itself when the connector comes back authorized, so the ending needs no button.
 */
export function DeviceCodeDialog() {
  const [prompt, setPrompt] = useState<Prompt | null>(null);
  const [copied, setCopied] = useState(false);
  const connectors = useConnectors();

  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = events.deviceCodeNeeded.listen((e) => {
      setCopied(false);
      setPrompt({
        instanceId: e.payload.instance_id,
        connector: e.payload.connector,
        userCode: e.payload.user_code,
        verificationUri: e.payload.verification_uri,
      });
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  // The sign-in finishing is what closes this, and the connector list is where that shows.
  // Adjusting state during render is the sanctioned way to follow a value from outside.
  const instance = (connectors.data ?? []).find((i) => i.id === prompt?.instanceId);
  if (prompt && instance?.auth_state === 'authorized') {
    setPrompt(null);
    return null;
  }
  if (!prompt) return null;

  return (
    <Dialog open onOpenChange={(next) => !next && setPrompt(null)}>
      <DialogContent className="max-w-sm">
        <DialogHeader>
          <DialogTitle>Sign in to {prompt.connector}</DialogTitle>
          <DialogDescription>
            Your browser is open on {hostOf(prompt.verificationUri)}. Type this code there and
            approve it.
          </DialogDescription>
        </DialogHeader>

        <button
          type="button"
          onClick={() => {
            void copyText(prompt.userCode);
            setCopied(true);
          }}
          className="flex items-center justify-center gap-3 rounded-3 border border-line bg-inset py-4 transition-colors duration-(--dur-1) hover:bg-hover"
        >
          <span className="selectable font-mono text-page tracking-[0.12em] text-fg">
            {prompt.userCode}
          </span>
          {copied ? (
            <CheckIcon className="size-4 text-good" aria-label="Copied" />
          ) : (
            <CopyIcon className="size-4 text-fg-3" aria-label="Copy the code" />
          )}
        </button>

        <p className="text-meta text-fg-3">
          Waiting for {prompt.connector}. This window closes itself when you are through.
        </p>

        <DialogFooter>
          <Button variant="secondary" onClick={() => setPrompt(null)}>
            Hide
          </Button>
          <Button onClick={() => void openExternal(prompt.verificationUri, false)}>
            <ArrowSquareOutIcon />
            Open the page again
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '');
  } catch {
    return 'the page';
  }
}
