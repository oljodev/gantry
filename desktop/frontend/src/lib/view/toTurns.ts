import type {
  ChatDetail,
  ContentPart,
  GrantScope,
  Interaction,
  Message,
  ToolCallDto,
  TurnDto,
} from '@/bindings';
import type {
  AccessAsk,
  ActivityItem,
  Block,
  ConnectorOffer,
  ElicitationAsk,
  GuardMark,
  Permission,
  SubAgentRun,
  Turn,
} from '@/fixtures/types';
import { isArtifactTool } from '@/features/artifacts/registry';
import { fileItem } from '@/lib/view/fileTools';
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
    return cachedTurn(t, modelLabel, titles);
  });
}

/**
 * Finished turns, built once (docs/dev/performance.md).
 *
 * This runs on every frame of a streaming answer, and every frame it rebuilt the whole
 * transcript — sixty turns of blocks, activity rows and footers, of which exactly one had
 * changed. A finished turn is a pure function of what it is given, and the backend hands back
 * the same `TurnDto` object until the chat query refetches, so the identity of that is the
 * cache key. Weak, so a chat the user has left is not held in memory by its own view model.
 *
 * The two other inputs are compared rather than trusted. `titles` is one object per chat query
 * and holds still; the label is a closure, and a closure rebuilt on each render would quietly
 * turn this cache off for ever — so what is compared is the name it produces, which is one
 * lookup and cannot be got wrong by a caller.
 *
 * The turn object being reused is also what lets `TurnView` skip the re-render entirely.
 */
const built = new WeakMap<TurnDto, { label: string; titles: ArtifactIndex; turn: Turn }>();

function cachedTurn(t: TurnDto, modelLabel: Label, titles: ArtifactIndex): Turn {
  const label = modelLabel(t.model);
  const hit = built.get(t);
  if (hit && hit.label === label && hit.titles === titles) return hit.turn;
  const turn = finishedTurn(t, modelLabel, titles);
  built.set(t, { label, titles, turn });
  return turn;
}

/**
 * The "Context used" row (05 §2, 12 §A4, §B4): the skills and memories this turn was given
 * beyond the transcript.
 *
 * It is read from the user message's own `turn_context` part rather than from the
 * `context.injected` event, because the part is written down — so the row is the same before a
 * reload and after one, and it says exactly what the model was sent rather than what an event
 * said it would be sent.
 */
function contextItem(t: TurnDto): ActivityItem | undefined {
  for (const part of t.user.parts) {
    if (part.kind !== 'turn_context') continue;
    const { skills, memories } = part.injected;
    if (skills.length === 0 && memories.length === 0) return undefined;
    return {
      kind: 'context',
      id: `${t.id}-context`,
      skills: skills.map((s) => s.name),
      memories: memories.length,
    };
  }
  return undefined;
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
  pushContext(blocks, contextItem(t));
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
          cached: t.usage.cache_read || undefined,
          subTokens: subTokens(blocks),
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
    live.output,
  );
  pushContext(blocks, contextItem(t));
  pushNotices(blocks, live.notices);
  if (live.error)
    blocks.push({
      kind: 'error',
      message: live.error.message,
      retryable: live.error.retryable,
      code: live.error.code,
    });
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
            cached: live.usage.cache_read || undefined,
            subTokens: subTokens(blocks),
          }
        : undefined,
    status:
      live.status === 'running' && live.pending.length > 0 ? 'waiting' : statusOf(live.status),
  };
}

