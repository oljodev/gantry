import {
  AtomIcon,
  BrowserIcon,
  CodeIcon,
  FileTextIcon,
  GraphIcon,
  ImageIcon,
  SparkleIcon,
} from '@phosphor-icons/react';

import { Button } from '@/components/ui/button';
import { typeInfo } from '@/features/artifacts/registry';

/**
 * The card for an artifact a turn created or changed (13 §10, 15 §8): a fixed-width `bg-raised`
 * bubble with the type's glyph in a tile, the title, "Markdown · v2" under it, and an Open
 * button. Clicking anywhere on it opens the panel.
 */
export function ArtifactCard({
  title,
  type,
  version,
  onOpen,
}: {
  title: string;
  type: string;
  version: number;
  onOpen?: () => void;
}) {
  const label = typeInfo(type)?.label ?? type;
  return (
    <div
      role={onOpen ? 'button' : undefined}
      tabIndex={onOpen ? 0 : undefined}
      onClick={onOpen}
      onKeyDown={(e) => {
        if (onOpen && (e.key === 'Enter' || e.key === ' ')) {
          e.preventDefault();
          onOpen();
        }
      }}
      className="flex w-full max-w-lg items-center gap-3 rounded-4 border border-line bg-raised py-3 pr-3 pl-3.5 text-left transition-colors duration-(--dur-1) hover:bg-hover"
    >
      <span className="flex size-11 shrink-0 items-center justify-center rounded-3 border border-line bg-surface text-fg-2 [&_svg]:size-5">
        <ArtifactGlyph type={type} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-body font-medium text-fg">{title}</span>
        <span className="block text-meta text-fg-3 tnum">
          {label}
          {version > 0 && ` · v${version}`}
        </span>
      </span>
      {onOpen && (
        <Button
          variant="secondary"
          size="md"
          onClick={(e) => {
            e.stopPropagation();
            onOpen();
          }}
        >
          Open
        </Button>
      )}
    </div>
  );
}

/** One glyph per artifact type; a type this build does not know gets the artifact sparkle. */
export function ArtifactGlyph({ type }: { type: string }) {
  switch (type) {
    case 'markdown':
      return <FileTextIcon />;
    case 'code':
      return <CodeIcon />;
    case 'svg':
      return <ImageIcon />;
    case 'html':
      return <BrowserIcon />;
    case 'mermaid':
      return <GraphIcon />;
    case 'react':
      return <AtomIcon />;
    default:
      return <SparkleIcon />;
  }
}
