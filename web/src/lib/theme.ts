// Theme is a single `data-theme` attribute on <html>; index.css redefines the
// colour ramps under `[data-theme="light"]`, so flipping the attribute
// re-themes everything at once.

import { useCallback, useEffect, useState } from 'react'

export type Theme = 'dark' | 'light'

const STORAGE_KEY = 'gantry.theme'

/** Stored preference, else the OS preference, else dark. */
export function preferredTheme(): Theme {
  const stored = localStorage.getItem(STORAGE_KEY)
  if (stored === 'dark' || stored === 'light') return stored
  return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme
}

export function useTheme(): { theme: Theme; toggle: () => void } {
  // Read straight from the DOM: main.tsx already applied the theme before
  // first paint, so this never disagrees with what the user is looking at.
  const [theme, setTheme] = useState<Theme>(
    () => (document.documentElement.dataset.theme as Theme | undefined) ?? 'dark',
  )

  useEffect(() => {
    applyTheme(theme)
    localStorage.setItem(STORAGE_KEY, theme)
  }, [theme])

  const toggle = useCallback(() => setTheme((t) => (t === 'dark' ? 'light' : 'dark')), [])
  return { theme, toggle }
}
