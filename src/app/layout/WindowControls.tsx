import { getCurrentWindow } from '@tauri-apps/api/window';
import { CopySimple, Minus, Square, X } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import { isTauri } from '@/lib/ipc/client';
import { cn } from '@/lib/utils';

/**
 * Minimise / maximise / close for Windows and Linux, where the native frame is dropped
 * (docs/plan/15 §7). macOS keeps its traffic lights and never renders this.
 */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    void win.isMaximized().then(setMaximized);
    void win
      .onResized(() => void win.isMaximized().then(setMaximized))
      .then((fn) => {
        unlisten = fn;
      });
    return () => unlisten?.();
  }, []);

  const call = (fn: (w: ReturnType<typeof getCurrentWindow>) => Promise<void>) => () => {
    if (isTauri()) void fn(getCurrentWindow());
  };

  return (
    <div className="flex h-full items-start" data-tauri-drag-region="false">
      <ControlButton label="Minimise" onClick={call((w) => w.minimize())}>
        <Minus size={14} />
      </ControlButton>
      <ControlButton
        label={maximized ? 'Restore' : 'Maximise'}
        onClick={call((w) => w.toggleMaximize())}
      >
        {maximized ? <CopySimple size={13} /> : <Square size={12} />}
      </ControlButton>
      <ControlButton label="Close" danger onClick={call((w) => w.close())}>
        <X size={14} />
      </ControlButton>
    </div>
  );
}

function ControlButton({
  label,
  danger,
  onClick,
  children,
}: {
  label: string;
  danger?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        'flex h-[var(--title-strip)] w-[46px] items-center justify-center text-fg-2 transition-colors duration-(--dur-1)',
        danger ? 'hover:bg-bad hover:text-fg-on-accent' : 'hover:bg-hover hover:text-fg',
      )}
    >
      {children}
    </button>
  );
}
