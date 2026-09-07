import type {
  ChatDetail,
  ContentPart,
  GrantScope,
  Interaction,
  Message,
  ToolCallDto,
  TurnDto,
} from '@/bindings';
import type { ActivityItem, Block, Permission, Turn } from '@/fixtures/types';
import { isArtifactTool } from '@/features/artifacts/registry';
import type { LiveMessage, LiveTurn } from '@/lib/stores/runStore';

type Label = (ref: { provider: string; model: string }) => string;

/**
 * Projects the backend's chat and the run store's live turn onto the view model the M0b
 * components render. Finished turns come from the chat; the running one from the channel.
 * Activity is inline in the order it happened (05 §1): consecutive tool calls fold into one
 * activity block, and a pending decision becomes a permission card after them.
 */
/** The chat's artifacts by id (title and type), so rows and cards can name what changed. */
export type ArtifactIndex = Record<string, { title: string; type: string; version: number }>;

export function toTurns(
  chat: ChatDetail,
  live: LiveTurn | undefined,
  modelLabel: Label,
  titles: ArtifactIndex = {},
): Turn[] {
  return chat.turns.map((t) => {
    if (live && live.turnId === t.id && (t.status === 'running' || live.status === 'running')) {
      return liveTurn(t, live, modelLabel, titles);
    }
    return finishedTurn(t, modelLabel, titles);
  });
}

function finishedTurn(t: TurnDto, modelLabel: Label, titles: ArtifactIndex): Turn {
  const calls = Object.fromEntries(t.tool_calls.map((c) => [c.id, c]));
  const blocks = messagesToBlocks(
    t.messages.map((m) => ({ id: m.id, role: m.role, parts: [...m.parts] })),
    calls,
    [],
    false,
    undefined,
    titles,
  );
  pushNotices(blocks, t.notices);
  if (t.status === 'failed' && t.error) {
    blocks.push({ kind: 'error', message: t.error, retryable: false });
  }
  const durationMs = t.ended_at ? t.ended_at - t.started_at : 0;
  return {
    id: t.id,
    user: userOf(t),
    blocks,
    footer: t.usage
      ? {
          model: modelLabel(t.model),
          durationMs,
          tokensIn: t.usage.input,
          tokensOut: t.usage.output,
        }
      : undefined,
    status: statusOf(t.status),
    endedAt: t.ended_at ?? undefined,
    feedback: t.feedback ?? undefined,
    text: textOf(t.messages),
  };
}

function liveTurn(t: TurnDto, live: LiveTurn, modelLabel: Label, titles: ArtifactIndex): Turn {
  const thinkingMs =
    live.thinkingStartedAt !== undefined
      ? (live.thinkingEndedAt ?? Date.now()) - live.thinkingStartedAt
      : undefined;
  const blocks = messagesToBlocks(
    live.messages,
    live.calls,
    live.pending,
    live.status === 'running',
    thinkingMs,
    titles,
  );
  pushNotices(blocks, live.notices);
  if (live.error)
    blocks.push({ kind: 'error', message: live.error.message, retryable: live.error.retryable });
  const done = live.status !== 'running';
  return {
    id: t.id,
    user: userOf(t),
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
      live.status === 'running' && live.pending.length > 0 ? 'waiting' : statusOf(live.status),
  };
}

/** Notices sit after the text and calls they explain, as plain rows (05 §1). */
function pushNotices(blocks: Block[], notices: string[]) {
  if (notices.length === 0) return;
  const items: ActivityItem[] = notices.map((text, i) => ({
    kind: 'notice',
    id: `notice-${i}`,
    text,
  }));
  const last = blocks[blocks.length - 1];
  if (last?.kind === 'activity') last.items.push(...items);
  else blocks.push({ kind: 'activity', items });
}

function statusOf(s: TurnDto['status']): Turn['status'] {
  return s === 'completed' ? 'done' : s;
}

/** The user's text plus a chip per attached file or image (15 §8, `UserMessage`). */
function userOf(t: TurnDto): Turn['user'] {
  const attachments: NonNullable<Turn['user']['attachments']> = [];
  for (const p of t.user.parts) {
    if (p.kind === 'document') attachments.push({ name: p.name, kind: 'file' });
    else if (p.kind === 'image') {
      attachments.push({
        name: `${p.mime.replace('image/', '')} image`,
        kind: 'image',
        blob: p.source.kind === 'blob' ? p.source.hash : undefined,
        mime: p.mime,
      });
    }
  }
  return {
    text: partsText(t.user.parts),
    attachments: attachments.length > 0 ? attachments : undefined,
  };
}

