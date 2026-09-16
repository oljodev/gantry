import {
  ArrowCounterClockwiseIcon,
  ArrowSquareOutIcon,
  CaretLeftIcon,
  CaretRightIcon,
  ChatCircleIcon,
  CheckIcon,
  CodeIcon,
  CopyIcon,
  DotsThreeIcon,
  DownloadSimpleIcon,
  EyeIcon,
  MagnifyingGlassMinusIcon,
  MagnifyingGlassPlusIcon,
  PencilSimpleIcon,
  PlayIcon,
  StopIcon,
  WarningCircleIcon,
  WrenchIcon,
} from '@phosphor-icons/react';
import { useNavigate } from '@tanstack/react-router';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import type { ArtifactContent, RenderReport } from '@/bindings';
import { ArtifactGlyph } from '@/components/gantry/chat/ArtifactCard';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Segmented } from '@/components/ui/radio-group';
import { toast } from '@/components/ui/toast';
import type { ConsoleLine } from '@/features/artifacts/bridge';
import { highlightLanguage, isSandboxed, typeInfo } from '@/features/artifacts/registry';
import { CodeRenderer } from '@/features/artifacts/renderers/CodeRenderer';
import { MarkdownRenderer } from '@/features/artifacts/renderers/MarkdownRenderer';
import { SandboxHost, type SandboxReport } from '@/features/artifacts/renderers/SandboxHost';
import { SvgRenderer } from '@/features/artifacts/renderers/SvgRenderer';
import {
  emptyView,
  type StreamingArtifact,
  streamingFor,
  useArtifactStore,
} from '@/features/artifacts/store';
import {
  canZoomIn,
  canZoomOut,
  DEFAULT_ZOOM,
  stepZoom,
  zoomLabel,
} from '@/features/artifacts/zoom';
import { copyText } from '@/lib/clipboard';
import { reportRender, useArtifact, useArtifactMutations } from '@/lib/ipc/hooks/artifacts';
import { shortcutLabel } from '@/lib/shortcuts';
import { cn } from '@/lib/utils';

export interface ArtifactPanelProps {
  artifactId: string;
  /** Sends a visible user message quoting a render error (13 §2, Fix this). */
  onFixThis?: (text: string) => void;
  /** An intercepted link click inside a sandboxed artifact (13 §5). */
  onOpenUrl?: (url: string) => void;
  /** Full-window mode (the separate window): no toolbar chrome beyond the essentials. */
  bare?: boolean;
}

/**
 * The artifact panel (docs/plan/13 §4): one toolbar row (Rendered | Source as glyphs, the
 * version stepper when there is more than one, the zoom readout once it is not 100 %, Restore
 * and Fix this when they apply, Copy, and a menu with zoom, Download, Open in window and Edit
 * source), the renderer for the type, and the Problems strip. Reports each version's render
 * once so the tool result can complete.
 */
