import { useState } from 'react';

import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Segmented } from '@/components/ui/radio-group';
import { Textarea } from '@/components/ui/textarea';
import type { ConnectorConfig } from '@/bindings';
import { useConnectorMutations } from '@/lib/ipc/hooks/connectors';
import { parseServerJson } from '@/lib/connectors/import';

type Tab = 'remote' | 'local' | 'paste';

/**
 * A server of your own (docs/plan/03 §8): a URL, a command, or a configuration pasted from
 * another client. It becomes an ordinary instance with the same permission treatment as
 * everything else — nothing about being hand-added makes it more trusted.
 */
export function AddCustomServer({ onClose }: { onClose: () => void }) {
  const { installCustom } = useConnectorMutations();
  const [tab, setTab] = useState<Tab>('remote');
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [command, setCommand] = useState('');
  const [args, setArgs] = useState('');
  const [json, setJson] = useState('');
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setError(null);
    try {
      const server = build();
      await installCustom.mutateAsync(server);
      onClose();
    } catch (err) {
      setError(String(err));
    }
  };

  /** The form or the pasted JSON as one { name, config }. */
  const build = (): { name: string; config: ConnectorConfig } => {
    if (tab === 'paste') {
      const parsed = parseServerJson(json);
      if (!parsed) throw new Error('This is not a server configuration Gantry recognises.');
      return { name: name.trim() || parsed.name, config: parsed.config };
    }
    if (!name.trim()) throw new Error('Give the server a name.');
    if (tab === 'remote') {
      if (!/^https?:\/\//i.test(url.trim())) throw new Error('The URL must start with https://');
      return {
        name: name.trim(),
        config: { kind: 'mcp-remote', url: url.trim(), headers: [], secret_headers: [] },
      };
    }
    if (!command.trim()) throw new Error('Give the command to run.');
    return {
      name: name.trim(),
      config: {
        kind: 'mcp-stdio',
        command: command.trim(),
        args: args.trim() === '' ? [] : args.trim().split(/\s+/),
        env: [],
        secret_env: [],
        cwd: null,
      },
    };
  };

  return (
    <Dialog open onOpenChange={(next) => !next && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Add a server</DialogTitle>
          <DialogDescription>
            Any MCP server: one you host, one you run locally, or a configuration from another
            client.
          </DialogDescription>
        </DialogHeader>

        <Segmented<Tab>
          aria-label="Kind of server"
          value={tab}
          onValueChange={setTab}
          options={[
            ['remote', 'URL'],
            ['local', 'Command'],
            ['paste', 'Paste JSON'],
          ]}
        />

        {tab !== 'paste' && (
          <Input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Name"
            aria-label="Name"
          />
        )}
        {tab === 'remote' && (
          <Input
            value={url}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://example.com/mcp"
            aria-label="Server URL"
          />
        )}
        {tab === 'local' && (
          <>
            <Input
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              placeholder="npx"
              aria-label="Command"
            />
            <Input
              value={args}
              onChange={(e) => setArgs(e.target.value)}
              placeholder="-y @modelcontextprotocol/server-everything"
              aria-label="Arguments"
            />
            <p className="text-meta text-fg-2">
              This runs on your machine with your permissions, like any program you start yourself.
              Gantry shows you every tool call it makes, but it cannot police what the process does
              on its own.
            </p>
          </>
        )}
        {tab === 'paste' && (
          <>
            <Textarea
              value={json}
              onChange={(e) => setJson(e.target.value)}
              placeholder={
                '{ "mcpServers": { "everything": { "command": "npx", "args": [...] } } }'
              }
              aria-label="Server configuration"
              className="h-40 font-mono text-mono"
            />
            <p className="text-meta text-fg-2">
              Claude Desktop's <code className="font-mono">mcpServers</code> block, or a registry{' '}
              <code className="font-mono">server.json</code>.
            </p>
          </>
        )}

        {error && (
          <p role="alert" className="text-meta text-bad">
            {error}
          </p>
        )}

        <DialogFooter>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button disabled={installCustom.isPending} onClick={() => void submit()}>
            {installCustom.isPending ? 'Connecting…' : 'Add'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