/** The context row goes first: it is what the model was given before it said anything. */
function pushContext(blocks: Block[], item: ActivityItem | undefined) {
  if (!item) return;
  const first = blocks[0];
  if (first?.kind === 'activity') first.items.unshift(item);
  else blocks.unshift({ kind: 'activity', items: [item] });
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
  // A turn Gantry opened rather than the user: **Allow anyway** starts one from a system note
  // (04 §6), and a note is not something the user said.
  if (t.user.role === 'system') {
    const text = t.user.parts
      .map((p) => (p.kind === 'system_note' ? p.text : ''))
      .filter(Boolean)
      .join(' ');
    return { text, system: true };
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
  /** What running calls have printed so far; empty for a finished turn, whose result has it. */
  output: Record<string, string[]> = {},
): Block[] {
  const blocks: Block[] = [];
  const pushItem = (item: ActivityItem) => {
    const last = blocks[blocks.length - 1];
    if (last?.kind === 'activity') {
      // Sub agents fold into the row already there (18 §7): a reply that started three of them
      // says "Waiting for 3 sub agents" once, not three rows each saying it about one.
      const prev = last.items[last.items.length - 1];
      if (item.kind === 'subagents' && prev?.kind === 'subagents') {
        prev.runs.push(...item.runs);
        return;
      }
      last.items.push(item);
    } else blocks.push({ kind: 'activity', items: [item] });
  };
  // Anthropic reports a provider search as two blocks — the call, then its results — so the row
  // is kept by id and filled in when the second one arrives. The row object is the one already
  // in the block, so mutating it here is what puts the results on screen.
  const webRows = new Map<string, Extract<ActivityItem, { kind: 'web' }>>();
  const lastMessage = messages[messages.length - 1];
  const lastIsThinking = () => {
    const parts = lastMessage?.parts ?? [];
    return parts.length > 0 && parts[parts.length - 1]?.kind === 'thinking';
  };
  for (const m of messages) {
    // A compaction marker is a system message, and the one system message worth drawing: it is
    // the reason the model no longer knows what is written a few rows above it (02 §6).
    if (m.role === 'system') {
      for (const part of m.parts) {
        if (part?.kind !== 'compacted') continue;
        pushItem({
          kind: 'compacted',
          id: m.id,
          replaced: part.replaced,
          summary: part.summary,
          artifacts: part.artifacts ?? [],
        });
      }
      continue;
    }
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
      } else if (part.kind === 'image') {
        // An image model answers with pictures; they belong in the reply at the point the model
        // produced them, not in an attachment tray at the end.
        blocks.push({
          kind: 'image',
          src:
            part.source.kind === 'base64'
              ? `data:${part.mime};base64,${part.source.data}`
              : undefined,
          blob: part.source.kind === 'blob' ? part.source.hash : undefined,
          mime: part.mime,
          alt: 'Picture from the model',
        });
      } else if (part.kind === 'audio' || part.kind === 'video') {
        blocks.push({
          kind: part.kind,
          src:
            part.source.kind === 'base64'
              ? `data:${part.mime};base64,${part.source.data}`
              : undefined,
          blob: part.source.kind === 'blob' ? part.source.hash : undefined,
          mime: part.mime,
        });
      } else if (part.kind === 'tool_call') {
        pushItem(callItem(part, calls[part.id], titles, output[part.id]));
      } else if (part.kind === 'provider_opaque') {
        applyOpaque(part, webRows, pushItem);
      }
    }
  }
  // Calls the stream announced but whose block has not been finalised yet. A folded sub-agent
  // row answers for every call in it, not only the one whose id it carries — otherwise the
  // second and third calls of a round are added again beside the row that already shows them.
  const shown = new Set(
    blocks.flatMap((b) =>
      b.kind === 'activity'
        ? b.items.flatMap((i) => (i.kind === 'subagents' ? i.runs.map((r) => r.id) : [i.id]))
        : [],
    ),
  );
  for (const c of Object.values(calls)) {
    if (!shown.has(c.id) && c.message_id === lastMessage?.id)
      pushItem(callItem(undefined, c, titles, output[c.id]));
  }
  blocks.push(...artifactCards(blocks, titles));
  for (const p of pending) {
    if (p.payload.kind === 'permission')
      blocks.push({ kind: 'permission', permission: permissionOf(p) });
    else if (p.payload.kind === 'access_request') blocks.push({ kind: 'access', ask: accessOf(p) });
    else if (p.payload.kind === 'connector_suggestion')
      blocks.push({ kind: 'offer', offer: offerOf(p) });
    else if (p.payload.kind === 'elicitation')
      blocks.push({ kind: 'elicit', ask: elicitationOf(p) });
    else if (p.payload.kind === 'skill_proposal')
      blocks.push({ kind: 'skillProposal', id: p.id, proposal: p.payload.proposal });
    else if (p.payload.kind === 'memory_proposal')
      blocks.push({ kind: 'memoryProposal', id: p.id, proposal: p.payload.proposal });
  }
  return blocks;
}

