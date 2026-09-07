import { describe, expect, it } from 'vitest';

import { parseServerJson } from '@/lib/connectors/import';

describe('parseServerJson', () => {
  it("reads Claude Desktop's mcpServers block", () => {
    const parsed = parseServerJson(
      JSON.stringify({
        mcpServers: {
          everything: {
            command: 'npx',
            args: ['-y', '@modelcontextprotocol/server-everything'],
            env: { API_KEY: 'x' },
          },
        },
      }),
    );
    expect(parsed?.name).toBe('Everything');
    expect(parsed?.config).toEqual({
      kind: 'mcp-stdio',
      command: 'npx',
      args: ['-y', '@modelcontextprotocol/server-everything'],
      env: [['API_KEY', 'x']],
      secret_env: [],
      cwd: null,
    });
  });

  it('reads a bare remote entry and names it after its host', () => {
    const parsed = parseServerJson('{"url":"https://mcp.example.com/mcp"}');
    expect(parsed?.name).toBe('Mcp.example.com');
    expect(parsed?.config).toEqual({
      kind: 'mcp-remote',
      url: 'https://mcp.example.com/mcp',
      headers: [],
      secret_headers: [],
    });
  });

  it('reads a registry server.json, remote first', () => {
    const parsed = parseServerJson(
      JSON.stringify({
        name: 'io.github.acme/weather-server',
        remotes: [{ type: 'streamable-http', url: 'https://weather.example/mcp' }],
        packages: [{ registry_type: 'npm', identifier: '@acme/weather' }],
      }),
    );
    expect(parsed?.name).toBe('Weather server');
    expect(parsed?.config.kind).toBe('mcp-remote');
  });

  it('falls back to a package when a registry entry has no remote', () => {
    const parsed = parseServerJson(
      JSON.stringify({
        name: 'weather',
        packages: [{ registry_type: 'pypi', identifier: 'weather-mcp' }],
      }),
    );
    expect(parsed?.config).toMatchObject({ command: 'uvx', args: ['weather-mcp'] });
  });

  it('refuses what it does not understand instead of guessing', () => {
    expect(parseServerJson('not json')).toBeNull();
    expect(parseServerJson('{"hello":"world"}')).toBeNull();
    expect(parseServerJson('[]')).toBeNull();
  });
});
