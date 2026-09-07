import { XIcon } from '@phosphor-icons/react';
import { type ReactNode, useCallback, useEffect, useRef, useState } from 'react';

import { Button } from '@/components/ui/button';
import { PANE_MIN, useUiStore } from '@/lib/stores/uiStore';
import { cn } from '@/lib/utils';

export interface PaneTab {
  id: string;
  title: string;
  icon?: ReactNode;
  /** Temporary detail tabs (diff, command, tool call) close back to the artifact (15 A17). */
  temporary?: boolean;
  content: ReactNode;
  toolbar?: ReactNode;
}

const OVERLAY_BELOW = 1100;

/**
 * The one right pane: resizable from 360 px to half the window, pushing the chat; below
 * 1100 px it overlays instead. Tabs for artifacts and temporary detail tabs (15 §7, A17).
 */
export function RightPane({
  tabs,
  activeId,
  onActivate,
  onClose,
  onCloseTab,
}: {
  tabs: PaneTab[];
  activeId: string;
  onActivate: (id: string) => void;
  onClose: () => void;
  onCloseTab: (id: string) => void;
}) {
  const width = useUiStore((s) => s.paneWidth);
  const setWidth = useUiStore((s) => s.setPaneWidth);
  const [overlay, setOverlay] = useState(() => window.innerWidth < OVERLAY_BELOW);
  const dragging = useRef(false);

  useEffect(() => {
    const onResize = () => setOverlay(window.innerWidth < OVERLAY_BELOW);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !e.defaultPrevented) {
        const active = tabs.find((t) => t.id === activeId);
        if (active?.temporary) onCloseTab(active.id);
        else onClose();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [tabs, activeId, onClose, onCloseTab]);

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      dragging.current = true;
      const startX = e.clientX;
      const startWidth = width;
      const target = e.currentTarget;
      target.setPointerCapture(e.pointerId);
      const move = (ev: PointerEvent) => {
        if (dragging.current)
          setWidth(startWidth - (ev.clientX - startX), Math.floor(window.innerWidth / 2));
      };
      const up = () => {
        dragging.current = false;
        target.removeEventListener('pointermove', move);
        target.removeEventListener('pointerup', up);
      };
      target.addEventListener('pointermove', move);
      target.addEventListener('pointerup', up);
    },
    [width, setWidth],
  );

  const active = tabs.find((t) => t.id === activeId) ?? tabs[0];

  return (
    <aside
      aria-label="Details"
      style={{ width: Math.min(width, Math.floor(window.innerWidth / 2)), minWidth: PANE_MIN }}
      className={cn(
        'relative flex h-full shrink-0 flex-col border-l border-line bg-surface pt-(--title-strip)',
        overlay && 'absolute inset-y-0 right-0 z-40 shadow-float',
      )}
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize pane"
        onPointerDown={onPointerDown}
        className="absolute inset-y-0 -left-0.75 z-10 w-1.5 cursor-col-resize transition-colors duration-(--dur-1) hover:bg-line-strong active:bg-line-strong"
      />
      <div
        className="flex h-(--row) shrink-0 items-end border-b border-line pl-1 pr-1"
        role="tablist"
      >
        {tabs.map((t) => (
          <div key={t.id} className="group/tab relative -mb-px flex items-end">
            <button
              type="button"
              role="tab"
              aria-selected={t.id === active?.id}
              onClick={() => onActivate(t.id)}
              className={cn(
                'flex h-(--control-lg) max-w-56 items-center gap-1.5 border-b-2 px-2 text-ui font-medium transition-colors duration-(--dur-1) [&_svg]:size-3.5 [&_svg]:shrink-0',
                t.id === active?.id
                  ? 'border-fg text-fg'
                  : 'border-transparent text-fg-2 hover:text-fg',
                t.temporary && 'pr-7',
              )}
            >
              {t.icon}
              <span className="truncate">{t.title}</span>
            </button>
            {t.temporary && (
              <button
                type="button"
                aria-label={`Close ${t.title}`}
                onClick={() => onCloseTab(t.id)}
                className="absolute right-1 bottom-2 rounded-1 p-0.5 text-fg-3 opacity-0 transition-opacity duration-(--dur-1) hover:bg-hover hover:text-fg group-hover/tab:opacity-100 focus-visible:opacity-100"
              >
                <XIcon className="size-3" />
              </button>
            )}
          </div>
        ))}
        <div className="ml-auto flex items-center pb-1">
          <Button variant="ghost" size="icon-sm" aria-label="Close pane" onClick={onClose}>
            <XIcon />
          </Button>
        </div>
      </div>
      {active?.toolbar && (
        <div className="flex h-(--row) shrink-0 items-center gap-2 border-b border-line-subtle px-3">
          {active.toolbar}
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto">{active?.content}</div>
    </aside>
  );
}
