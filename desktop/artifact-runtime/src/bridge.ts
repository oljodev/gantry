/**
 * The artifact side of the postMessage protocol (docs/plan/13 §5). The parent sends `mount`,
 * `update` and `theme`; the document answers with `ready`, `error`, `console`, `resize` and
 * `open_url`. Every message carries the nonce the parent gave at mount; nothing else exists.
 */

export type Phase = 'compile' | 'runtime';

export interface MountMessage {
  kind: 'mount';
  nonce: string;
  type: string;
  content: string;
  theme: 'light' | 'dark';
  language?: string;
}

export interface UpdateMessage {
  kind: 'update';
  nonce: string;
  content: string;
}

export interface ThemeMessage {
  kind: 'theme';
  nonce: string;
  mode: 'light' | 'dark';
}

export type ParentMessage = MountMessage | UpdateMessage | ThemeMessage;

export interface ErrorReport {
  phase: Phase;
  message: string;
  stack?: string;
  componentStack?: string;
  line?: number;
  column?: number;
}

let nonce: string | null = null;
let consoleBudget = 50;
let consoleWindow = Date.now();

function post(message: Record<string, unknown>) {
  if (nonce === null) return;
  // The opaque origin cannot name its parent's origin; the parent checks the source and nonce.
  window.parent.postMessage({ ...message, nonce }, '*');
}

export function setNonce(value: string) {
  nonce = value;
}

export function ready() {
  post({ kind: 'ready' });
}

export function reportError(report: ErrorReport) {
  post({ kind: 'error', ...report });
}

/** Console lines are capped at 16 KB each and 50 per second (13 §5). */
export function reportConsole(level: string, text: string) {
  const now = Date.now();
  if (now - consoleWindow > 1000) {
    consoleWindow = now;
    consoleBudget = 50;
  }
  if (consoleBudget <= 0) return;
  consoleBudget -= 1;
  const clipped = text.length > 16_384 ? `${text.slice(0, 16_384)}…` : text;
  post({ kind: 'console', level, text: clipped });
  if (consoleBudget === 0)
    post({ kind: 'console', level: 'warn', text: '[console output rate-limited]' });
}

export function reportResize(height: number) {
  post({ kind: 'resize', height });
}

export function openUrl(url: string) {
  post({ kind: 'open_url', url });
}

function describe(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value instanceof Error) return value.stack ?? value.message;
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}

/** Every failure and every console line becomes a bridge message (13 §6). */
export function installErrorCapture() {
  window.addEventListener('error', (e) => {
    reportError({
      phase: 'runtime',
      message: e.message || String(e.error ?? 'error'),
      stack: e.error instanceof Error ? e.error.stack : undefined,
      line: e.lineno || undefined,
      column: e.colno || undefined,
    });
  });
  window.addEventListener('unhandledrejection', (e) => {
    const reason: unknown = e.reason;
    reportError({
      phase: 'runtime',
      message: reason instanceof Error ? reason.message : describe(reason),
      stack: reason instanceof Error ? reason.stack : undefined,
    });
  });
  for (const level of ['log', 'info', 'warn', 'error', 'debug'] as const) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      original(...args);
      reportConsole(level, args.map(describe).join(' '));
    };
  }
}

/** Links never navigate the sandbox; the parent decides what to do with the URL (13 §5). */
export function installLinkInterception() {
  document.addEventListener(
    'click',
    (e) => {
      const target = (e.target as Element | null)?.closest('a[href]');
      if (!target) return;
      e.preventDefault();
      const href = target.getAttribute('href') ?? '';
      if (/^https?:/i.test(href)) openUrl(href);
    },
    true,
  );
}

/** Auto-height: the parent clamps what it gets. */
export function installResizeReporting(root: HTMLElement) {
  const report = () =>
    reportResize(Math.ceil(Math.max(root.scrollHeight, document.body.scrollHeight)));
  const observer = new ResizeObserver(report);
  observer.observe(root);
  observer.observe(document.body);
  report();
}

export function onParentMessage(handler: (m: ParentMessage) => void) {
  window.addEventListener('message', (e: MessageEvent) => {
    if (e.source !== window.parent) return;
    const data: unknown = e.data;
    if (!data || typeof data !== 'object') return;
    const m = data as ParentMessage;
    if (m.kind === 'mount') {
      setNonce(m.nonce);
    } else if (nonce === null || m.nonce !== nonce) {
      return;
    }
    handler(m);
  });
}
