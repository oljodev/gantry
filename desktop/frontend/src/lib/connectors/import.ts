import type { ConnectorConfig } from '@/bindings';

export interface ImportedServer {
  name: string;
  config: ConnectorConfig;
}

/**
 * Reads a server configuration pasted from another client (docs/plan/03 §8). Three shapes are
 * accepted, because those are the three people have in their clipboards:
 *
 * - Claude Desktop's `{ "mcpServers": { "<name>": { … } } }`
 * - a single entry of that block, `{ "command": … }` or `{ "url": … }`
 * - the MCP registry's `server.json`, whose remotes and packages say the same thing differently
 *
 * Anything else returns null rather than guessing.
 */
export function parseServerJson(text: string): ImportedServer | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof value !== 'object' || value === null) return null;
  const root = value as Record<string, unknown>;

  // The whole block: take the first server in it.
  const servers = root.mcpServers ?? root.servers;
  if (typeof servers === 'object' && servers !== null) {
    const entries = Object.entries(servers as Record<string, unknown>);
    const first = entries[0];
    if (!first) return null;
    const config = entryToConfig(first[1]);
    return config ? { name: prettyName(first[0]), config } : null;
  }

  // A registry document: name, plus remotes or packages.
  if (typeof root.name === 'string' && (root.remotes || root.packages)) {
    const remotes = Array.isArray(root.remotes) ? root.remotes : [];
    const remote = remotes[0] as Record<string, unknown> | undefined;
    if (remote && typeof remote.url === 'string') {
      return {
        name: prettyName(root.name),
        config: { kind: 'mcp-remote', url: remote.url, headers: [], secret_headers: [] },
      };
    }
    const packages = Array.isArray(root.packages) ? root.packages : [];
    const pkg = packages[0] as Record<string, unknown> | undefined;
    if (pkg && typeof pkg.identifier === 'string') {
      const runtime = typeof pkg.registry_type === 'string' ? pkg.registry_type : 'npm';
      const command = runtime === 'pypi' ? 'uvx' : 'npx';
      const args = runtime === 'pypi' ? [pkg.identifier] : ['-y', pkg.identifier];
      return {
        name: prettyName(root.name),
        config: { kind: 'mcp-stdio', command, args, env: [], secret_env: [], cwd: null },
      };
    }
    return null;
  }

  // A bare entry.
  const config = entryToConfig(root);
  return config ? { name: prettyName(guessName(root)), config } : null;
}

function entryToConfig(value: unknown): ConnectorConfig | null {
  if (typeof value !== 'object' || value === null) return null;
  const entry = value as Record<string, unknown>;
  if (typeof entry.url === 'string') {
    return {
      kind: 'mcp-remote',
      url: entry.url,
      headers: stringPairs(entry.headers),
      secret_headers: [],
    };
  }
  if (typeof entry.command === 'string') {
    return {
      kind: 'mcp-stdio',
      command: entry.command,
      args: Array.isArray(entry.args) ? entry.args.filter((a) => typeof a === 'string') : [],
      env: stringPairs(entry.env),
      secret_env: [],
      cwd: typeof entry.cwd === 'string' ? entry.cwd : null,
    };
  }
  return null;
}

function stringPairs(value: unknown): [string, string][] {
  if (typeof value !== 'object' || value === null) return [];
  return Object.entries(value as Record<string, unknown>)
    .filter(([, v]) => typeof v === 'string')
    .map(([k, v]) => [k, v as string]);
}

function guessName(entry: Record<string, unknown>): string {
  if (typeof entry.name === 'string') return entry.name;
  if (typeof entry.url === 'string') {
    try {
      return new URL(entry.url).hostname.replace(/^www\./, '');
    } catch {
      return 'server';
    }
  }
  if (typeof entry.command === 'string') return entry.command;
  return 'server';
}

/**
 * `io.github.foo/bar-server` and `bar_server` both become "Bar server". Dots survive: the
 * namespace before the slash is already gone, and what is left is often a hostname.
 */
function prettyName(raw: string): string {
  const tail = raw.split('/').pop() ?? raw;
  const words = tail.replace(/[-_]+/g, ' ').trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}
