import { CheckIcon } from '@phosphor-icons/react';

import { ConnectorMark } from '@/components/gantry/ConnectorMark';
import { Badge } from '@/components/ui/badge';
import type { ConnectorEntry } from '@/fixtures/connectors';

/** Browse tile (15 §8): mark, name, one line, Built in / MCP badge, an Installed check. */
export function ConnectorTile({
  connector,
  onClick,
}: {
  connector: ConnectorEntry;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="flex h-full flex-col gap-2 rounded-3 border border-line-subtle bg-raised p-3 text-left transition-colors duration-(--dur-1) hover:border-line-strong hover:bg-hover"
    >
      <div className="flex items-center gap-2">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-2 bg-inset">
          <ConnectorMark id={connector.id} name={connector.name} size={18} />
        </span>
        <span className="min-w-0 flex-1 truncate text-ui font-medium text-fg">
          {connector.name}
        </span>
        {connector.installed && (
          <CheckIcon className="size-3.5 shrink-0 text-good" aria-label="Installed" />
        )}
      </div>
      <div className="line-clamp-2 text-meta text-fg-2">{connector.does}</div>
      <div className="mt-auto flex items-center gap-1.5">
        <Badge variant={connector.kind === 'native' ? 'outline' : 'neutral'}>
          {connector.kind === 'native' ? 'Built in' : 'MCP'}
        </Badge>
        {connector.status === 'needs_reconnect' && <Badge variant="warn">Reconnect</Badge>}
        {connector.status === 'runtime_missing' && <Badge variant="warn">Runtime missing</Badge>}
      </div>
    </button>
  );
}
