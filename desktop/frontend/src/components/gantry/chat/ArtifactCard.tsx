import {
  AtomIcon,
  BrowserIcon,
  CodeIcon,
  FileTextIcon,
  GraphIcon,
  ImageIcon,
  SparkleIcon,
} from '@phosphor-icons/react';

import { typeInfo } from '@/features/artifacts/registry';

/**
 * The card for an artifact a turn created or changed (13 §10, 15 §8): the type's glyph in a
 * tile, the title, "Markdown · v2" under it. The whole card opens the panel.
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
    <button
      type="button"
      onClick={onOpen}
      disabled={!onOpen}
      className="group/card flex w-full max-w-md items-center gap-3 rounded-3 border border-line bg-surface p-3 text-left transition-colors duration-(--dur-1) enabled:hover:bg-hover"
    >
      <span className="flex size-10 shrink-0 items-center justify-center rounded-2 bg-raised text-fg-2 [&_svg]:size-5">
        <ArtifactGlyph type={type} />
      </span>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-ui font-medium text-fg">{title}</span>
        <span className="block text-meta text-fg-3 tnum">
          {label}
          {version > 0 && ` · v${version}`}
        </span>
      </span>
      {onOpen && (
        <span className="pr-1 text-meta text-fg-3 transition-colors duration-(--dur-1) group-hover/card:text-fg">
          Open
        </span>
      )}
    </button>
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
