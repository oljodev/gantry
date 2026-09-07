import { describe, expect, it } from 'vitest';

import { compile } from './compile';

describe('compile', () => {
  it('rewrites allowed imports, guards loops and registers the default export', () => {
    const out = compile(
      `import { useState } from "react";\nimport clsx from "clsx";\nexport default function App() {\n  for (let i = 0; i < 3; i++) {}\n  return <div className={clsx("a")}>{useState(0)[0]}</div>;\n}\n`,
      'tsx',
    );
    if ('error' in out) throw new Error(out.error.message);
    expect(out.code).toContain('__gantryRequire("react")');
    expect(out.code).toContain('__gantryLoopCheck');
    expect(out.code).toContain('__gantryExports.default = App');
    expect(out.code).not.toContain('import ');
  });

  it('refuses imports outside the allowlist with the list', () => {
    const out = compile(`import axios from "axios";\nexport default () => null;`, 'tsx');
    expect('error' in out && out.error.message).toContain('"axios" is not available');
    expect('error' in out && out.error.message).toContain('recharts');
  });

  it('reports syntax errors with a line', () => {
    const out = compile(`export default function App() {\n  return <div>\n}`, 'tsx');
    expect('error' in out && out.error.line).toBeGreaterThan(0);
  });
});
