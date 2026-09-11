import { describe, expect, it } from 'vitest';

import type { AgentEvent, AgentEventKind, ChatDetail, Interaction, TurnDto } from '@/bindings';
import { applyBatch, fresh } from '@/lib/stores/runStore';
import { toTurns } from '@/lib/view/toTurns';

const TURN = '01TURN';
const MSG = '01MSG';
const CALL = 'call_1';

let seq = 0;
const ev = (event: AgentEventKind): AgentEvent => ({
  seq: ++seq,
  ts: 1000 + seq,
  turn_id: TURN,
  event,
});

const interaction: Interaction = {
  id: '01INT',
  chat_id: 'c1',
  turn_id: TURN,
  kind: 'permission',
  payload: {
    kind: 'permission',
    request: {
      call_id: CALL,
      connector: 'gantry',
      connector_name: 'Gantry',
      tool: 'clock',
      model_tool_name: 'gantry__clock',
      tier: 'read',
      args: {},
      display: { kind: 'read', summary: '' },
      guardrail: null,
      why: 'Let me check',
      description: 'The time',
      scopes: ['tool', 'all_reads'],
    },
  },
  status: 'pending',
  resolution: null,
  created_at: 1,
  resolved_at: null,
};

const chat = (turn: Partial<TurnDto>): ChatDetail => ({
  id: 'c1',
  title: 't',
  pinned: false,
  archived: false,
  project_id: null,
  surface: 'chat',
  roots: [],
  created_at: 0,
  last_message_at: 0,
  model: { provider: 'openrouter', model: 'm' },
  mode: 'manual',
  guard: true,
  effort: 'off',
  web_search: false,
  active_turn: TURN,
  turns: [
    {
      id: TURN,
      status: 'running',
      model: { provider: 'openrouter', model: 'm' },
      user: {
        id: 'u',
        role: 'user',
        parts: [{ kind: 'text', text: 'time?' }],
        origin: null,
        created_at: 0,
      },
      messages: [],
      tool_calls: [],
      notices: [],
      usage: null,
      stop_reason: null,
      error: null,
      feedback: null,
      started_at: 0,
      ended_at: null,
      ...turn,
    },
  ],
});

describe('the run store follows a tool call through a permission prompt', () => {
  it('shows the row, then the card, then the result', () => {
    seq = 0;
    let live = fresh(TURN);
    live = applyBatch(live, {
      turn_id: TURN,
      events: [
        ev({ type: 'message.started', message_id: MSG, role: 'assistant' }),
        ev({ type: 'text.delta', message_id: MSG, block: 0, text: 'Let me check. ' }),
        ev({
          type: 'tool_call.started',
          call_id: CALL,
          message_id: MSG,
          connector: 'gantry',
          connector_name: 'Gantry',
          tool: 'clock',
          model_tool_name: 'gantry__clock',
        }),
        ev({
          type: 'tool_call.ready',
          call_id: CALL,
          args: {},
          tier: 'read',
          display: { kind: 'read', summary: '' },
        }),
        ev({ type: 'decision.requested', interaction }),
      ],
    });
    expect(live.callOrder).toEqual([CALL]);
    expect(live.calls[CALL]?.status).toBe('awaiting_decision');
    expect(live.pending).toHaveLength(1);

    let turns = toTurns(chat({}), live, () => 'm');
    expect(turns[0]?.status).toBe('waiting');
    const kinds = turns[0]?.blocks.map((b) => b.kind);
    expect(kinds).toEqual(['text', 'activity', 'permission']);
    const activity = turns[0]?.blocks[1];
    expect(
      activity?.kind === 'activity' &&
        activity.items[0]?.kind === 'connector' &&
        activity.items[0].status,
    ).toBe('waiting');
    const card = turns[0]?.blocks[2];
    expect(card?.kind === 'permission' && card.permission.title).toBe('Gantry wants to run clock');
    expect(card?.kind === 'permission' && card.permission.why).toBe('Let me check');

    live = applyBatch(live, {
      turn_id: TURN,
      events: [
        ev({
          type: 'decision.resolved',
          interaction_id: interaction.id,
          resolution: {
            kind: 'permission',
            decision: { kind: 'allow_once' },
            message: null,
          },
          source: 'user_once',
        }),
        ev({ type: 'tool_call.executing', call_id: CALL, source: 'user_once' }),
        ev({
          type: 'tool_call.completed',
          call_id: CALL,
          status: 'completed',
          is_error: false,
          duration_ms: 2,
          result_preview: '{"date":"2026-09-07"}',
          result: [{ kind: 'json', json: { date: '2026-09-07' } }],
        }),
        ev({
          type: 'block.done',
          message_id: MSG,
          block: 1,
          part: { kind: 'tool_call', id: CALL, name: 'gantry__clock', args: {} },
        }),
        ev({ type: 'message.started', message_id: '01MSG2', role: 'assistant' }),
        ev({ type: 'text.delta', message_id: '01MSG2', block: 0, text: 'It is Monday.' }),
        ev({
          type: 'turn.completed',
          status: 'completed',
          usage: null,
          duration_ms: 5,
          tool_calls: 1,
        }),
      ],
    });
    expect(live.pending).toHaveLength(0);
    expect(live.status).toBe('completed');
    expect(live.messages).toHaveLength(2);
    turns = toTurns(chat({}), live, () => 'm');
    expect(turns[0]?.blocks.map((b) => b.kind)).toEqual(['text', 'activity', 'text']);
    const row = turns[0]?.blocks[1];
    expect(
      row?.kind === 'activity' && row.items[0]?.kind === 'connector' && row.items[0].status,
    ).toBe('done');
    expect(
      row?.kind === 'activity' && row.items[0]?.kind === 'connector' && row.items[0].result,
    ).toEqual([{ kind: 'json', json: { date: '2026-09-07' } }]);
  });

  it('ignores events at or below the snapshot sequence', () => {
    seq = 0;
    let live = fresh(TURN);
    live = applyBatch(live, {
      turn_id: TURN,
      events: [
        {
          seq: 10,
          ts: 1,
          turn_id: TURN,
          event: {
            type: 'turn.snapshot',
            snapshot: {
              chat_id: 'c1',
              status: 'running',
              messages: [
                {
                  id: MSG,
                  role: 'assistant',
                  parts: [{ kind: 'text', text: 'so far' }],
                  origin: null,
                  created_at: 0,
                },
              ],
              tool_calls: [],
              pending: [],
              usage: null,
              started_at: 0,
              seq: 10,
            },
          },
        },
        {
          seq: 9,
          ts: 2,
          turn_id: TURN,
          event: { type: 'text.delta', message_id: MSG, block: 0, text: 'OLD' },
        },
        {
          seq: 11,
          ts: 3,
          turn_id: TURN,
          event: { type: 'text.delta', message_id: MSG, block: 0, text: ' and more' },
        },
      ],
    });
    expect(live.messages[0]?.parts[0]).toEqual({ kind: 'text', text: 'so far and more' });
  });
});

