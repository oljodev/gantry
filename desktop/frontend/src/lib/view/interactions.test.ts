import { describe, expect, it } from 'vitest';

import type { Interaction } from '@/bindings';
import { accessOf, offerOf } from '@/lib/view/toTurns';

const base = {
  id: 'i1',
  chat_id: 'c1',
  turn_id: 't1',
  status: 'pending' as const,
  resolution: null,
  created_at: 0,
  resolved_at: null,
};

describe('the connector decisions (03 §9, 04 §9)', () => {
  it('reads an access request the way its card shows it', () => {
    const i: Interaction = {
      ...base,
      kind: 'access_request',
      payload: {
        kind: 'access_request',
        request: {
          instance_id: 'in1',
          connector: 'github',
          connector_name: 'GitHub',
          tools: ['list_issues'],
          tool_count: 44,
          reason: 'To read the open issues.',
        },
      },
    };
    expect(accessOf(i)).toEqual({
      id: 'i1',
      connector: 'github',
      connectorName: 'GitHub',
      tools: ['list_issues'],
      toolCount: 44,
      reason: 'To read the open issues.',
    });
    expect(() => offerOf(i)).toThrow();
  });

  it('reads a suggestion, runtimes and all', () => {
    const i: Interaction = {
      ...base,
      kind: 'connector_suggestion',
      payload: {
        kind: 'connector_suggestion',
        suggestion: {
          catalog_id: 'playwright',
          name: 'Playwright',
          description: 'Drives a browser.',
          category: 'developer',
          auth: 'none',
          requires: [{ name: 'node', version: '>=20' }],
          reason: 'You asked me to open a page.',
        },
      },
    };
    expect(offerOf(i)).toEqual({
      id: 'i1',
      catalogId: 'playwright',
      name: 'Playwright',
      description: 'Drives a browser.',
      auth: 'none',
      requires: ['node >=20'],
      reason: 'You asked me to open a page.',
    });
    expect(() => accessOf(i)).toThrow();
  });
});
