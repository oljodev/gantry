import { describe, expect, it } from 'vitest';

import type { Interaction } from '@/bindings';
import { elicitationOf } from '@/lib/view/toTurns';

function interaction(payload: Interaction['payload']): Interaction {
  return {
    id: 'i1',
    chat_id: 'c1',
    turn_id: 't1',
    kind: 'elicitation',
    payload,
    status: 'pending',
    resolution: null,
    created_at: 0,
    resolved_at: null,
  } as unknown as Interaction;
}

describe('a server asking mid-call (03 §6)', () => {
  it('carries the server’s own words and its form through to the card', () => {
    const ask = elicitationOf(
      interaction({
        kind: 'elicitation',
        request: {
          call_id: 'call-1',
          connector: 'linear',
          connector_name: 'Linear',
          message: 'Which team should this issue go to?',
          fields: [
            {
              key: 'team',
              kind: 'enum',
              title: 'Team',
              description: null,
              required: true,
              options: [
                { value: 'eng', label: 'Engineering' },
                { value: 'des', label: 'Design' },
              ],
              format: null,
            },
          ],
        },
      }),
    );
    expect(ask).toMatchObject({
      id: 'i1',
      connector: 'linear',
      connectorName: 'Linear',
      message: 'Which team should this issue go to?',
    });
    expect(ask.fields[0]?.options.map((o) => o.value)).toEqual(['eng', 'des']);
  });

  it('refuses to read an interaction of another kind as one', () => {
    expect(() =>
      elicitationOf(
        interaction({
          kind: 'connector_suggestion',
          suggestion: {},
        } as unknown as Interaction['payload']),
      ),
    ).toThrow();
  });
});
