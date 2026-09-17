/**
 * The clock the window is measured by (docs/dev/performance.md).
 *
 * Three things are timed, because they are the three things a person notices: how long the app
 * takes to open, how long a window that opens over it takes to appear, and how long the app
 * takes to answer. Everything here is a number already lying around — `performance.now()` is
 * free and the browser is taking these timestamps anyway — so the measurement is always on.
 * Settings → Advanced reads it.
 *
 * Two shapes are kept, because two questions are being asked. **Totals per name** answer "is
 * this slow, and how slow usually" and are kept forever: a count, a total, the worst and the
 * last. **The slow list** answers "what just hitched" and holds only the spans over
 * [`SLOW_MS`], newest last, capped. A span that happens on every frame therefore costs one map
 * entry rather than pushing everything else out of a ring buffer.
 */

import { useEffect } from 'react';

/** A span worth remembering individually. */
export type Span = { name: string; ms: number; at: number };

/** Totals for one name, over the life of the window. */
export type Stat = { name: string; count: number; total: number; worst: number; last: number };

/** Below this, a span is counted but not listed: it is within one frame of a 120 Hz screen. */
export const SLOW_MS = 8;

const LIMIT = 100;
const stats = new Map<string, Stat>();
const slow: Span[] = [];
const listeners = new Set<() => void>();
let notifying: ReturnType<typeof setTimeout> | null = null;

/** Records one span. Safe to call on every frame. */
export function record(name: string, ms: number): void {
  const stat = stats.get(name);
  if (stat) {
    stat.count += 1;
    stat.total += ms;
    stat.last = ms;
    if (ms > stat.worst) stat.worst = ms;
  } else {
    stats.set(name, { name, count: 1, total: ms, worst: ms, last: ms });
  }
  if (ms >= SLOW_MS) {
    slow.push({ name, ms, at: performance.now() });
    if (slow.length > LIMIT) slow.splice(0, slow.length - LIMIT);
  }
  cached = null;
  notify();
}

/** Times a synchronous call and records it under `name`. */
export function timed<T>(name: string, fn: () => T): T {
  const started = performance.now();
  try {
    return fn();
  } finally {
    record(name, performance.now() - started);
  }
}

/**
 * Times a promise and records it under `name`, whether it settles or throws.
 *
 * `then(stop, stop)` rather than `finally`: a `finally` chain rejects when its promise does,
 * and nothing is watching that derived promise, so it would report an unhandled rejection for
 * every error the caller is handling perfectly well.
 */
export function timedAsync<T>(name: string, promise: Promise<T>): Promise<T> {
  const started = performance.now();
  const stop = () => record(name, performance.now() - started);
  void promise.then(stop, stop);
  return promise;
}

// ---------------------------------------------------------------------------- boot

/**
 * How the window came up, in milliseconds from `timeOrigin` — which is when the webview was
 * created, not when the process started. `startup_timing`'s `since_start_ms` is what joins the
 * two clocks; the panel does that sum, not this file.
 */
export const boot: Record<string, number> = {};

/** Records a boot mark once; the first value wins, so a remount cannot overwrite it. */
export function bootMark(name: string, ms = performance.now()): void {
  if (!(name in boot)) {
    boot[name] = ms;
    cached = null;
    notify();
  }
}

/**
 * The first paint, as the engine reports it. WebKit and WebView2 both publish
 * `first-contentful-paint`; where it is missing the double frame in `main.tsx` stands in, which
 * is a little later but never earlier.
 */
export function watchPaint(): void {
  try {
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        if (entry.name === 'first-contentful-paint') bootMark('paint', entry.startTime);
      }
    }).observe({ type: 'paint', buffered: true });
  } catch {
    /* no paint timing here: main.tsx's frame callback is the fallback */
  }
}

// ---------------------------------------------------------------------------- input

let lastInput = 0;

/**
 * When the user last pressed something. A window's open time is measured from here rather than
 * from the React state change, because the wait a person feels starts at their own finger: the
 * event handler, the query, the render and the paint are all inside it.
 */
export function watchInput(): void {
  const stamp = () => {
    lastInput = performance.now();
  };
  for (const type of ['pointerdown', 'keydown'] as const) {
    window.addEventListener(type, stamp, { capture: true, passive: true });
  }
}

/** Milliseconds since the last key or click, or 0 if there has not been one. */
export function sinceInput(): number {
  return lastInput === 0 ? 0 : performance.now() - lastInput;
}

/**
 * How long this window took to appear, from the key or click that asked for it.
 *
 * Measured from the user's own finger to the second frame after the content mounted, which is
 * the first frame they could have seen it in. The name is read from the dialog's title at that
 * moment rather than passed in as a prop, so every dialog in the app is measured by the one
 * call in `DialogContent` and none of them carries a label for the benefit of a stopwatch.
 *
 * A dialog that nobody asked for — one opened by a finished turn, or by a notification — is not
 * counted: measuring it from whatever the user last touched would report a minute-long "open".
 */
export function useOpenTiming(kind: string): void {
  useEffect(() => {
    let cancelled = false;
    const frame = requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (cancelled) return;
        const ms = sinceInput();
        if (ms === 0 || ms > UNPROMPTED_MS) return;
        record(`${kind}: ${openLabel()}`, ms);
      }),
    );
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
    };
  }, [kind]);
}

/** Longer than this after the last key or click, nothing on screen is a reply to it. */
const UNPROMPTED_MS = 2000;

/** The newest dialog's title, for the row in the panel. */
function openLabel(): string {
  const dialogs = document.querySelectorAll('[data-slot="dialog-content"]');
  const newest = dialogs[dialogs.length - 1];
  const title = newest?.querySelector('[data-slot="dialog-title"]')?.textContent?.trim();
  return title ? title.slice(0, 40) : 'untitled';
}

// ---------------------------------------------------------------------------- reading

export type Snapshot = { boot: Record<string, number>; stats: Stat[]; slow: Span[] };

let cached: Snapshot | null = null;

/**
 * Everything measured so far, as one value whose identity changes only when something has
 * been recorded — which is what lets a React view subscribe to it directly. Built on demand,
 * so a window with the panel closed pays nothing for it beyond dropping the cache.
 */
export function snapshot(): Snapshot {
  cached ??= {
    boot: { ...boot },
    stats: [...stats.values()].map((s) => ({ ...s })).sort((a, b) => b.worst - a.worst),
    slow: [...slow].reverse(),
  };
  return cached;
}

/** For the panel. Coalesced: a streaming chat records spans far faster than anyone can read. */
export function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function notify(): void {
  if (listeners.size === 0 || notifying !== null) return;
  notifying = setTimeout(() => {
    notifying = null;
    for (const listener of listeners) listener();
  }, 250);
}

/** Forgets everything measured so far: the panel's own button, and the tests. */
export function reset(): void {
  stats.clear();
  slow.length = 0;
  for (const key of Object.keys(boot)) delete boot[key];
  lastInput = 0;
  cached = null;
  for (const listener of listeners) listener();
}
