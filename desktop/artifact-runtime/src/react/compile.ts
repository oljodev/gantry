/**
 * TSX/JSX → JavaScript with `@babel/standalone`, the React and TypeScript presets, and the
 * two Gantry plugins (docs/plan/13 §6). Errors carry the line and column of the artifact
 * source, which is the file Babel sees.
 */

import * as Babel from '@babel/standalone';

import { ImportError, importsPlugin } from './imports';
import { loopGuardPlugin } from './loop-guard';

let registered = false;

export interface CompileError {
  message: string;
  line?: number;
  column?: number;
}

export function compile(
  source: string,
  language: 'tsx' | 'jsx',
): { code: string } | { error: CompileError } {
  if (!registered) {
    Babel.registerPlugin('gantry-loop-guard', loopGuardPlugin as never);
    Babel.registerPlugin('gantry-imports', importsPlugin as never);
    registered = true;
  }
  try {
    const out = Babel.transform(source, {
      filename: language === 'tsx' ? 'artifact.tsx' : 'artifact.jsx',
      // Babel 8's TypeScript preset reads the dialect off the filename.
      presets: ['typescript', ['react', { runtime: 'automatic' }]],
      plugins: ['gantry-imports', 'gantry-loop-guard'],
      sourceType: 'module',
      retainLines: true,
    });
    return { code: out.code ?? '' };
  } catch (err) {
    const e = err as { message?: string; loc?: { line?: number; column?: number } };
    const message =
      err instanceof ImportError
        ? err.message
        : (e.message ?? String(err)).replace(/^\/?artifact\.[tj]sx: /, '');
    return {
      error: {
        message,
        line: e.loc?.line,
        column: e.loc?.column !== undefined ? e.loc.column + 1 : undefined,
      },
    };
  }
}
