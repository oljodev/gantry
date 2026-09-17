import { CheckCircleIcon, CircleNotchIcon, WarningCircleIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import { TurnView } from '@/components/gantry/chat/TurnView';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { SubAgentNode, TurnId } from '@/bindings';
import type { Turn } from '@/fixtures/types';
import { useSubAgents } from '@/lib/ipc/hooks/agents';
import { chatQuery } from '@/lib/ipc/hooks/chats';
import { modelLabel, useModelCatalog } from '@/lib/ipc/hooks/providers';
import { isTauri } from '@/lib/ipc/client';
import { toTurns } from '@/lib/view/toTurns';
import { cn } from '@/lib/utils';

/**
 * The agent tree (docs/plan/18 §7, A7): the turn at the root, the sub agents it started under
 * it, and any one of them opened to its own transcript.
 *
 * **There is no composer, here or anywhere else in it** (A8). A sub agent's whole contract is
 * one task in and one report out; an answer typed into the middle of one would have nowhere to
 * go in the parent's transcript. The one thing that can interrupt a sub agent is a permission
 * card, and that is answered in the chat behind this modal, where the user is.
 *
 * Live while it runs: the list refetches every second, and so does the transcript of whichever
 * node is open, because a sub agent's events reach the database and no channel.
 */
export function AgentTreeDialog({
  turnId,
  parentTurn,
  running,
  onClose,
}: {
  /** The parent turn whose sub agents these are; `null` closes the modal. */
  turnId: TurnId | null;
  /** That turn as the chat behind this modal already drew it, for the root node. */
  parentTurn?: Turn;
  /** Whether the parent turn is still going, which is when a node can still appear. */
  running: boolean;
  onClose: () => void;
}) {
  // `null` is "nothing chosen yet", which shows the first sub agent: the row was clicked to see
  // them, not to read again the reply it sits under. `'parent'` is the root node.
  const [openNode, setOpenNode] = useState<string | null>(null);
  const nodes = useSubAgents(turnId, running);
  const list = nodes.data ?? [];
  const node =
    openNode === 'parent' ? undefined : (list.find((n) => n.chat_id === openNode) ?? list[0]);

  return (
    <Dialog open={turnId !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="h-[80vh] w-[60rem] max-w-[calc(100vw-2rem)] overflow-hidden">
        <DialogHeader>
          <DialogTitle>Sub agents</DialogTitle>
          <DialogDescription>
            What this reply handed to other models. You cannot talk to a sub agent — it was given
            one task and hands back one report.
          </DialogDescription>
        </DialogHeader>
        <div className="flex min-h-0 flex-1 gap-4">
          <AgentTree
            nodes={list}
            parentLabel={parentTurn?.footer?.model ?? 'the chat you are in'}
            selected={node?.chat_id ?? null}
            empty={nodes.isPending ? 'Looking…' : 'Nothing started yet.'}
            onSelect={(id) => setOpenNode(id ?? 'parent')}
          />
          <div className="min-w-0 flex-1 overflow-y-auto">
            {node ? (
              <NodeTranscript node={node} />
            ) : parentTurn ? (
              <TurnView turn={parentTurn} />
            ) : (
              <p className="text-meta text-fg-3">This turn is no longer here.</p>
            )}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}

/**
 * The tree itself: the conversation at the root, the sub agents under it (18 §7).
 *
 * A component of its own because it is the one new drawing in this modal — everything to the
 * right of it is the turn view the chat already uses — and a drawing worth looking at before
 * a model has run is a drawing worth being able to render from fixtures.
 */
export function AgentTree({
  nodes,
  parentLabel,
  selected,
  empty,
  onSelect,
}: {
  nodes: SubAgentNode[];
  /** What the root node says under "This conversation": its model, usually. */
  parentLabel: string;
  /** The open node's chat id, or `null` for the conversation itself. */
  selected: string | null;
  empty: string;
  onSelect: (chatId: string | null) => void;
}) {
  return (
    <nav
      aria-label="The agent tree"
      className="flex w-60 shrink-0 flex-col gap-1 overflow-y-auto border-r border-line-subtle pr-3"
    >
      <TreeButton
        title="This conversation"
        detail={parentLabel}
        selected={selected === null}
        onClick={() => onSelect(null)}
      />
      <div className="ml-3 flex flex-col gap-1 border-l border-line-subtle pl-2">
        {nodes.map((n) => (
          <TreeButton
            key={n.chat_id}
            title={n.name}
            detail={n.task}
            status={n.status}
            selected={selected === n.chat_id}
            onClick={() => onSelect(n.chat_id)}
          />
        ))}
        {nodes.length === 0 && <p className="px-1 py-2 text-meta text-fg-3">{empty}</p>}
      </div>
    </nav>
  );
}

/** One node: its name, what it was asked, and how it is getting on. */
function TreeButton({
  title,
  detail,
  status,
  selected,
  onClick,
}: {
  title: string;
  detail: string;
  status?: SubAgentNode['status'];
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={selected}
      className={cn(
        'flex flex-col gap-0.5 rounded-2 px-2 py-1.5 text-left transition-colors duration-(--dur-1)',
        selected ? 'bg-selected text-fg' : 'text-fg-2 hover:bg-hover',
      )}
    >
      <span className="flex min-w-0 items-center gap-1.5 text-ui">
        {status && <StatusMark status={status} />}
        <span className="truncate">{title}</span>
      </span>
      <span className="truncate text-meta text-fg-3">{detail}</span>
    </button>
  );
}

function StatusMark({ status }: { status: SubAgentNode['status'] }) {
  if (status === 'running') {
    return <CircleNotchIcon className="size-3.5 shrink-0 animate-spin text-fg-3" />;
  }
  if (status === 'completed') return <CheckCircleIcon className="size-3.5 shrink-0 text-good" />;
  return <WarningCircleIcon className="size-3.5 shrink-0 text-bad" />;
}

/**
 * One sub agent's own conversation, read-only.
 *
 * The same projection the chat uses, on a chat row like any other (A1) — which is the whole
 * argument for a sub agent being a chat: a second kind of transcript would need a second view
 * to read it, and this one is free.
 */
function NodeTranscript({ node }: { node: SubAgentNode }) {
  const { providers } = useModelCatalog();
  const chat = useQuery({
    ...chatQuery(node.chat_id),
    enabled: isTauri(),
    refetchInterval: node.status === 'running' ? 1000 : false,
  });
  const turns = chat.data ? toTurns(chat.data, undefined, (ref) => modelLabel(providers, ref)) : [];
  // Only once it has finished: a clock that ticked here would be a second timer beside the
  // spinner, saying the same thing less clearly.
  const seconds = node.ended_at ? Math.round((node.ended_at - node.started_at) / 1000) : undefined;
  const tokens = node.usage ? node.usage.input + node.usage.output : undefined;
  return (
    <div className="flex min-w-0 flex-col">
      <header className="flex flex-col gap-1 border-b border-line-subtle pb-3">
        <div className="flex items-center gap-2">
          <h3 className="text-title font-medium text-fg">{node.name}</h3>
          <code className="text-meta text-fg-3">{node.agent}</code>
        </div>
        <p className="flex flex-wrap items-center gap-3 text-meta text-fg-3 tnum">
          <span>{modelLabel(providers, node.model)}</span>
          <span>{node.status === 'running' ? 'running' : node.status}</span>
          {seconds !== undefined && <span>{seconds} s</span>}
          {tokens !== undefined && <span>{tokens.toLocaleString()} tokens</span>}
        </p>
      </header>
      {turns.length === 0 && (
        <p className="pt-4 text-meta text-fg-3">
          {node.status === 'running' ? 'It has not said anything yet.' : 'It said nothing.'}
        </p>
      )}
      {turns.map((turn) => (
        <TurnView key={turn.id} turn={turn} />
      ))}
    </div>
  );
}
