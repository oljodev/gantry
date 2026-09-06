import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { Ajv2020 } from 'ajv/dist/2020';
import { describe, expect, it } from 'vitest';

const root = fileURLToPath(new URL('..', import.meta.url));
const ajv = new Ajv2020({ allErrors: true, strict: true, formats: { uri: true } });

const manifestSchema = JSON.parse(
  readFileSync(join(root, 'schemas/connector-manifest.schema.json'), 'utf8'),
);
const skillSchema = JSON.parse(
  readFileSync(join(root, 'schemas/skill-frontmatter.schema.json'), 'utf8'),
);

describe('connector manifest schema', () => {
  const validate = ajv.compile(manifestSchema);
  const dirs = readdirSync(join(root, 'connectors')).filter((d: string) =>
    statSync(join(root, 'connectors', d)).isDirectory(),
  );

  it('finds the bundled connectors', () => {
    expect(dirs).toEqual(expect.arrayContaining(['filesystem', 'code-editor', 'shell', 'web']));
  });

  for (const dir of dirs) {
    it(`validates connectors/${dir}/manifest.json`, () => {
      const manifest = JSON.parse(
        readFileSync(join(root, 'connectors', dir, 'manifest.json'), 'utf8'),
      );
      const ok = validate(manifest);
      expect(validate.errors ?? []).toEqual([]);
      expect(ok).toBe(true);
      expect(manifest.id).toBe(dir);
    });
  }

  it('rejects a manifest with an unknown runtime', () => {
    expect(
      validate({
        manifest_version: '1',
        id: 'x',
        name: 'x',
        description: 'x',
        version: '1.0.0',
        icon: 'icon.svg',
        category: 'web',
        publisher: { name: 'x' },
        runtime: { kind: 'sidecar' },
        auth: { type: 'none' },
        risk: { network: 'none', local_system: 'none', default_tool_tier: 'read' },
      }),
    ).toBe(false);
  });
});

describe('skill frontmatter schema', () => {
  const validate = ajv.compile(skillSchema);

  it('accepts the example from the plan', () => {
    expect(
      validate({
        name: 'rust-idioms',
        description: 'Idiomatic Rust for this codebase. Use when writing or reviewing Rust.',
        license: 'FSL-1.1-ALv2',
        metadata: {
          'gantry-triggers': 'rust, cargo, clippy',
          'gantry-always': 'false',
          'gantry-version': '2',
          author: 'olav',
        },
      }),
    ).toBe(true);
  });

  it('rejects a name that is not a slug', () => {
    expect(validate({ name: 'Rust Idioms', description: 'x' })).toBe(false);
  });
});
