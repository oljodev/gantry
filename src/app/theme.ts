import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

import { type Density, type ThemePref, useUiStore } from '@/lib/stores/uiStore';

const darkQuery = () => window.matchMedia('(prefers-color-scheme: dark)');

export function resolvedTheme(pref: ThemePref): 'light' | 'dark' {
  return pref === 'system' ? (darkQuery().matches ? 'dark' : 'light') : pref;
}

/**
 * Stamps the theme on <html> and keeps the native window in step: the OS-drawn chrome follows
 * `setTheme`, and the window background is set from the `--bg-base` token so a new window or a
 * resize never flashes the wrong colour (docs/plan/11 §3).
 */
export async function applyTheme(pref: ThemePref) {
  const root = document.documentElement;
  if (pref === 'system') root.removeAttribute('data-theme');
  else root.setAttribute('data-theme', pref);

  if (!isTauri()) return;
  const win = getCurrentWindow();
  try {
    await win.setTheme(pref === 'system' ? null : pref);
    const bg = getComputedStyle(root).getPropertyValue('--bg-base').trim();
    if (bg) await win.setBackgroundColor(bg);
  } catch (err) {
    console.warn('theme: native window update failed', err);
  }
}

export function applyDensity(density: Density) {
  const root = document.documentElement;
  if (density === 'compact') root.setAttribute('data-density', 'compact');
  else root.removeAttribute('data-density');
}

/** Called once at startup: applies the stored preferences and follows OS changes. */
export function initTheme() {
  const { theme, density } = useUiStore.getState();
  void applyTheme(theme);
  applyDensity(density);

  darkQuery().addEventListener('change', () => {
    if (useUiStore.getState().theme === 'system') void applyTheme('system');
  });

  useUiStore.subscribe((state, prev) => {
    if (state.theme !== prev.theme) void applyTheme(state.theme);
    if (state.density !== prev.density) applyDensity(state.density);
  });
}

/** Corrects the user-agent guess made in index.html with the OS plugin's answer. */
export async function initOs() {
  if (!isTauri()) return;
  try {
    const { type } = await import('@tauri-apps/plugin-os');
    const os = type();
    if (os === 'macos' || os === 'windows' || os === 'linux') {
      document.documentElement.dataset.os = os;
    }
  } catch (err) {
    console.warn('os: plugin unavailable', err);
  }
}
