import { describe, expect, it } from 'vitest';

import type { ToolCallDto, ToolCallStatus } from '@/bindings';
import { fileItem } from '@/lib/view/fileTools';

function call(status: ToolCallStatus, extra: Partial<ToolCallDto> = {}): ToolCallDto {
  return {
    id: 'c1',
    chat_id: 'chat',
    turn_id: 'turn',
    message_id: 'm1',
    connector: 'shell',
    connector_name: 'Shell',
    tool: 'run_command',
    model_tool_name: 'shell__run_command',
    args: { command: 'rm -rf build', cwd: '/w' },
    tier: 'execute',
    status,
    decision_source: null,
    judge: null,
    display: { kind: 'command', summary: '$ rm -rf build' },
    result_preview: null,
    result: null,
    is_error: false,
    started_at: null,
    ended_at: null,
    duration_ms: null,
    ...extra,
  };
}

describe('the rows a file or command call draws (16 §6)', () => {
  it('shows a running command as running, with the lines that have arrived', () => {
    expect(fileItem(call('running'), ['compiling…'])).toMatchObject({
      kind: 'command',
      command: 'rm -rf build',
      cwd: '/w',
      output: ['compiling…'],
      status: 'running',
    });
  });

  it('does not leave a command that ended badly spinning for ever', () => {
    expect(fileItem(call('failed'))).toMatchObject({ kind: 'command', status: 'failed' });
  });

  // Both of these used to come out as `failed`, which put a red cross on a command that had
  // not run and made the fold above it count a failure the turn never had.
  it('separates a command that is waiting or was stopped from one that failed', () => {
    expect(fileItem(call('awaiting_decision'))).toMatchObject({
      kind: 'command',
      status: 'waiting',
    });
    expect(fileItem(call('cancelled'))).toMatchObject({ kind: 'command', status: 'cancelled' });
  });

  it('draws nothing for a call that was refused, so the row can say who refused it', () => {
    expect(fileItem(call('denied', { decision_source: 'guardrail' }))).toBeUndefined();
    expect(
      fileItem(
        call('denied', {
          connector: 'filesystem',
          tool: 'read_file',
          model_tool_name: 'filesystem__read_file',
          args: { path: '/home/olav/.ssh/id_ed25519' },
        }),
      ),
    ).toBeUndefined();
  });
});