/**
 * One card per artifact the turn created or changed, once the call has finished and the
 * artifact exists (13 §10).
 *
 * **The version is the one this turn produced**, not the newest one there is. A card sits under
 * the reply that made it, and in a chat where an artifact was revised three times, three cards
 * all stamped `v3` say the same untrue thing twice: they report the present, where the feed is
 * a record of what happened (05 §1). The version comes from the call's own result, and only
 * falls back to the index when a result carried none.
 *
 * The title and type still come from the index, because those are not facts about the turn —
 * renaming an artifact should rename it everywhere it is referred to, the way the panel's tab
 * and the library already do.
 */
function artifactCards(blocks: Block[], titles: ArtifactIndex): Block[] {
  const cards = new Map<string, Extract<Block, { kind: 'artifact' }>>();
  for (const b of blocks) {
    if (b.kind !== 'activity') continue;
    for (const item of b.items) {
      if (item.kind !== 'artifact' || !item.artifactId || item.status !== 'done') continue;
      const prev = cards.get(item.artifactId);
      // A turn that both created and edited an artifact shows the last version it left behind.
      const made = Math.max(item.version, prev?.version ?? 0);
      cards.set(item.artifactId, {
        kind: 'artifact',
        artifactId: item.artifactId,
        title: titles[item.artifactId]?.title ?? item.title,
        type: titles[item.artifactId]?.type ?? item.type,
        version: made || (titles[item.artifactId]?.version ?? 1),
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
  output?: string[],
): ActivityItem {
  const id = call?.id ?? part?.id ?? '';
  const [connector, tool] = call ? [call.connector, call.tool] : splitName(part?.name ?? '');
  const modelName = call?.model_tool_name ?? part?.name ?? '';
  if (isArtifactTool(modelName)) return artifactItem(id, tool, call, part, titles);
  // A sub agent is not "using a connector": it is a conversation this one started, and the row
  // is a door into it rather than a line about a tool (18 §7).
  if (connector === 'subagents')
    return { kind: 'subagents', id, runs: [subAgentRun(id, call, part)] };
  // The file connectors have richer rows than "used a tool": a read with its line range, a
  // search with its count, an edit with its diff (16 §6).
  // The guard decides about a command far more often than about anything else, so the rows a
  // command becomes are the rows its mark matters most on (04 §6). It rides along here rather
  // than inside `fileItem`, which builds the row and has no opinion about who allowed it.
  const file = call ? fileItem(call, output) : undefined;
  if (file) {
    file.guard = guardOf(call);
    return file;
  }
  return {
    kind: 'connector',
    id,
    connector,
    connectorName: call?.connector_name,
    tool,
    title: runtimeTitle(connector, tool, call?.args ?? part?.args, resultJson(call)),
    summary: rowSummary(connector, tool, call, part),
    status: rowStatus(call),
    blocked: blockedBy(call),
    guard: guardOf(call),
    tier: call?.tier,
    args: call?.args ?? part?.args,
    result: call?.result ?? undefined,
    hasWholeOutput: call?.result_blob_hash != null,
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
/**
 * One sub agent as the parent's row shows it (18 §7).
 *
 * What it was asked comes from the call's arguments, so the row says something while it runs;
 * what it cost comes from the structured result, which the connector fills in with the type,
 * the seconds, the tokens and the transcript's id — the id being what opens the tree at it.
 */
function subAgentRun(
  id: string,
  call: ToolCallDto | undefined,
  part: Extract<ContentPart, { kind: 'tool_call' }> | undefined,
): SubAgentRun {
  const args = (call?.args ?? part?.args ?? {}) as Record<string, unknown>;
  const result = resultJson(call);
  const str = (v: unknown) => (typeof v === 'string' && v.length > 0 ? v : undefined);
  const num = (v: unknown) => (typeof v === 'number' ? v : undefined);
  return {
    id,
    agent: str(args.agent) ?? 'sub agent',
    task: str(args.task) ?? '',
    status: rowStatus(call),
    chatId: str(result?.transcript),
    seconds: num(result?.seconds),
    tokens: num(result?.tokens),
  };
}

/** What the sub agents under this turn spent, for the footer's second number. */
function subTokens(blocks: Block[]): number | undefined {
  let total = 0;
  for (const b of blocks) {
    if (b.kind !== 'activity') continue;
    for (const item of b.items) {
      if (item.kind !== 'subagents') continue;
      for (const run of item.runs) total += run.tokens ?? 0;
    }
  }
  return total > 0 ? total : undefined;
}

/**
 * The structured half of a result, wherever it sits in it.
 *
 * Not only the first part: a connector that answers with prose *and* the facts about it — the
 * sub-agent tool hands back the report and then what it cost — puts the text first, because
 * the text is what the model came for.
 */
function resultJson(call: ToolCallDto | undefined): Record<string, unknown> | undefined {
  for (const part of call?.result ?? []) {
    if (part.kind === 'json' && part.json && typeof part.json === 'object') {
      return part.json as Record<string, unknown>;
    }
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

/**
 * Gantry's own tools, said as what they do (04 §2: they are `app` tier because they act only on
 * Gantry's own state). Every other connector is named by its name, which is the useful thing to
 * say about it; ours is called Gantry, and "Using Gantry" is not.
 */
function runtimeTitle(
  connector: string,
  tool: string,
  args: unknown,
  result?: Record<string, unknown>,
): string | undefined {
  if (connector !== 'gantry') return undefined;
  const named = (key: string) => {
    const v = args && typeof args === 'object' ? (args as Record<string, unknown>)[key] : undefined;
    return typeof v === 'string' && v.length > 0 ? v : undefined;
  };
  switch (tool) {
    case 'request_access': {
      const name = named('connector') ?? 'a connector';
      // In Auto nobody is asked (04 §9), so the row says what happened rather than what was
      // requested: the request and its answer are the same moment once it has one.
      if (result?.attached === true) return `Attached ${name}`;
      if (result?.attached === false) return `Did not attach ${name}`;
      return `Asked to attach ${name}`;
    }
    case 'suggest_connector': {
      const name = named('catalog_id');
      return name ? `Offered to install ${name}` : 'Offered a connector';
    }
    case 'search_connectors':
      return 'Looked through the connector catalog';
    case 'clock':
      return 'Checked the date and time';
    default:
      return undefined;
  }
}

/**
 * What the row says beside its title.
 *
 * Three things in order of what a person wants to know. A call that failed says why it failed:
 * the model was told, and a row that shows only a red cross leaves the fold's "2 failed" as a
 * count of mysteries. One of Gantry's own tools says its reason rather than its arguments,
 * because `runtimeTitle` has already said the rest of them. Everything else keeps the
 * connector's own summary, which is the arguments, which for a server Gantry knows nothing
 * about is the honest thing to show.
 */
function rowSummary(
  connector: string,
  tool: string,
  call: ToolCallDto | undefined,
  part: Extract<ContentPart, { kind: 'tool_call' }> | undefined,
): string {
  const failure = errorText(call);
  if (failure) return failure;
  const reason = runtimeReason(connector, tool, call?.args ?? part?.args);
  if (reason !== undefined) return reason;
  return call?.display.summary ?? '';
}

/** The message a failed call returned, which is the message the model was given. */
function errorText(call: ToolCallDto | undefined): string | undefined {
  if (!call?.is_error) return undefined;
  for (const p of call.result ?? []) {
    if (p.kind === 'text' && p.text.trim() !== '') return p.text.trim();
  }
  return undefined;
}

/** The `reason` a Gantry tool was given, for the tools whose title has said everything else. */
function runtimeReason(connector: string, tool: string, args: unknown): string | undefined {
  if (connector !== 'gantry') return undefined;
  if (tool !== 'request_access' && tool !== 'suggest_connector') return undefined;
  const v = args && typeof args === 'object' ? (args as Record<string, unknown>).reason : undefined;
  return typeof v === 'string' ? v : '';
}

/** What the guard decided, when the guard was asked (04 §6). */
function guardOf(call: ToolCallDto | undefined): GuardMark | undefined {
  const j = call?.judge;
  if (!j) return undefined;
  return {
    ok: j.decision === 'allow',
    reason: j.reason,
    overridden: j.overridden ?? false,
    wrong: j.wrong ?? undefined,
  };
}

/** The guardrail's own reason, when a guardrail is what refused the call (04 §5). */
function blockedBy(call: ToolCallDto | undefined): string | undefined {
  if (call?.status !== 'denied' || call.decision_source !== 'guardrail') return undefined;
  const message = resultJson(call)?.message;
  return typeof message === 'string' ? message : 'A guardrail refused this call.';
}

function splitName(name: string): [string, string] {
  const i = name.indexOf('__');
  return i > 0 ? [name.slice(0, i), name.slice(i + 2)] : ['', name];
}

/** A pending permission interaction as the card renders it (04 §7, §8). */
export function permissionOf(i: Interaction): Permission {
  if (i.payload.kind !== 'permission') throw new Error('not a permission');
  const r = i.payload.request;
  const choices = (r.choices ?? []).map((c) => ({
    key: c.key,
    label: c.label,
    value: c.value ?? undefined,
    options: (c.options ?? []).map((o) => ({
      value: o.value,
      label: o.label,
      detail: o.detail ?? undefined,
    })),
    note: c.note ?? undefined,
  }));
  const args: Record<string, string> = {};
  if (r.args && typeof r.args === 'object' && !Array.isArray(r.args)) {
    for (const [k, v] of Object.entries(r.args as Record<string, unknown>)) {
      // An argument with a control of its own is shown by that control, resolved; listing the
      // model twice — once as the model typed it, once as it will run — invites reading the
      // wrong one.
      if (choices.some((c) => c.key === k)) continue;
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
    guardrail: r.guardrail ? { rule: r.guardrail.rule, reason: r.guardrail.reason } : undefined,
    guard: r.guard ?? undefined,
    why: r.why ?? undefined,
    scopes: [{ id: 'once', label: 'Allow once' }, ...r.scopes.map((s) => scopeOption(s, r.tool))],
    choices,
  };
}

/** A pending access request as its card renders it (04 §9). */
export function accessOf(i: Interaction): AccessAsk {
  if (i.payload.kind !== 'access_request') throw new Error('not an access request');
  const r = i.payload.request;
  return {
    id: i.id,
    connector: r.connector,
    connectorName: r.connector_name,
    tools: r.tools,
    toolCount: r.tool_count,
    reason: r.reason,
  };
}

/** A pending connector suggestion as its card renders it (03 §9). */
export function offerOf(i: Interaction): ConnectorOffer {
  if (i.payload.kind !== 'connector_suggestion') throw new Error('not a suggestion');
  const s = i.payload.suggestion;
  return {
    id: i.id,
    catalogId: s.catalog_id,
    name: s.name,
    description: s.description,
    auth: s.auth,
    requires: s.requires.map((r) => `${r.name} ${r.version}`),
    reason: s.reason,
  };
}

/** A server's mid-call question as its card renders it (03 §6). */
export function elicitationOf(i: Interaction): ElicitationAsk {
  if (i.payload.kind !== 'elicitation') throw new Error('not an elicitation');
  const r = i.payload.request;
  return {
    id: i.id,
    connector: r.connector,
    connectorName: r.connector_name,
    message: r.message,
    fields: r.fields,
  };
}

/**
 * A grant scope as the card's dropdown shows it (04 §8).
 *
 * The scope itself travels with the option, because an argument scope carries the prefix it
 * will grant: the label and what is granted come from one value, so the card cannot promise a
 * folder and grant a different one.
 */
function scopeOption(
  scope: GrantScope,
  tool: string,
): { id: string; label: string; grant: GrantScope } {
  switch (scope.kind) {
    case 'all_reads':
      return { id: 'all_reads', label: 'Allow all reads for this chat', grant: scope };
    case 'path_prefix':
      return {
        id: 'path_prefix',
        label: `Allow ${tool} under ${scope.prefix} for this chat`,
        grant: scope,
      };
    case 'command_prefix':
      return {
        id: 'command_prefix',
        label: `Allow \`${scope.prefix}…\` for this chat`,
        grant: scope,
      };
    default:
      return { id: 'tool', label: `Allow ${tool} for this chat`, grant: scope };
  }
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

/**
 * A provider's own server tools, which arrive as opaque blocks the app persists and replays but
 * does not interpret (02 §5). Web search is the one it names, because it is the one the user has
 * to be able to see: nothing else in the transcript says where the pages in the answer came
 * from, and a search Gantry never ran is a search no permission card ever mentioned.
 *
 * Every other block kind — citations, encrypted reasoning, a server tool this version has never
 * heard of — is left alone rather than guessed at. Opaque means opaque.
 */
function applyOpaque(
  part: Extract<ContentPart, { kind: 'provider_opaque' }>,
  rows: Map<string, Extract<ActivityItem, { kind: 'web' }>>,
  pushItem: (item: ActivityItem) => void,
): void {
  const json = (part.json ?? {}) as Record<string, unknown>;
  const str = (v: unknown): string | undefined => (typeof v === 'string' ? v : undefined);
  const obj = (v: unknown): Record<string, unknown> =>
    v && typeof v === 'object' ? (v as Record<string, unknown>) : {};

  switch (part.block_kind) {
    // Anthropic: the call, whose results arrive as their own block below.
    case 'server_tool_use': {
      if (str(json.name) !== 'web_search') return;
      const id = str(json.id) ?? `web-${rows.size}`;
      const row: Extract<ActivityItem, { kind: 'web' }> = {
        kind: 'web',
        id,
        query: str(obj(json.input).query),
        results: [],
        status: 'running',
      };
      rows.set(id, row);
      pushItem(row);
      return;
    }
    case 'web_search_tool_result': {
      const row = rows.get(str(json.tool_use_id) ?? '');
      if (!row) return;
      const content = Array.isArray(json.content) ? json.content : [];
      row.results = content
        .map((r) => obj(r))
        .filter((r) => str(r.url) !== undefined)
        .map((r) => ({ title: str(r.title) ?? str(r.url) ?? '', url: str(r.url) ?? '' }));
      row.status = 'done';
      return;
    }
    // OpenAI Responses: one item, which carries its own status and no results.
    case 'web_search_call': {
      pushItem({
        kind: 'web',
        id: str(json.id) ?? `web-${rows.size}`,
        query: str(obj(json.action).query),
        results: [],
        status: str(json.status) === 'completed' ? 'done' : 'running',
      });
      return;
    }
    default:
  }
}
