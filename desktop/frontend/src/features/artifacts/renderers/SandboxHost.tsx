import { useEffect, useMemo, useRef, useState } from 'react';

import type { RenderError } from '@/bindings';
import {
  acceptMessage,
  type ConsoleLine,
  htmlDocument,
  loadRuntime,
  newNonce,
  SANDBOX_FLAGS,
  type Theme,
} from '@/features/artifacts/bridge';
import { record } from '@/lib/perf';
import { useUiStore } from '@/lib/stores/uiStore';

export interface SandboxReport {
  status: 'ok' | 'error';
  errors: RenderError[];
}

/**
 * The sandboxed iframe for `html`, `mermaid` and `react` (docs/plan/13 §5): `srcdoc`,
 * `sandbox="allow-scripts"`, no `allow-same-origin`, so the document has an opaque origin.
 * Every message back is checked for source, origin and nonce. Reports once per content.
 */
export function SandboxHost({
  type,
  content,
  language,
  onReport,
  onConsole,
  onOpenUrl,
  className,
  zoom = 1,
}: {
  type: 'html' | 'mermaid' | 'react';
  content: string;
  language?: string | null;
  onReport?: (report: SandboxReport) => void;
  onConsole?: (line: ConsoleLine) => void;
  onOpenUrl?: (url: string) => void;
  className?: string;
  /** Page zoom inside the document (13 §4): the frame is not scaled, the document is. */
  zoom?: number;
}) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [runtime, setRuntime] = useState<string | null>(null);
  const [height, setHeight] = useState<number | null>(null);
  // One nonce per document: a new content means a new frame, a new handshake, a new report.
  const docKey = `${type}\u0000${content}`;
  const nonce = useMemo(() => {
    void docKey;
    return newNonce();
  }, [docKey]);
  const reported = useRef(false);
  const errors = useRef<RenderError[]>([]);
  const theme = useUiStore((s) => s.theme);
  const mode: Theme =
    theme === 'system'
      ? window.matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light'
      : theme;
  const latest = useRef({
    onReport,
    onConsole,
    onOpenUrl,
    content,
    type,
    language,
    mode,
    nonce,
    zoom,
  });
  useEffect(() => {
    latest.current = {
      onReport,
      onConsole,
      onOpenUrl,
      content,
      type,
      language,
      mode,
      nonce,
      zoom,
    };
  });

  // A new document per content: the content itself for html, the runtime for react and mermaid.
  const htmlDoc = useMemo(() => (type === 'html' ? htmlDocument(content) : null), [type, content]);
  // When this document started loading, so the wait for its first frame can be named
  // (docs/dev/performance.md). A new nonce is a new mount: a new artifact, or a new version.
  const startedAt = useRef(0);
  useEffect(() => {
    reported.current = false;
    errors.current = [];
    startedAt.current = performance.now();
  }, [nonce]);
  useEffect(() => {
    if (type === 'html') return;
    let cancelled = false;
    void loadRuntime().then((doc) => {
      if (!cancelled) setRuntime(doc);
    });
    return () => {
      cancelled = true;
    };
  }, [type]);
  const srcdoc = type === 'html' ? htmlDoc : runtime;

  useEffect(() => {
    const onMessage = (e: MessageEvent) => {
      const { onReport, onConsole, onOpenUrl, content, type, language, mode, nonce, zoom } =
        latest.current;
      const m = acceptMessage(e, frame.current, nonce);
      if (!m) return;
      switch (m.kind) {
        case 'loaded':
          frame.current?.contentWindow?.postMessage(
            { kind: 'mount', nonce, type, content, theme: mode, language: language ?? undefined },
            '*',
          );
          // A remount starts the document at zoom 1; the panel's zoom is the user's, so it is
          // reapplied here rather than being quietly lost on the next version.
          if (zoom !== 1) {
            frame.current?.contentWindow?.postMessage({ kind: 'zoom', nonce, factor: zoom }, '*');
          }
          break;
        case 'ready':
          if (!reported.current) {
            reported.current = true;
            record(`artifact: ${type}`, performance.now() - startedAt.current);
            onReport?.({ status: errors.current.length ? 'error' : 'ok', errors: errors.current });
          }
          break;
        case 'error': {
          const err: RenderError = {
            phase: m.phase,
            message: m.message,
            line: m.line ?? null,
            column: m.column ?? null,
          };
          errors.current = [...errors.current, err].slice(0, 20);
          onConsole?.({ level: 'error', text: `${m.phase}: ${m.message}` });
          if (!reported.current) {
            reported.current = true;
            onReport?.({ status: 'error', errors: errors.current });
          }
          break;
        }
        case 'console':
          onConsole?.({ level: m.level, text: m.text });
          break;
        case 'resize':
          if (typeof m.height === 'number' && Number.isFinite(m.height)) {
            setHeight(Math.max(80, Math.min(20_000, Math.round(m.height))));
          }
          break;
        case 'open_url':
          if (typeof m.url === 'string' && /^https:\/\//i.test(m.url)) onOpenUrl?.(m.url);
          break;
        case 'storage':
        case 'tools':
          // Reserved namespaces (13 §5, §8): answered, never implemented here.
          frame.current?.contentWindow?.postMessage(
            { kind: `${m.kind}.result`, id: m.id, error: 'unsupported', nonce },
            '*',
          );
          break;
      }
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, []);

  // Theme follows the app without a reload.
  useEffect(() => {
    frame.current?.contentWindow?.postMessage({ kind: 'theme', nonce, mode }, '*');
  }, [mode, nonce]);

  // And so does the zoom. A document that has not answered `loaded` yet gets it at mount.
  useEffect(() => {
    frame.current?.contentWindow?.postMessage({ kind: 'zoom', nonce, factor: zoom }, '*');
  }, [zoom, nonce]);

  if (srcdoc === null) {
    return <div className={className} />;
  }
  return (
    <iframe
      // A new frame per content, so the runtime reloads and the handshake starts over.
      key={nonce}
      ref={frame}
      title="Artifact"
      sandbox={SANDBOX_FLAGS}
      referrerPolicy="no-referrer"
      srcDoc={srcdoc}
      className={className}
      style={{
        width: '100%',
        height: type === 'html' ? '100%' : (height ?? 320),
        border: 0,
        display: 'block',
        background: 'transparent',
      }}
    />
  );
}