describe('a running command shows what it has printed', () => {
  it('keeps the live window while it runs and drops it when the result arrives', () => {
    const CMD = 'call_cmd';
    let live = fresh(TURN);
    live = applyBatch(live, {
      turn_id: TURN,
      events: [
        ev({ type: 'message.started', message_id: MSG, role: 'assistant' }),
        ev({
          type: 'tool_call.started',
          call_id: CMD,
          message_id: MSG,
          connector: 'shell',
          connector_name: 'Shell',
          tool: 'run_command',
          model_tool_name: 'shell__run_command',
        }),
        ev({
          type: 'tool_call.ready',
          call_id: CMD,
          args: { command: 'cargo test' },
          tier: 'execute',
          display: { kind: 'command', summary: 'cargo test' },
        }),
        ev({ type: 'tool_call.executing', call_id: CMD, source: 'mode' }),
        // Chunks are not lines: one read can carry half a line, and the next its other half.
        ev({ type: 'tool_call.output', call_id: CMD, stream: 'stdout', chunk: 'running 3 te' }),
        ev({ type: 'tool_call.output', call_id: CMD, stream: 'stdout', chunk: 'sts\nok 1\n' }),
        ev({ type: 'tool_call.output', call_id: CMD, stream: 'stderr', chunk: 'warning: x\n' }),
      ],
    });
    expect(live.output[CMD]).toEqual(['running 3 tests', 'ok 1', 'warning: x', '']);

    const row = toTurns(
      {
        id: 'c1',
        turns: [
          {
            id: TURN,
            status: 'running',
            model: { provider: 'openrouter', model: 'm' },
            user: { id: 'u', role: 'user', parts: [], origin: 'user', created_at: 1 },
            messages: [],
            usage: null,
            stop_reason: null,
            error: null,
            started_at: 1,
            ended_at: null,
            feedback: null,
            attachments: [],
          },
        ],
      } as never,
      live,
      () => 'M',
    )[0];
    const activity = row?.blocks.find((b) => b.kind === 'activity');
    const item = activity?.kind === 'activity' ? activity.items[0] : undefined;
    expect(item?.kind).toBe('command');
    expect(item?.kind === 'command' && item.status).toBe('running');
    expect(item?.kind === 'command' && item.output).toEqual([
      'running 3 tests',
      'ok 1',
      'warning: x',
      '',
    ]);

    live = applyBatch(live, {
      turn_id: TURN,
      events: [
        ev({
          type: 'tool_call.completed',
          call_id: CMD,
          status: 'completed',
          is_error: false,
          duration_ms: 900,
          result_preview: '{}',
          result: [
            {
              kind: 'json',
              json: {
                command: 'cargo test',
                cwd: '/repo',
                exit_code: 0,
                stdout: 'running 3 tests\nok 1\n',
                stderr: '',
                duration_ms: 900,
              },
            },
          ],
        }),
      ],
    });
    // The result carries the output now; keeping the window as well would double it.
    expect(live.output[CMD]).toBeUndefined();
  });
});
