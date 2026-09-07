/**
 * The modules a React artifact may import (docs/plan/13 §6). Adding one is a line here plus
 * a line in the core prompt and `desktop/skills/artifact-authoring/references/react-runtime.md`.
 */

import * as React from 'react';
import * as ReactDOM from 'react-dom';
import * as ReactDOMClient from 'react-dom/client';
import * as JsxRuntime from 'react/jsx-runtime';
import * as Lucide from 'lucide-react';
import * as Recharts from 'recharts';
import clsx from 'clsx';

type Module = Record<string, unknown> & { default?: unknown };

function withDefault(m: Record<string, unknown>, fallback?: unknown): Module {
  return { ...m, default: (m as Module).default ?? fallback ?? m };
}

export const MODULES: Record<string, Module> = {
  react: withDefault(React as unknown as Record<string, unknown>, React),
  'react-dom': withDefault(ReactDOM as unknown as Record<string, unknown>, ReactDOM),
  'react-dom/client': withDefault(
    ReactDOMClient as unknown as Record<string, unknown>,
    ReactDOMClient,
  ),
  'react/jsx-runtime': withDefault(JsxRuntime as unknown as Record<string, unknown>, JsxRuntime),
  'lucide-react': withDefault(Lucide as unknown as Record<string, unknown>, Lucide),
  recharts: withDefault(Recharts as unknown as Record<string, unknown>, Recharts),
  clsx: withDefault({ clsx }, clsx),
};

export function availableList(): string {
  return Object.keys(MODULES).join(', ');
}
