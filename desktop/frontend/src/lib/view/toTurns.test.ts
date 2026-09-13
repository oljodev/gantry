import { describe, expect, it } from 'vitest';

import type { ChatDetail, JudgeVerdict, ToolCallDto, TurnDto } from '@/bindings';
import type { ActivityItem } from '@/fixtures/types';
import { toTurns } from '@/lib/view/toTurns';

function call(extra: Partial<ToolCallDto>): ToolCallDto {
  return {
    id: 'c1',
    chat_id: 'chat',
    turn_id: 'turn',
    message_id: 'm1',
    connector: 'filesystem',
    connector_name: 'Filesystem',
    tool: 'read_file',
    model_tool_name: 'filesystem__read_file',
    args: { path: '/proc/cpuinfo' },
    tier: 'read',
    status: 'completed',
    decision_source: null,
    judge: null,
    display: { kind: 'connector', summary: 'path=/proc/cpuinfo' },
    result_preview: null,
    result: null,
    is_error: false,
    started_at: null,
    ended_at: null,
    duration_ms: null,
    ...extra,
  } as ToolCallDto;
}

function chat(calls: ToolCallDto[]): ChatDetail {
  const turn: TurnDto = {
    id: 'turn',
    status: 'completed',
    model: { provider: 'openrouter', model: 'm' },
    user: { id: 'u1', role: 'user', parts: [{ kind: 'text', text: 'go' }] },
    messages: [
      {
        id: 'm1',
        role: 'assistant',
        parts: calls.map((c) => ({
          kind: 'tool_call',
          id: c.id,
          name: c.model_tool_name,
          args: c.args,
        })),
      },
    ],
    tool_calls: calls,
    notices: [],
    usage: null,
    stop_reason: null,
    error: null,
    feedback: null,
    started_at: 0,
    ended_at: 1,
  } as unknown as TurnDto;
  return {
    id: 'chat',
    surface: 'chat',
    roots: [],
    title: 'A chat',
    pinned: false,
    archived: false,
    project_id: null,
    created_at: 0,
    last_message_at: 0,
    model: { provider: 'openrouter', model: 'm' },
    mode: 'auto',
    guard: true,
    effort: 'off',
    web_search: false,
    active_turn: null,
    turns: [turn],
  } as unknown as ChatDetail;
}

/** A turn whose assistant message is the given parts verbatim, for the opaque-block cases. */
function partRows(parts: unknown[]): ActivityItem[] {
  const base = chat([]);
  const turn = {
    ...base.turns[0]!,
    messages: [{ id: 'm1', role: 'assistant', parts }],
  } as unknown as TurnDto;
  const [only] = toTurns({ ...base, turns: [turn] }, undefined, (r) => r.model);
  return only!.blocks.flatMap((b) => (b.kind === 'activity' ? b.items : []));
}

function rows(calls: ToolCallDto[]): ActivityItem[] {
  const [turn] = toTurns(chat(calls), undefined, (r) => r.model);
  return turn!.blocks.flatMap((b) => (b.kind === 'activity' ? b.items : []));
}

describe('what the guard decided, where the row can show it (04 §6)', () => {
  const verdict: JudgeVerdict = {
    decision: 'allow',
    confidence: 0.95,
    reason: 'Runs the silly command the user explicitly asked for.',
    flags: [],
    source: 'model',
    model: 'x',
    latency_ms: 900,
    overridden: false,
    wrong: null,
  };

  // A command is what the guard decides about most, and the command row is the one that could
  // not say so: its mark has to survive the projection into a first-party row.
  it('marks a command the guard allowed, not only a connector call', () => {
    const [row] = rows([
      call({
        connector: 'shell',
        connector_name: 'Shell',
        tool: 'run_command',
        model_tool_name: 'shell__run_command',
        args: { command: 'cowsay moo' },
        tier: 'execute',
        decision_source: 'judge',
        judge: verdict,
        display: { kind: 'command', summary: '$ cowsay moo' },
        result: [
          { kind: 'json', json: { command: 'cowsay moo', cwd: '/home/olav', exit_code: 0 } },
        ],
      }),
    ]);
    expect(row).toMatchObject({
      kind: 'command',
      command: 'cowsay moo',
      guard: { ok: true, reason: 'Runs the silly command the user explicitly asked for.' },
    });
  });

  it('leaves the mark off a call no guard decided', () => {
    const [row] = rows([
      call({
        connector: 'shell',
        connector_name: 'Shell',
        tool: 'run_command',
        model_tool_name: 'shell__run_command',
        args: { command: 'free -h' },
        tier: 'execute',
        decision_source: 'mode',
        display: { kind: 'command', summary: '$ free -h' },
        result: [{ kind: 'json', json: { command: 'free -h', cwd: '/home/olav', exit_code: 0 } }],
      }),
    ]);
    expect(row).toMatchObject({ kind: 'command', guard: undefined });
  });
});