/**
 * Walks the turn's messages in order. Text parts merge into markdown blocks, thinking gets its
 * collapsed block, tool-call parts become activity rows (results are read from `calls`), and
 * the pending permission cards follow the rows of the last round.
 */
function messagesToBlocks(
  messages: LiveMessage[],
  calls: Record<string, ToolCallDto>,
  pending: Interaction[],
  running: boolean,
  thinkingMs: number | undefined,
  titles: ArtifactIndex,
): Block[] {
  const blocks: Block[] = [];
  const pushItem = (item: ActivityItem) => {
    const last = blocks[blocks.length - 1];
    if (last?.kind === 'activity') last.items.push(item);
    else blocks.push({ kind: 'activity', items: [item] });
  };
  const lastMessage = messages[messages.length - 1];
  const lastIsThinking = () => {
    const parts = lastMessage?.parts ?? [];
    return parts.length > 0 && parts[parts.length - 1]?.kind === 'thinking';
  };
  for (const m of messages) {
    if (m.role !== 'assistant') continue;
    for (const part of m.parts) {
      if (!part) continue;
      if (part.kind === 'text') {
        if (part.text.trim() === '') continue;
        const last = blocks[blocks.length - 1];
        if (last?.kind === 'text') last.markdown += part.text;
        else blocks.push({ kind: 'text', markdown: part.text });
      } else if (part.kind === 'thinking') {
        blocks.push({
          kind: 'thinking',
          text: part.text,
          running: running && m === lastMessage && lastIsThinking(),
          durationMs: thinkingMs,
        });
      } else if (part.kind === 'tool_call') {
        pushItem(callItem(part, calls[part.id], titles));
      }
    }
  }
  // Calls the stream announced but whose block has not been finalised yet.
  const shown = new Set(
    blocks.flatMap((b) => (b.kind === 'activity' ? b.items.map((i) => i.id) : [])),
  );
  for (const c of Object.values(calls)) {
    if (!shown.has(c.id) && c.message_id === lastMessage?.id)
      pushItem(callItem(undefined, c, titles));
  }
  blocks.push(...artifactCards(blocks, titles));
  for (const p of pending) {
    if (p.payload.kind === 'permission')
      blocks.push({ kind: 'permission', permission: permissionOf(p) });
  }
  return blocks;
}

/**
 * One card per artifact the turn created or changed, once the call has finished and the
 * artifact exists: the newest version, named by the artifact's current title (13 §10).
 */
function artifactCards(blocks: Block[], titles: ArtifactIndex): Block[] {
  const cards = new Map<string, Extract<Block, { kind: 'artifact' }>>();
  for (const b of blocks) {
    if (b.kind !== 'activity') continue;
    for (const item of b.items) {
      if (item.kind !== 'artifact' || !item.artifactId || item.status !== 'done') continue;
      const prev = cards.get(item.artifactId);
      cards.set(item.artifactId, {
        kind: 'artifact',
        artifactId: item.artifactId,
        title: titles[item.artifactId]?.title ?? item.title,
        type: titles[item.artifactId]?.type ?? item.type,
        version: titles[item.artifactId]?.version ?? Math.max(item.version, prev?.version ?? 0),
        action: prev?.action ?? item.action ?? 'created',
      });
    }
  }
  return [...cards.values()];
}

function callItem(
  part: Extract<ContentPart, { kind: 'tool_call' }> | undefined,
  call: ToolCallDto | undefined,
  titles: ArtifactIndex,
): ActivityItem {
  const id = call?.id ?? part?.id ?? '';
  const [connector, tool] = call ? [call.connector, call.tool] : splitName(part?.name ?? '');
  const modelName = call?.model_tool_name ?? part?.name ?? '';
  if (isArtifactTool(modelName)) return artifactItem(id, tool, call, part, titles);
  return {
    kind: 'connector',
    id,
    connector,
    connectorName: call?.connector_name,
    tool,
    summary: call?.display.summary ?? '',
    status: rowStatus(call),
    tier: call?.tier,
    args: call?.args ?? part?.args,
    result: call?.result ?? undefined,
    isError: call?.is_error,
    durationMs: call?.duration_ms ?? undefined,
  };
}

