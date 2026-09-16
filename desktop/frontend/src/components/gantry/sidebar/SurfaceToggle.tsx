import { ChatCircleIcon, CodeIcon } from '@phosphor-icons/react';
import { useNavigate } from '@tanstack/react-router';

import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { type Surface, useUiStore } from '@/lib/stores/uiStore';
import { cn } from '@/lib/utils';
import { shortcutLabel } from '@/lib/shortcuts';

/**
 * The two-icon segmented control beside the logo (docs/plan/16 §4): Chat and Code, one window,
 * two lists of sessions.
 *
 * Switching returns to where that surface was, not to its index, because the two surfaces are
 * two places you are working rather than two views of one place — coming back to a blank New
 * chat every time would make the toggle feel like it threw something away.
 */
export function SurfaceToggle() {
  const navigate = useNavigate();
  const surface = useUiStore((s) => s.surface);
  const lastRoute = useUiStore((s) => s.lastRoute);
  const setSurface = useUiStore((s) => s.setSurface);

  const go = (next: Surface) => {
    setSurface(next);
    const back = lastRoute[next];
    void navigate({ to: back ?? (next === 'code' ? '/code' : '/chat') });
  };

  return (
    <div
      role="tablist"
      aria-label="Surface"
      className="flex items-center gap-0.5 rounded-2 bg-raised p-0.5"
    >
      <Choice
        surface="chat"
        current={surface}
        label="Chat"
        icon={<ChatCircleIcon size={15} />}
        onSelect={go}
      />
      <Choice
        surface="code"
        current={surface}
        label="Code"
        icon={<CodeIcon size={15} />}
        onSelect={go}
      />
    </div>
  );
}

function Choice({
  surface,
  current,
  label,
  icon,
  onSelect,
}: {
  surface: Surface;
  current: Surface;
  label: string;
  icon: React.ReactNode;
  onSelect: (surface: Surface) => void;
}) {
  const active = surface === current;
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            role="tab"
            aria-selected={active}
            aria-label={label}
            data-tauri-drag-region="false"
            onClick={() => onSelect(surface)}
            className={cn(
              'flex size-6 items-center justify-center rounded-1.5 transition-colors duration-(--dur-1)',
              active ? 'bg-selected text-fg' : 'text-fg-3 hover:text-fg-2',
            )}
          />
        }
      >
        {icon}
      </TooltipTrigger>
      <TooltipContent>
        {label} <span className="text-fg-3">{shortcutLabel('mod+shift+K')}</span>
      </TooltipContent>
    </Tooltip>
  );
}