describe('what a row says beside its title (05 §1)', () => {
  it('gives a failed call the reason it failed, not the arguments it was given', () => {
    const [row] = rows([
      call({
        is_error: true,
        result: [
          { kind: 'text', text: '/proc/cpuinfo is outside the folders attached to this chat.' },
        ],
      }),
    ]);
    expect(row).toMatchObject({
      kind: 'connector',
      status: 'failed',
      summary: '/proc/cpuinfo is outside the folders attached to this chat.',
    });
  });

  it('keeps the connector’s own summary when the call went through', () => {
    const [row] = rows([
      call({
        connector: 'weather',
        connector_name: 'Weather',
        tool: 'forecast',
        model_tool_name: 'weather__forecast',
        args: { city: 'Oslo' },
        display: { kind: 'connector', summary: 'city=Oslo' },
        result: [{ kind: 'json', json: {} }],
      }),
    ]);
    expect(row).toMatchObject({ kind: 'connector', status: 'done', summary: 'city=Oslo' });
  });

  // The title already says which connector was asked for; repeating `connector=shell` beside it
  // spends the row's one line on something the reader has just read.
  it('says why Gantry asked to attach a connector, since the title says which', () => {
    const [row] = rows([
      call({
        connector: 'gantry',
        connector_name: 'Gantry',
        tool: 'request_access',
        model_tool_name: 'gantry__request_access',
        args: { connector: 'shell', reason: 'I need to run hardware inspection commands.' },
        display: { kind: 'connector', summary: 'connector=shell reason=I need to run hardware…' },
        result: [{ kind: 'json', json: {} }],
      }),
    ]);
    expect(row).toMatchObject({
      title: 'Asked to attach shell',
      summary: 'I need to run hardware inspection commands.',
    });
  });
});

describe('the artifact card reports its own turn (13 §10)', () => {
  const update = (version: number): ToolCallDto =>
    call({
      connector: 'gantry',
      connector_name: 'Gantry',
      tool: 'update_artifact',
      model_tool_name: 'gantry__update_artifact',
      args: { artifact_id: 'a1', content: '…' },
      result: [{ kind: 'json', json: { artifact_id: 'a1', version } }],
      display: { kind: 'connector', summary: 'a1' },
    } as Partial<ToolCallDto>);

  const cardOf = (calls: ToolCallDto[], titles = {}) => {
    const [turn] = toTurns(chat(calls), undefined, (r) => r.model, titles);
    return turn!.blocks.find((b) => b.kind === 'artifact');
  };

  it('shows the version that turn left behind, not the newest one there is', () => {
    // The chat has since reached v3; this turn made v2, and that is what its card says.
    const card = cardOf([update(2)], { a1: { title: 'Invite', type: 'react', version: 3 } });
    expect(card).toMatchObject({ version: 2, title: 'Invite', type: 'react' });
  });

  it('takes the last version of a turn that changed it twice', () => {
    const card = cardOf([update(2), { ...update(3), id: 'c2' }]);
    expect(card).toMatchObject({ version: 3 });
  });

  it('falls back to the index when the call carried no version', () => {
    const noVersion = { ...update(0), result: [{ kind: 'json', json: { artifact_id: 'a1' } }] };
    const card = cardOf([noVersion as ToolCallDto], {
      a1: { title: 'Invite', type: 'react', version: 3 },
    });
    expect(card).toMatchObject({ version: 3 });
  });
});

describe("a provider's own web search, which no permission card ever mentioned (02 §5)", () => {
  it('shows the query and the pages it came back with', () => {
    const items = partRows([
      {
        kind: 'provider_opaque',
        provider: 'anthropic',
        block_kind: 'server_tool_use',
        json: { id: 'srvtoolu_01', name: 'web_search', input: { query: 'gantry crane' } },
      },
      {
        kind: 'provider_opaque',
        provider: 'anthropic',
        block_kind: 'web_search_tool_result',
        json: {
          tool_use_id: 'srvtoolu_01',
          content: [
            { type: 'web_search_result', title: 'Gantry crane', url: 'https://example.org/g' },
          ],
        },
      },
    ]);
    expect(items).toHaveLength(1);
    const row = items[0]!;
    expect(row.kind).toBe('web');
    if (row.kind !== 'web') return;
    expect(row.query).toBe('gantry crane');
    expect(row.status).toBe('done');
    expect(row.results).toEqual([{ title: 'Gantry crane', url: 'https://example.org/g' }]);
  });

  it('reads the Responses shape too, which reports one item and no results', () => {
    const items = partRows([
      {
        kind: 'provider_opaque',
        provider: 'openai_responses',
        block_kind: 'web_search_call',
        json: { id: 'ws_01', status: 'completed', action: { type: 'search', query: 'gantry' } },
      },
    ]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: 'web', query: 'gantry', status: 'done' });
  });

  it('leaves every other opaque block alone rather than guessing at it', () => {
    const items = partRows([
      {
        kind: 'provider_opaque',
        provider: 'anthropic',
        block_kind: 'code_execution_tool_result',
        json: { tool_use_id: 'x', content: [] },
      },
    ]);
    expect(items).toEqual([]);
  });
});
