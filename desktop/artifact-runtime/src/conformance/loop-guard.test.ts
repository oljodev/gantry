/**
 * The compile-time half of docs/plan/13 §5's hang risk: a loop in a `react` artifact that runs
 * past the budget must throw rather than freeze the window it shares with the app.
 *
 * `compile.test.ts` holds that the guard is injected. This holds that it fires — the two are
 * not the same claim, and an injected guard whose runtime is missing or whose comparison is
 * the wrong way round would pass the first and fail every artifact.
 *
 * The other half, an `html` artifact's own inline scripts, is not compiled at all; see
 * `an_html_artifacts_inline_script_is_loop_guarded` in
 * `desktop/crates/gantry-agent/tests/artifacts_sandbox.rs`.
 */

import { beforeAll, describe, expect, it } from 'vitest';

import { compile } from '../react/compile';
import { installLoopGuardRuntime, LOOP_BUDGET_MS } from '../react/loop-guard';

/** The runtime installs itself on `window`, which the compiled code names. */
function asArtifactWindow() {
  const g = globalThis as { window?: unknown };
  g.window ??= globalThis;
}

/** Compiles one artifact and returns its default export. */
function mount(source: string): () => unknown {
  const out = compile(source, 'tsx');
  if ('error' in out) throw new Error(out.error.message);
  const exports: Record<string, unknown> = {};
  new Function('__gantryExports', '__gantryRequire', out.code)(exports, () => ({}));
  return exports.default as () => unknown;
}

describe('the loop guard', () => {
  beforeAll(() => {
    asArtifactWindow();
    // A tenth of a second instead of three, so the test costs what it measures.
    installLoopGuardRuntime(100);
  });

  it('is budgeted at the three seconds 13 §5 states', () => {
    expect(LOOP_BUDGET_MS).toBe(3000);
  });

  for (const [shape, body] of [
    ['while', 'let n = 0;\n  while (true) { n += 1; }\n  return n;'],
    ['do/while', 'let n = 0;\n  do { n += 1; } while (true);\n  return n;'],
    ['for', 'let n = 0;\n  for (;;) { n += 1; }\n  return n;'],
    ['for…of', 'let n = 0;\n  for (const x of forever()) { n += x; }\n  return n;'],
  ] as const) {
    it(`stops a runaway ${shape} loop`, () => {
      const App = mount(
        `function* forever() { let i = 1; while (true) yield i++; }\n` +
          `export default function App() {\n  ${body}\n}\n`,
      );
      expect(App).toThrow(/Artifact loop guard/);
    });
  }

  it('leaves a loop that finishes alone', () => {
    const App = mount(
      'export default function App() {\n  let n = 0;\n  for (let i = 0; i < 1000; i++) n += i;\n  return n;\n}\n',
    );
    expect(App()).toBe(499_500);
  });
});
