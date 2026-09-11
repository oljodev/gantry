import { connectorName } from '@/fixtures/connectors';
import type { ActivityItem } from '@/fixtures/types';

/**
 * "Created an artifact, read 3 files, ran a command": one fragment per kind of work. Work the
 * user stopped or refused is left out — it never happened — so a stopped turn can summarise to
 * nothing, and the caller says so instead.
 */
export function summarize(items: ActivityItem[]): string {
  const fragments: string[] = [];
  const reads = new Set<string>();
  const edits = new Set<string>();
  const created = new Set<string>();
  const updated = new Set<string>();
  const uses = new Map<string, number>();
  const own: string[] = [];
  let searches = 0;
  let commands = 0;
  let blocked = 0;
  const order: string[] = [];
  const note = (key: string) => {
    if (!order.includes(key)) order.push(key);
  };
  for (const item of items) {
    // A block is work that did not happen, and is worth saying so; an override is work that
    // did, and is counted with the rest (04 §6).
    if (item.kind === 'connector' && item.guard && !item.guard.ok && !item.guard.overridden) {
      blocked++;
      note('guard');
      continue;
    }
    if ('status' in item && (item.status === 'cancelled' || item.status === 'denied')) continue;
    // Nor has work the user has not answered for yet: "ran a command" is a claim about the past.
    if ('status' in item && item.status === 'waiting') continue;
    switch (item.kind) {
      case 'read':
        reads.add(item.path);
        note('read');
        break;
      case 'search':
        searches++;
        note('search');
        break;
      case 'edit':
        edits.add(item.path);
        note('edit');
        break;
      case 'command':
        commands++;
        note('command');
        break;
      case 'connector': {
        // Gantry's own tools already say what they did; a count of "used Gantry" does not.
        if (item.title) {
          own.push(item.title);
          note(`own:${own.length - 1}`);
          break;
        }
        const name = item.connectorName ?? connectorName(item.connector);
        uses.set(name, (uses.get(name) ?? 0) + 1);
        note(`use:${name}`);
        break;
      }
      case 'artifact':
        (item.action === 'updated' ? updated : created).add(item.artifactId ?? item.id);
        note(item.action === 'updated' ? 'updated' : 'created');
        break;
      default:
        break;
    }
  }
  for (const key of order) {
    if (key === 'read') fragments.push(`read ${count(reads.size, 'file')}`);
    else if (key === 'search')
      fragments.push(searches === 1 ? 'searched' : `searched ${searches} times`);
    else if (key === 'edit') fragments.push(`edited ${count(edits.size, 'file')}`);
    else if (key === 'command') fragments.push(`ran ${count(commands, 'command')}`);
    else if (key === 'created') fragments.push(`created ${count(created.size, 'artifact')}`);
    else if (key === 'updated') fragments.push(`updated ${count(updated.size, 'artifact')}`);
    else if (key === 'guard')
      fragments.push(blocked === 1 ? 'blocked by guard' : `blocked by guard ${blocked} times`);
    else if (key.startsWith('own:')) {
      const said = own[Number(key.slice(4))];
      if (said) fragments.push(said.charAt(0).toLowerCase() + said.slice(1));
    } else if (key.startsWith('use:')) {
      const name = key.slice(4);
      const n = uses.get(name) ?? 1;
      fragments.push(n === 1 ? `used ${name}` : `used ${name} ${n} times`);
    }
  }
  const text = fragments.join(', ');
  return text.charAt(0).toUpperCase() + text.slice(1);
}

function count(n: number, noun: string): string {
  if (n === 1) return `${noun === 'artifact' ? 'an' : 'a'} ${noun}`;
  return `${n} ${noun}s`;
}