export function ArtifactPanel({ artifactId, onFixThis, onOpenUrl, bare }: ArtifactPanelProps) {
  const view = useArtifactStore((s) => s.views[artifactId]) ?? emptyView();
  const patchView = useArtifactStore((s) => s.patchView);
  const addProblem = useArtifactStore((s) => s.addProblem);
  const clearProblems = useArtifactStore((s) => s.clearProblems);
  const markReported = useArtifactStore((s) => s.markReported);
  const streaming = useArtifactStore((s) => streamingFor(s.streaming, s.createdBy, artifactId));
  const current = useArtifact(artifactId);
  const shownVersion = view.version;
  const older = useArtifact(shownVersion === undefined ? null : artifactId, shownVersion);
  const { save, restore, exportFile, openWindow, continueInNewChat } = useArtifactMutations();
  const navigate = useNavigate();
  const [copied, setCopied] = useState(false);

  const data: ArtifactContent | undefined =
    shownVersion === undefined ? current.data : (older.data ?? current.data);
  const artifact = data?.artifact ?? current.data?.artifact;
  const latest = artifact?.current_version ?? 1;
  const version = shownVersion ?? latest;
  const isLatest = shownVersion === undefined || shownVersion === latest;

  // While an update streams in for this artifact, the panel shows the arriving content.
  const live = streaming && streaming.content.length > 0 ? streaming : undefined;
  const type = artifact?.type ?? live?.type ?? 'markdown';
  const language = artifact?.language ?? live?.language ?? null;
  const content = live?.content ?? data?.content ?? '';
  const title = artifact?.title ?? live?.title ?? 'Artifact';
  const info = typeInfo(type);
  const sandboxed = isSandboxed(type);
  const mode = view.mode;
  const editing = view.editing && !live;

  // Report the render once per stored version (parent-rendered types are ok on first paint).
  const reportKey = `${version}`;
  const alreadyReported = view.reported[reportKey] !== undefined;
  useEffect(() => {
    if (live || !data || data.version !== version || sandboxed || alreadyReported) return;
    const report: RenderReport = { status: 'ok', errors: [] };
    markReported(artifactId, version, report);
    reportRender(artifactId, version, report);
  }, [live, data, version, sandboxed, alreadyReported, artifactId, markReported]);

  const onSandboxReport = (r: SandboxReport) => {
    if (live || !data || data.version !== version) return;
    if (useArtifactStore.getState().views[artifactId]?.reported[reportKey]) return;
    const report: RenderReport = { status: r.status, errors: r.errors };
    markReported(artifactId, version, report);
    reportRender(artifactId, version, report);
  };
  const onConsole = (line: ConsoleLine) => addProblem(artifactId, line);

  const errors = useMemo(() => view.problems.filter((p) => p.level === 'error'), [view.problems]);

  const copy = async () => {
    try {
      await copyText(content);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (err) {
      toast.add({ title: 'Could not copy', description: String(err), type: 'error' });
    }
  };
  const download = () =>
    exportFile.mutate(
      { artifactId, version: isLatest ? undefined : version },
      {
        onSuccess: (path) => {
          if (path) toast.add({ title: 'Saved', description: path, type: 'success' });
        },
        onError: (err) =>
          toast.add({ title: 'Could not save', description: describe(err), type: 'error' }),
      },
    );
  const startEdit = () => patchView(artifactId, { editing: true, draft: content, mode: 'source' });
  const cancelEdit = () => patchView(artifactId, { editing: false, draft: '' });
  const commitEdit = () => {
    if (view.draft === content) {
      cancelEdit();
      return;
    }
    save.mutate(
      { artifactId, content: view.draft },
      {
        onSuccess: () => {
          patchView(artifactId, {
            editing: false,
            draft: '',
            version: undefined,
            mode: 'rendered',
            stopped: false,
          });
          clearProblems(artifactId);
        },
        onError: (err) =>
          toast.add({ title: 'Could not save', description: describe(err), type: 'error' }),
      },
    );
  };
  const restoreThis = () =>
    restore.mutate(
      { artifactId, version },
      {
        onSuccess: () => {
          patchView(artifactId, { version: undefined, stopped: false });
          clearProblems(artifactId);
          toast.add({ title: `Restored v${version} as v${latest + 1}`, type: 'success' });
        },
        onError: (err) =>
          toast.add({ title: 'Could not restore', description: describe(err), type: 'error' }),
      },
    );
  const fixThis = () => {
    const first = errors[errors.length - 1];
    if (!first || !onFixThis) return;
    onFixThis(
      `The artifact "${title}" (${artifactId}, v${version}) failed to render:\n\n${first.text}\n\nPlease fix it.`,
    );
  };
  const step = (delta: number) => {
    const next = Math.min(latest, Math.max(1, version + delta));
    patchView(artifactId, {
      version: next === latest ? undefined : next,
      editing: false,
      stopped: false,
    });
    clearProblems(artifactId);
  };
  /**
   * 13 §5, hang risk 2: unmount the frame and leave it unmounted. Whatever the artifact was
   * doing — a timer, an animation, audio, a fetch it will never get — stops with the document.
   *
   * A version that has not reported yet is reported as stopped rather than left hanging: the
   * model asked for a render and is waiting for the answer, and "it never finished" is an
   * answer it can do something with.
   */
  const stop = () => {
    patchView(artifactId, { stopped: true });
    if (!sandboxed || live || !data || data.version !== version) return;
    if (useArtifactStore.getState().views[artifactId]?.reported[reportKey]) return;
    const report: RenderReport = {
      status: 'error',
      errors: [
        {
          phase: 'runtime',
          message: 'Stopped before it finished rendering.',
          line: null,
          column: null,
        },
      ],
    };
    markReported(artifactId, version, report);
    reportRender(artifactId, version, report);
  };
  const run = () => patchView(artifactId, { stopped: false });

  /**
   * Page zoom for this box (13 §4). `framed` is the one state where the content is a live
   * sandbox document rather than something the app drew: there the factor is sent in and the
   * artifact zooms itself, because CSS zoom does not cross into another browsing context.
   */
  const zoom = view.zoom ?? DEFAULT_ZOOM;
  const framed = sandboxed && mode === 'rendered' && !editing && !live && !view.stopped;
  const setZoom = useCallback(
    (next: number) => patchView(artifactId, { zoom: next }),
    [artifactId, patchView],
  );
  const zoomBy = useCallback(
    (delta: number) =>
      setZoom(stepZoom(useArtifactStore.getState().views[artifactId]?.zoom ?? DEFAULT_ZOOM, delta)),
    [artifactId, setZoom],
  );

  // Only one panel is ever mounted in a window — the right pane renders the active tab and
  // nothing else, and the artifact window renders one — so the keys can be taken at the window
  // rather than depending on where the focus happens to be. Not while a field has it, though;
  // `mod+-` in the composer is a hyphen.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = document.documentElement.dataset.os === 'macos' ? e.metaKey : e.ctrlKey;
      if (!mod || e.altKey || isTyping(e.target)) return;
      // Shift is not rejected: on a US layout `+` *is* Shift and `=`, which is why every
      // browser takes both spellings for zooming in. Same for `_` and `-`.
      if (e.key === '=' || e.key === '+') zoomBy(1);
      else if (e.key === '-' || e.key === '_') zoomBy(-1);
      else if (e.key === '0' && !e.shiftKey) setZoom(DEFAULT_ZOOM);
      else return;
      e.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [zoomBy, setZoom]);

  // The wheel has to be a listener of its own: React's is passive, and a passive handler
  // cannot stop the webview zooming the whole window instead — the thing this is here to
  // avoid. Over a sandboxed artifact the wheel belongs to that document and never reaches
  // here, which is correct: an artifact may want the wheel, and its input is its own.
  const scroller = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const node = scroller.current;
    if (!node) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();
      if (e.deltaY !== 0) zoomBy(e.deltaY < 0 ? 1 : -1);
    };
    node.addEventListener('wheel', onWheel, { passive: false });
    return () => node.removeEventListener('wheel', onWheel);
  }, [zoomBy]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-(--row) shrink-0 items-center gap-2 border-b border-line-subtle px-2">
        <Segmented
          value={mode}
          onValueChange={(v) => patchView(artifactId, { mode: v })}
          options={[
            ['rendered', <EyeIcon key="r" />, 'Rendered'],
            ['source', <CodeIcon key="s" />, 'Source'],
          ]}
          aria-label="View"
        />
        {bare && (
          <span className="flex min-w-0 items-center gap-1.5 text-ui font-medium [&_svg]:size-4 [&_svg]:shrink-0 [&_svg]:text-fg-2">
            <ArtifactGlyph type={type} />
            <span className="truncate">{title}</span>
          </span>
        )}
        {latest > 1 && (
          <div className="flex items-center gap-0.5 text-meta text-fg-2 tnum">
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Previous version"
              disabled={version <= 1}
              onClick={() => step(-1)}
            >
              <CaretLeftIcon />
            </Button>
            <span className="whitespace-nowrap">
              v{version} of {latest}
            </span>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="Next version"
              disabled={version >= latest}
              onClick={() => step(1)}
            >
              <CaretRightIcon />
            </Button>
          </div>
        )}
        <div className="ml-auto flex items-center gap-1">
          {zoom !== DEFAULT_ZOOM && (
            // Nothing at 100 %: a control for a setting nobody changed is a control in the way.
            // Once it has been changed it is state, and state the panel is holding gets shown.
            <div className="flex items-center gap-0.5 text-meta text-fg-2 tnum">
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Zoom out"
                disabled={!canZoomOut(zoom)}
                onClick={() => zoomBy(-1)}
              >
                <MagnifyingGlassMinusIcon />
              </Button>
              <button
                type="button"
                onClick={() => setZoom(DEFAULT_ZOOM)}
                aria-label={`Zoom ${zoomLabel(zoom)}, reset to 100%`}
                title={`Reset zoom (${shortcutLabel('mod+0')})`}
                className="min-w-10 whitespace-nowrap rounded-2 px-1 py-0.5 hover:text-fg"
              >
                {zoomLabel(zoom)}
              </button>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label="Zoom in"
                disabled={!canZoomIn(zoom)}
                onClick={() => zoomBy(1)}
              >
                <MagnifyingGlassPlusIcon />
              </Button>
            </div>
          )}
          {!isLatest && (
            <Button
              variant="secondary"
              size="sm"
              onClick={restoreThis}
              disabled={restore.isPending}
            >
              <ArrowCounterClockwiseIcon />
              Restore this version
            </Button>
          )}
          {sandboxed && !editing && !live && mode === 'rendered' && (
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={view.stopped ? 'Run' : 'Stop'}
              title={view.stopped ? 'Run this artifact again' : 'Stop this artifact'}
              onClick={view.stopped ? run : stop}
            >
              {view.stopped ? <PlayIcon /> : <StopIcon />}
            </Button>
          )}
          {errors.length > 0 && onFixThis && (
            <Button variant="secondary" size="sm" onClick={fixThis}>
              <WrenchIcon />
              Fix this
            </Button>
          )}
          <Button variant="secondary" size="sm" onClick={() => void copy()} className="w-18">
            {copied ? <CheckIcon className="text-good" /> : <CopyIcon />}
            {copied ? 'Copied' : 'Copy'}
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger
              render={<Button variant="ghost" size="icon-sm" aria-label="More actions" />}
            >
              <DotsThreeIcon weight="bold" />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => zoomBy(1)} disabled={!canZoomIn(zoom)}>
                <MagnifyingGlassPlusIcon />
                Zoom in
                <DropdownMenuShortcut>{shortcutLabel('mod++')}</DropdownMenuShortcut>
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => zoomBy(-1)} disabled={!canZoomOut(zoom)}>
                <MagnifyingGlassMinusIcon />
                Zoom out
                <DropdownMenuShortcut>{shortcutLabel('mod+-')}</DropdownMenuShortcut>
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={() => setZoom(DEFAULT_ZOOM)}
                disabled={zoom === DEFAULT_ZOOM}
              >
                Actual size
                <DropdownMenuShortcut>{shortcutLabel('mod+0')}</DropdownMenuShortcut>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={download}>
                <DownloadSimpleIcon />
                Download…
              </DropdownMenuItem>
              {!bare && (
                <DropdownMenuItem onClick={() => openWindow.mutate(artifactId)}>
                  <ArrowSquareOutIcon />
                  Open in window
                </DropdownMenuItem>
              )}
              {isLatest && !editing && !live && (
                <DropdownMenuItem onClick={startEdit}>
                  <PencilSimpleIcon />
                  Edit source
                </DropdownMenuItem>
              )}
              {!bare && !live && (
                // 13 §9: a fresh chat in the same project, told which artifact it is about and
                // to read it first. The transcript that explains this one stays where it is.
                <DropdownMenuItem
                  onClick={() =>
                    continueInNewChat.mutate(artifactId, {
                      onSuccess: (chat) =>
                        void navigate({ to: '/chat/$chatId', params: { chatId: chat.id } }),
                      onError: (err) =>
                        toast.add({
                          title: 'Could not start the chat',
                          description: describe(err),
                          type: 'error',
                        }),
                    })
                  }
                >
                  <ChatCircleIcon />
                  Continue in new chat
                </DropdownMenuItem>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
      <div ref={scroller} className="relative min-h-0 flex-1 overflow-auto">
        {live && (
          <div className="sticky top-0 z-10 flex h-6 items-center gap-2 border-b border-line-subtle bg-raised/90 px-3 text-meta text-fg-3 backdrop-blur">
            <span className="size-1.5 animate-pulse rounded-full bg-accent" />
            Writing…
          </div>
        )}
        {/*
         * The zoomed box, for everything the app draws itself. `h-full` keeps meaning the
         * visible pane at any factor: a percentage height resolves against the containing
         * block in the *zoomed* space, so `height: 100%` under `zoom: 2` paints 300 px of a
         * 300 px pane, not 600 — measured in WebKitGTK, and what the standardised `zoom`
         * specifies. That is what lets the renderers keep their `min-h-full` unchanged.
         */}
        <div className="h-full" style={framed || zoom === DEFAULT_ZOOM ? undefined : { zoom }}>
          {editing ? (
            <Editor
              value={view.draft}
              onChange={(draft) => patchView(artifactId, { draft })}
              onCancel={cancelEdit}
              onSave={commitEdit}
              saving={save.isPending}
            />
          ) : mode === 'source' || (live && !info?.streamsRender) ? (
            <CodeRenderer
              content={content}
              language={highlightLanguage(type, language)}
              streaming={!!live}
            />
          ) : (
            <Rendered
              key={`${artifactId}:${version}:${live ? 'live' : 'stored'}`}
              type={type}
              content={content}
              language={language}
              title={title}
              streaming={live}
              stopped={view.stopped}
              onRun={run}
              onReport={onSandboxReport}
              onConsole={onConsole}
              onOpenUrl={onOpenUrl}
              zoom={zoom}
            />
          )}
        </div>
      </div>
      {view.problems.length > 0 && !editing && (
        <Problems problems={view.problems} onClear={() => clearProblems(artifactId)} />
      )}
    </div>
  );
}

