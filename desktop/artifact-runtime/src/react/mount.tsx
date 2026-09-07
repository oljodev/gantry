/**
 * Compiles and mounts a React artifact into `#root` (docs/plan/13 §6): the default export is
 * the component (a named `App` is the fallback), rendered with no props inside an error
 * boundary that reports through the bridge.
 */

import * as React from 'react';
import { createRoot, type Root } from 'react-dom/client';

import { ready, reportError } from '../bridge';
import { compile } from './compile';
import { installLoopGuardRuntime } from './loop-guard';
import { MODULES } from './modules';

let root: Root | null = null;

class Boundary extends React.Component<
  { children: React.ReactNode; onError: (error: Error, info: React.ErrorInfo) => void },
  { error: Error | null }
> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    this.props.onError(error, info);
  }

  render() {
    if (this.state.error) {
      return (
        <pre
          style={{
            fontFamily: 'ui-monospace, monospace',
            fontSize: 12,
            whiteSpace: 'pre-wrap',
            color: '#b42318',
          }}
        >
          {this.state.error.message}
        </pre>
      );
    }
    return this.props.children;
  }
}

function evaluate(code: string): Record<string, unknown> {
  const exports: Record<string, unknown> = {};
  const require = (name: string) => {
    const m = MODULES[name];
    if (!m) throw new Error(`Module "${name}" is not available in Gantry artifacts`);
    return m;
  };
  // eslint-disable-next-line @typescript-eslint/no-implied-eval
  const run = new Function('__gantryRequire', '__gantryExports', code) as (
    r: typeof require,
    e: Record<string, unknown>,
  ) => void;
  run(require, exports);
  return exports;
}

export function mountReact(content: string, language: string | undefined, container: HTMLElement) {
  installLoopGuardRuntime();
  const compiled = compile(content, language === 'jsx' ? 'jsx' : 'tsx');
  if ('error' in compiled) {
    reportError({ phase: 'compile', ...compiled.error });
    return;
  }
  let exports: Record<string, unknown>;
  try {
    exports = evaluate(compiled.code);
  } catch (err) {
    const e = err as Error;
    reportError({ phase: 'runtime', message: e.message, stack: e.stack });
    return;
  }
  const Component = (exports.default ?? exports.App) as React.ComponentType | undefined;
  if (typeof Component !== 'function') {
    reportError({
      phase: 'compile',
      message: 'The artifact has no default export (or named App export) that is a component.',
    });
    return;
  }
  root?.unmount();
  root = createRoot(container);
  let failed = false;
  root.render(
    <Boundary
      onError={(error, info) => {
        failed = true;
        reportError({
          phase: 'runtime',
          message: error.message,
          stack: error.stack,
          componentStack: info.componentStack ?? undefined,
        });
      }}
    >
      <Component />
    </Boundary>,
  );
  // React commits asynchronously; the first paint after the commit is when the render either
  // held or threw into the boundary.
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      if (!failed) ready();
    });
  });
}
