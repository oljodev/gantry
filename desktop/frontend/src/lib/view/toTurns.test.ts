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