function Rendered({
  type,
  content,
  language,
  title,
  streaming,
  stopped,
  onRun,
  onReport,
  onConsole,
  onOpenUrl,
  zoom,
}: {
  type: string;
  content: string;
  language: string | null;
  title: string;
  streaming: StreamingArtifact | undefined;
  stopped: boolean;
  onRun: () => void;
  onReport: (r: SandboxReport) => void;
  onConsole: (line: ConsoleLine) => void;
  onOpenUrl?: (url: string) => void;
  /** Only the sandboxed types read this: the rest are inside a zoomed box already. */
  zoom: number;
}) {
  switch (type) {
    case 'markdown':
      return <MarkdownRenderer content={content} />;
    case 'code':
      return (
        <CodeRenderer
          content={content}
          language={highlightLanguage(type, language)}
          streaming={!!streaming}
        />
      );
    case 'svg':
      return <SvgRenderer content={content} title={title} />;
    case 'html':
    case 'mermaid':
    case 'react':
      if (streaming) {
        return (
          <CodeRenderer content={content} language={highlightLanguage(type, language)} streaming />
        );
      }
      // Stopped: no frame at all, which is the whole point — an unmounted document runs nothing.
      if (stopped) return <Stopped onRun={onRun} />;
      return (
        <SandboxHost
          type={type}
          content={content}
          language={language}
          onReport={onReport}
          onConsole={onConsole}
          onOpenUrl={onOpenUrl}
          zoom={zoom}
          className="min-h-full"
        />
      );
    default:
      return (
        <div className="p-4 text-body text-fg-2">This build cannot render “{type}” artifacts.</div>
      );
  }
}