/** "Created artifact · Title (React)" / "Updated artifact · Title (v3)" rows (13 §10). */
function artifactItem(
  id: string,
  tool: string,
  call: ToolCallDto | undefined,
  part: Extract<ContentPart, { kind: 'tool_call' }> | undefined,
  titles: ArtifactIndex,
): ActivityItem {
  const args = (call?.args ?? part?.args ?? {}) as Record<string, unknown>;
  const str = (k: string) => (typeof args[k] === 'string' ? (args[k] as string) : undefined);
  const result = resultJson(call);
  const artifactId =
    typeof result?.artifact_id === 'string' ? result.artifact_id : str('artifact_id');
  const version = typeof result?.version === 'number' ? result.version : 0;
  const status = rowStatus(call);
  return {
    kind: 'artifact',
    id,
    artifactId,
    title: str('title') ?? (artifactId ? (titles[artifactId]?.title ?? 'artifact') : 'artifact'),
    type: str('type') ?? (artifactId ? (titles[artifactId]?.type ?? 'artifact') : 'artifact'),
    version,
    action: tool === 'create_artifact' ? 'created' : 'updated',
    status:
      status === 'done'
        ? 'done'
        : status === 'failed' || status === 'denied'
          ? 'failed'
          : status === 'cancelled'
            ? 'cancelled'
            : 'running',
  };
}

/** The structured JSON of a finished call's result, if it has one. */
function resultJson(call: ToolCallDto | undefined): Record<string, unknown> | undefined {
  const first = call?.result?.[0];
  if (first?.kind === 'json' && first.json && typeof first.json === 'object') {
    return first.json as Record<string, unknown>;
  }
  return undefined;
}

function rowStatus(
  call: ToolCallDto | undefined,
): Extract<ActivityItem, { kind: 'connector' }>['status'] {
  switch (call?.status) {
    case undefined:
    case 'proposed':
      return 'proposed';
    case 'awaiting_decision':
      return 'waiting';
    case 'running':
      return 'running';
    case 'completed':
      return call.is_error ? 'failed' : 'done';
    case 'failed':
      return 'failed';
    case 'denied':
      return 'denied';
    case 'cancelled':
      return 'cancelled';
  }
}

function splitName(name: string): [string, string] {
  const i = name.indexOf('__');
  return i > 0 ? [name.slice(0, i), name.slice(i + 2)] : ['', name];
}

/** A pending permission interaction as the card renders it (04 §7). Grants arrive with M7. */
export function permissionOf(i: Interaction): Permission {
  if (i.payload.kind !== 'permission') throw new Error('not a permission');
  const r = i.payload.request;
  const args: Record<string, string> = {};
  if (r.args && typeof r.args === 'object' && !Array.isArray(r.args)) {
    for (const [k, v] of Object.entries(r.args as Record<string, unknown>)) {
      args[k] = typeof v === 'string' ? v : JSON.stringify(v);
    }
  }
  return {
    id: i.id,
    connector: r.connector,
    connectorName: r.connector_name,
    tool: r.tool,
    tier: r.tier,
    title: `${r.connector_name} wants to run ${r.tool}`,
    args,
    note: r.description,
    why: r.why ?? undefined,
    scopes: [{ id: 'once', label: 'Allow once' }, ...r.scopes.map((s) => scopeOption(s, r.tool))],
  };
}

/** A grant scope as the card's dropdown shows it (04 §8). */
function scopeOption(scope: GrantScope, tool: string): { id: string; label: string } {
  return scope === 'all_reads'
    ? { id: 'all_reads', label: 'Allow all reads for this chat' }
    : { id: 'tool', label: `Allow ${tool} for this chat` };
}

function textOf(messages: Message[]): string | undefined {
  const text = messages
    .filter((m) => m.role === 'assistant')
    .map((m) => partsText(m.parts).trim())
    .filter((t) => t !== '')
    .join('\n\n');
  return text === '' ? undefined : text;
}

function partsText(parts: ContentPart[]): string {
  return parts.map((p) => (p.kind === 'text' ? p.text : '')).join('');
}
