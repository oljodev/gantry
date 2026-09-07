import {
  CodeIcon,
  FolderIcon,
  GithubLogoIcon,
  GlobeIcon,
  type Icon,
  PlugIcon,
  TerminalIcon,
} from '@phosphor-icons/react';

import { cn } from '@/lib/utils';

/**
 * A connector's mark in the app: first-party glyphs from Phosphor; third parties show a
 * two-letter monogram until their logo is cleared (website/public/connectors/ is the site's
 * override folder; the app gets its own in M9).
 */
const GLYPHS: Record<string, Icon> = {
  filesystem: FolderIcon,
  'code-editor': CodeIcon,
  shell: TerminalIcon,
  web: GlobeIcon,
  github: GithubLogoIcon,
  mcp: PlugIcon,
};

export function ConnectorMark({
  id,
  name,
  size = 16,
  className,
}: {
  id: string;
  name?: string;
  size?: number;
  className?: string;
}) {
  const Glyph = GLYPHS[id];
  if (Glyph)
    return <Glyph size={size} className={cn('shrink-0 text-fg-2', className)} aria-hidden />;
  const initials = (name ?? id)
    .split(/[\s-]+/)
    .map((w) => w[0]?.toUpperCase() ?? '')
    .join('')
    .slice(0, 2);
  return (
    <span
      aria-hidden
      style={{ width: size, height: size, fontSize: Math.round(size * 0.45) }}
      className={cn(
        'inline-flex shrink-0 items-center justify-center rounded-1 bg-hover font-medium leading-none text-fg-2',
        className,
      )}
    >
      {initials}
    </span>
  );
}