/** What the panel shows in place of an artifact that has been stopped (13 §5, hang risk 2). */
function Stopped({ onRun }: { onRun: () => void }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
      <p className="text-body text-fg-2">
        Stopped. Nothing in this artifact is running.
        <br />
        Running it again starts it from the beginning.
      </p>
      <Button variant="secondary" size="sm" onClick={onRun}>
        <PlayIcon />
        Run
      </Button>
    </div>
  );
}

function Editor({
  value,
  onChange,
  onCancel,
  onSave,
  saving,
}: {
  value: string;
  onChange: (v: string) => void;
  onCancel: () => void;
  onSave: () => void;
  saving: boolean;
}) {
  return (
    <div className="flex h-full min-h-0 flex-col">
      <textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Escape') onCancel();
          if (e.key === 's' && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            onSave();
          }
        }}
        spellCheck={false}
        aria-label="Artifact source"
        className="selectable min-h-0 flex-1 resize-none bg-transparent p-3 font-mono text-code text-fg outline-none"
      />
      <div className="flex h-(--row) shrink-0 items-center justify-end gap-1 border-t border-line-subtle px-2">
        <span className="mr-auto text-meta text-fg-3">Saving makes a new version</span>
        <Button variant="ghost" size="sm" onClick={onCancel}>
          Cancel
        </Button>
        <Button variant="primary" size="sm" onClick={onSave} disabled={saving}>
          {saving ? 'Saving…' : 'Save'}
        </Button>
      </div>
    </div>
  );
}

