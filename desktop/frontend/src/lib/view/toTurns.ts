import type { ChatDetail, ContentPart, TurnDto } from '@/bindings';
import type { Block, Turn } from '@/fixtures/types';
import type { LiveTurn } from '@/lib/stores/runStore';

/**
 * Projects the backend's chat and the run store's live turn onto the view model the M0b
 * components render. Finished turns come from the chat; the running one from the channel.
 */
export function toTurns(
  chat: ChatDetail,
  live: LiveTurn | undefined,
  modelLabel: (ref: { provider: string; model: string }) => string,
): Turn[] {
  return chat.turns.map((t) => {
    if (live && live.turnId === t.id && (t.status === 'running' || live.status === 'running')) {
      return liveTurn(t, live, modelLabel);
    }
    return finishedTurn(t, modelLabel);
  });
}

function finishedTurn(
  t: TurnDto,
  modelLabel: (ref: { provider: string; model: string }) => string,
): Turn {
  const blocks = partsToBlocks(t.assistant?.parts ?? [], false, undefined);
  if (t.status === 'failed' && t.error) {
    blocks.push({ kind: 'error', message: t.error, retryable: false });
  }
  const durationMs = t.ended_at ? t.ended_at - t.started_at : 0;
  return {
    id: t.id,
    user: { text: textOf(t.user.parts) },
    blocks,
    footer: t.usage
      ? {
          model: modelLabel(t.model),
          durationMs,
          tokensIn: t.usage.input,
          tokensOut: t.usage.output,
        }
      : undefined,
    status: t.status === 'running' ? 'running' : t.status === 'completed' ? 'done' : t.status,
    endedAt: t.ended_at ?? undefined,
    feedback: t.feedback ?? undefined,
    text: t.assistant ? textOf(t.assistant.parts) : undefined,
  };
}

function liveTurn(
  t: TurnDto,
  live: LiveTurn,
  modelLabel: (ref: { provider: string; model: string }) => string,
): Turn {
  const parts = live.parts.filter((p): p is ContentPart => p !== undefined);
  const thinkingMs =
    live.thinkingStartedAt !== undefined
      ? (live.thinkingEndedAt ?? Date.now()) - live.thinkingStartedAt
      : undefined;
  const blocks = partsToBlocks(parts, live.status === 'running', thinkingMs);
  if (live.error)
    blocks.push({ kind: 'error', message: live.error.message, retryable: live.error.retryable });
  const done = live.status !== 'running';
  return {
    id: t.id,
    user: { text: textOf(t.user.parts) },
    blocks,
    footer:
      done && live.usage
        ? {
            model: modelLabel(live.model ?? t.model),
            durationMs: (live.endedAt ?? Date.now()) - live.startedAt,
            tokensIn: live.usage.input,
            tokensOut: live.usage.output,
          }
        : undefined,
    status:
      live.status === 'running' ? 'running' : live.status === 'completed' ? 'done' : live.status,
  };
}

/** Consecutive text parts merge; thinking gets its own collapsed block. */
function partsToBlocks(
  parts: ContentPart[],
  running: boolean,
  thinkingMs: number | undefined,
): Block[] {
  const blocks: Block[] = [];
  const lastIsThinking = () => parts.length > 0 && parts[parts.length - 1]?.kind === 'thinking';
  for (const part of parts) {
    if (part.kind === 'text') {
      const last = blocks[blocks.length - 1];
      if (last?.kind === 'text') last.markdown += part.text;
      else blocks.push({ kind: 'text', markdown: part.text });
    } else if (part.kind === 'thinking') {
      blocks.push({
        kind: 'thinking',
        text: part.text,
        running: running && lastIsThinking(),
        durationMs: thinkingMs,
      });
    }
  }
  return blocks;
}

function textOf(parts: ContentPart[]): string {
  return parts.map((p) => (p.kind === 'text' ? p.text : '')).join('');
}
