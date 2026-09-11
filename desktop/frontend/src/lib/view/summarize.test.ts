import { describe, expect, it } from 'vitest';

import type { ActivityItem } from '@/fixtures/types';
import { summarize } from '@/lib/view/summarize';

const read = (id: string, path: string): ActivityItem => ({ kind: 'read', id, path });

describe('summarize', () => {
  it('names each kind of work once, in the order it first happened', () => {
    const items: ActivityItem[] = [
      {
        kind: 'artifact',
        id: 'a',
        artifactId: 'A1',
        title: 'Dash',
        type: 'react',
        version: 1,
        action: 'created',
        status: 'done',
      },
      read('r1', 'src/a.ts'),
      read('r2', 'src/b.ts'),
      read('r3', 'src/a.ts'),
      { kind: 'command', id: 'c', command: 'pnpm test', cwd: '.', output: [], status: 'done' },
      {
        kind: 'artifact',
        id: 'b',
        artifactId: 'A1',
        title: 'Dash',
        type: 'react',
        version: 2,
        action: 'updated',
        status: 'done',
      },
    ];
    expect(summarize(items)).toBe(
      'Created an artifact, read 2 files, ran a command, updated an artifact',
    );
  });

  it('counts connector uses by name and ignores passing guards', () => {
    const items: ActivityItem[] = [
      {
        kind: 'connector',
        id: '1',
        connector: 'github',
        tool: 'list_issues',
        summary: '',
        status: 'done',
      },
      {
        kind: 'connector',
        id: '2',
        connector: 'github',
        tool: 'get_issue',
        summary: '',
        status: 'done',
      },
      { kind: 'search', id: 's', query: 'x', glob: '**', matches: 3 },
    ];
    expect(summarize(items)).toBe('Used GitHub 2 times, searched');
  });

  it('leaves out work the user stopped or refused', () => {
    const items: ActivityItem[] = [
      read('r1', 'src/a.ts'),
      {
        kind: 'artifact',
        id: 'a',
        title: 'artifact',
        type: 'artifact',
        version: 0,
        action: 'created',
        status: 'cancelled',
      },
      {
        kind: 'connector',
        id: '1',
        connector: 'github',
        tool: 'create_issue',
        summary: '',
        status: 'denied',
      },
    ];
    expect(summarize(items)).toBe('Read a file');
    expect(summarize(items.slice(1))).toBe('');
  });
});

describe('runtime tools', () => {
  it('says what Gantry did rather than that it used itself', () => {
    const items: ActivityItem[] = [
      {
        kind: 'connector',
        id: '1',
        connector: 'gantry',
        connectorName: 'Gantry',
        tool: 'request_access',
        title: 'Asked to attach shell',
        summary: '',
        status: 'done',
      },
      {
        kind: 'connector',
        id: '2',
        connector: 'github',
        tool: 'list_issues',
        summary: '',
        status: 'done',
      },
    ];
    expect(summarize(items)).toBe('Asked to attach shell, used GitHub');
  });
});
