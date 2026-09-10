import { XIcon } from '@phosphor-icons/react';
import { type ReactNode, useCallback, useEffect, useRef, useState } from 'react';

import { Button } from '@/components/ui/button';
import { PANE_MIN, paneWidthFor, useUiStore } from '@/lib/stores/uiStore';
import { cn } from '@/lib/utils';

export interface PaneTab {
  id: string;
  title: string;
  icon?: ReactNode;
  /** Temporary detail tabs (diff, command, tool call) close back to the artifact (15 A17). */
  temporary?: boolean;
  /** A home tab is not closable: it is derived from the session, so an X would do nothing. */
  closable?: boolean;
  content: ReactNode;
  toolbar?: ReactNode;
}

const OVERLAY_BELOW = 1100;

/** A tab shows an X when it is temporary, or when the screen closes all of them — unless it
    says otherwise, which a home tab does. */
function closable(tab: PaneTab, all: boolean | undefined): boolean {
  if (tab.closable === false) return false;
  return tab.temporary === true || all === true;
}

/**
 * The one right pane: a level 1 card floating 8 px in from the window's edge under the title
 * strip. Opens at half the window and resizes down to 360 px, pushing the chat; below 1100 px
 * it overlays instead. Tabs for artifacts and temporary detail tabs (15 §7, A17).
 */
export function RightPane({
  tabs,
  activeId,
  onActivate,
  onClose,
  onCloseTab,
  allClosable,
}: {
  tabs: PaneTab[];
  activeId: string;
  onActivate: (id: string) => void;
  onClose: () => void;
  onCloseTab: (id: string) => void;
  /** Every tab shows a close button, not only the temporary ones (artifact tabs, 13 §4). */
  allClosable?: boolean;
}) {
  const stored = useUiStore((s) => s.paneWidth);
  const setWidth = useUiStore((s) => s.setPaneWidth);
  const [windowWidth, setWindowWidth] = useState(() => window.innerWidth);
  const width = paneWidthFor(stored, windowWidth);
  const overlay = windowWidth < OVERLAY_BELOW;
  const dragging = useRef(false);

  useEffect(() => {
    const onResize = () => setWindowWidth(window.innerWidth);
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
      style={{ width, minWidth: PANE_MIN }}
      className={cn(
        'relative flex h-full shrink-0 flex-col pr-2 pb-2 pl-1 pt-(--title-strip)',
        overlay && 'absolute inset-y-0 right-0 z-40',
      )}
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize pane"
        onPointerDown={onPointerDown}
        className="absolute inset-y-0 left-0 z-10 w-1.5 cursor-col-resize rounded-full transition-colors duration-(--dur-1) hover:bg-line-strong active:bg-line-strong"
      />
      <div
        className={cn(
          'flex min-h-0 flex-1 flex-col overflow-hidden rounded-4 border border-line-subtle bg-raised',
          overlay && 'shadow-float',
        )}
      >
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
                  closable(t, allClosable) && 'pr-7',
                )}
              >
                {t.icon}
                <span className="truncate">{t.title}</span>
              </button>
              {closable(t, allClosable) && (
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
      </div>
    </aside>
  );
}