function Problems({
  problems,
  onClear,
}: {
  problems: { at: number; level: string; text: string }[];
  onClear: () => void;
}) {
  const errors = problems.filter((p) => p.level === 'error').length;
  return (
    <div className="max-h-40 shrink-0 overflow-auto border-t border-line bg-inset">
      <div className="sticky top-0 flex h-7 items-center gap-2 bg-inset px-3 text-micro font-medium uppercase tracking-[0.04em] text-fg-3">
        <WarningCircleIcon className={cn('size-3.5', errors > 0 ? 'text-bad' : 'text-fg-3')} />
        Problems · {problems.length}
        <button
          type="button"
          onClick={onClear}
          className="ml-auto normal-case tracking-normal hover:text-fg"
        >
          Clear
        </button>
      </div>
      <ul className="selectable px-3 pb-2 font-mono text-mono">
        {problems.map((p, i) => (
          <li
            key={`${p.at}-${i}`}
            className={cn('whitespace-pre-wrap', p.level === 'error' ? 'text-bad' : 'text-fg-2')}
          >
            {p.text}
          </li>
        ))}
      </ul>
    </div>
  );
}

/** A field has the keys while it has the focus; `mod+-` there is a hyphen, not a zoom. */
function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.tagName !== 'string') return false;
  return (
    el.tagName === 'INPUT' ||
    el.tagName === 'TEXTAREA' ||
    el.tagName === 'SELECT' ||
    el.isContentEditable
  );
}

function describe(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err)
    return String((err as { message: unknown }).message);
  return String(err);
}
