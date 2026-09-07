import { SCRIPT, USER_MESSAGE, CPS, type Tool } from './script';

export interface Msg { id: string; text: string; shown: number; streaming: boolean }
export interface Row { id: string; tool: Tool; action: string; state: 'running' | 'done' | 'asking' | 'allowed'; result?: string }
export interface Prompt { id: string; tool: Tool; action: string; detail: string; approved: boolean }
export interface Frame { user: string; messages: Msg[]; rows: Row[]; prompt: Prompt | null; done: boolean }

/** Pure: the same `t` gives the same frame on the server and in the browser. */
export function frameAt(t: number): Frame {
  const messages: Msg[] = [];
  const rows: Row[] = [];
  let prompt: Prompt | null = null;
  let done = false;
  for (const ev of SCRIPT) {
    if (ev.at > t) break;
    switch (ev.type) {
      case 'assistant': {
        const shown = Math.min(ev.text.length, Math.floor(((t - ev.at) * CPS) / 1000));
        messages.push({ id: ev.id, text: ev.text, shown, streaming: shown < ev.text.length });
        break;
      }
      case 'tool': {
        const finished = t >= ev.at + ev.duration;
        rows.push({ id: ev.id, tool: ev.tool, action: ev.action, state: finished ? 'done' : 'running', result: finished ? ev.result : undefined });
        break;
      }
      case 'prompt':
        prompt = { id: ev.id, tool: ev.tool, action: ev.action, detail: ev.detail, approved: false };
        rows.push({ id: `${ev.id}-ask`, tool: ev.tool, action: ev.action, state: 'asking' });
        break;
      case 'approve': {
        if (prompt && prompt.id === ev.id) prompt.approved = true;
        const ask = rows.find((r) => r.id === `${ev.id}-ask`);
        if (ask) { ask.state = 'allowed'; ask.result = 'allowed once'; }
        break;
      }
      case 'done':
        done = true;
        break;
    }
  }
  return { user: USER_MESSAGE, messages, rows, prompt, done };
}
