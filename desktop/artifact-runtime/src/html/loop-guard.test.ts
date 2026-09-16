/**
 * The other half of docs/plan/13 §5's hang risk: an `html` artifact's own inline scripts.
 *
 * Two claims are worth separating. The first is that the rewrite is *safe* — that a document
 * which worked before still works, which is the only reason to be allowed near somebody's
 * code at all. The second is that it *fires*. Most of what follows is the first.
 *
 * The engine end of it — the same rewrite, in a real WebKitGTK view, against a page that means
 * to hang — is `an_html_artifacts_inline_script_is_loop_guarded` in
 * `desktop/crates/gantry-agent/tests/artifacts_sandbox.rs`.
 */

import { describe, expect, it } from 'vitest';

import {
  guardHtmlScripts,
  guardScript,
  LOOP_BUDGET_MS,
  LOOP_GUARD_MESSAGE,
  LOOP_GUARD_RUNTIME,
} from './loop-guard.js';

interface FakeWindow {
  __gantryLoopCheck: () => void;
  __gantryLoopIter: <T>(it: Iterable<T>) => Iterable<T>;
}

/**
 * The guard's own runtime, on a clock the test turns. Every reading of the clock moves it on
 * by `perCall` milliseconds, which is how a loop "takes" time here; the event loop never gets
 * a turn, because that is the situation the guard exists for.
 */
function installed(perCall: number): FakeWindow {
  let now = 0;
  const clock = {
    now: () => {
      now += perCall;
      return now;
    },
  };
  const window = {} as FakeWindow;
  new Function('window', 'Date', 'setInterval', LOOP_GUARD_RUNTIME)(window, clock, () => 0);
  return window;
}

/** Runs one artifact script under that runtime and returns what it put in `out`. */
function run(source: string, window: FakeWindow, out: unknown[] = []): unknown[] {
  new Function('window', 'out', guardScript(source))(window, out);
  return out;
}

describe('what the guard leaves alone', () => {
  const untouched = [
    ['a method named for', 'const o = { for() {}, while() {} };'],
    ['a loop word inside a string', 'const s = "while (true) {}";'],
    ['a loop word inside a comment', '// while (true) {}\nconst a = 1;'],
    ['a loop word inside a template', 'const t = `${x} while (true)`;'],
    ['a property named while', 'const n = obj.while + obj.for;'],
    ['for…in, whose keys are finite', 'for (const k in o) f(k);'],
    [
      'for await, which a sync wrapper would break',
      'async function f() { for await (const c of s) g(c); }',
    ],
    ['a regular expression that looks like division', 'const r = /for (a)/g.test(s);'],
    ['a script that does not parse', 'function ( {'],
    ['a script with no loop at all', 'document.title = "hello";'],
  ] as const;
  for (const [what, source] of untouched) {
    it(what, () => {
      expect(guardScript(source)).toBe(source);
    });
  }

  it('an unterminated string, where the scan can no longer be trusted', () => {
    const source = 'const s = "open;\nwhile (true) {}';
    expect(guardScript(source)).toBe(source);
  });
});

describe('what the guard rewrites', () => {
  it('a while head, keeping the condition value', () => {
    expect(guardScript('while (a < b) f();')).toBe(
      'while (window.__gantryLoopCheck(), a < b) f();',
    );
  });

  it('a do…while, which is the same head', () => {
    expect(guardScript('do f(); while (a);')).toBe(
      'do f(); while (window.__gantryLoopCheck(), a);',
    );
  });

  it('an empty for head, which needs a value of its own', () => {
    expect(guardScript('for (;;) f();')).toBe('for (; window.__gantryLoopCheck(), true;) f();');
  });

  it('a counting for, in the condition where it belongs', () => {
    expect(guardScript('for (let i = 0; i < n; i++) f();')).toBe(
      'for (let i = 0; window.__gantryLoopCheck(), i < n; i++) f();',
    );
  });

  it('a for…of, by wrapping what it walks', () => {
    expect(guardScript('for (const x of xs) f(x);')).toBe(
      'for (const x of window.__gantryLoopIter( xs)) f(x);',
    );
  });

  it('a loop with no braces at all, which is why nothing looks for a body', () => {
    expect(guardScript('while (true);')).toBe('while (window.__gantryLoopCheck(), true);');
  });

  it('a loop nested inside another', () => {
    const guarded = guardScript('while (a) { for (;;) f(); }');
    expect(guarded.match(/__gantryLoopCheck/g)).toHaveLength(2);
  });
});

describe('what the guard does when it runs', () => {
  it('is budgeted at the three seconds 13 §5 states, for both artifact kinds', () => {
    expect(LOOP_BUDGET_MS).toBe(3000);
    expect(LOOP_GUARD_RUNTIME).toContain(LOOP_GUARD_MESSAGE);
  });

  for (const [shape, source] of [
    ['while', 'while (true) { out.push(1); }'],
    ['do/while', 'do { out.push(1); } while (true);'],
    ['for', 'for (;;) { out.push(1); }'],
    [
      'for…of',
      'function* forever() { let i = 0; while (true) yield i++; }\nfor (const x of forever()) out.push(x);',
    ],
  ] as const) {
    it(`stops a runaway ${shape} loop`, () => {
      const window = installed(200);
      expect(() => run(source, window)).toThrow(new RegExp(LOOP_GUARD_MESSAGE));
    });
  }

  it('lets a loop that finishes finish, with the answer it had before', () => {
    const window = installed(1);
    const source = 'let n = 0;\nfor (let i = 0; i < 1000; i++) n += i;\nout.push(n);';
    expect(run(source, window)).toEqual([499_500]);
  });

  it('walks a for…of over the same items, in the same order', () => {
    const window = installed(1);
    expect(run('for (const x of ["a", "b", "c"]) out.push(x);', window)).toEqual(['a', 'b', 'c']);
  });

  it('leaves a value that cannot be walked to fail the way it would have', () => {
    const window = installed(1);
    expect(() => run('for (const x of 7) out.push(x);', window)).toThrow(TypeError);
  });
});

describe('which scripts in a document are rewritten', () => {
  it('an inline one', () => {
    expect(guardHtmlScripts('<body><script>while (true) {}</script></body>')).toContain(
      '__gantryLoopCheck',
    );
  });

  it('a module, which is still JavaScript', () => {
    expect(guardHtmlScripts('<script type="module">while (true) {}</script>')).toContain(
      '__gantryLoopCheck',
    );
  });

  it('not one with a src, whose body is not here', () => {
    const doc = '<script src="x.js">while (true) {}</script>';
    expect(guardHtmlScripts(doc)).toBe(doc);
  });

  it('not one holding data rather than code', () => {
    const doc = '<script type="application/json">{"while": "(true) {}"}</script>';
    expect(guardHtmlScripts(doc)).toBe(doc);
  });

  it('every one of several, and nothing around them', () => {
    const doc =
      '<head><script>while (a) {}</script></head><body><p>while (true)</p>' +
      '<SCRIPT >for (;;) {}</SCRIPT ></body>';
    const guarded = guardHtmlScripts(doc);
    expect(guarded.match(/__gantryLoopCheck/g)).toHaveLength(2);
    expect(guarded).toContain('<p>while (true)</p>');
  });
});
