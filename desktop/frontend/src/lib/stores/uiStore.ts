import { create } from 'zustand';
import { persist } from 'zustand/middleware';

import type { CustomizeSection, Section } from '@/features/settings/sections';

export type ThemePref = 'system' | 'light' | 'dark';
export type Density = 'comfortable' | 'compact';

interface UiState {
  theme: ThemePref;
  density: Density;
  sidebarWidth: number;
  sidebarCollapsed: boolean;
  paneWidth: number;
  /** The open section of each dialog, or null when it is closed (15 A18). */
  settings: Section | null;
  customize: CustomizeSection | null;
  openSettings: (section?: Section) => void;
  closeSettings: () => void;
  openCustomize: (section?: CustomizeSection) => void;
  closeCustomize: () => void;
  setTheme: (theme: ThemePref) => void;
  setDensity: (density: Density) => void;
  setSidebarWidth: (width: number) => void;
  toggleSidebar: () => void;
  setPaneWidth: (width: number, max: number) => void;
}

type Persisted = Pick<
  UiState,
  'theme' | 'density' | 'sidebarWidth' | 'sidebarCollapsed' | 'paneWidth'
>;

export const SIDEBAR_MIN = 200;
export const SIDEBAR_MAX = 320;
export const SIDEBAR_DEFAULT = 240;
export const PANE_MIN = 360;
/** `paneWidth` 0 means "half the window", the width the pane opens at until it is dragged. */
export const PANE_DEFAULT = 0;

/** The pane's width for a window: the dragged width, else half the window (15 A17). */
export function paneWidthFor(stored: number, windowWidth: number): number {
  const half = Math.floor(windowWidth / 2);
  return stored > 0 ? Math.min(stored, half) : half;
}

/**
 * Per-window UI preferences, persisted to localStorage under `gantry.ui`. The inline script in
 * index.html reads the same key before first paint. Theme and density are mirrored here; the
 * backend settings table becomes the source of truth in M2 (docs/plan/11 §3).
 */
export const useUiStore = create<UiState>()(
  persist(
    (set) => ({
      theme: 'system',
      density: 'comfortable',
      sidebarWidth: SIDEBAR_DEFAULT,
      sidebarCollapsed: false,
      paneWidth: PANE_DEFAULT,
      settings: null,
      customize: null,
      // Only one of the two is ever open: they are the same kind of surface.
      openSettings: (section = 'general') => set({ settings: section, customize: null }),
      closeSettings: () => set({ settings: null }),
      openCustomize: (section = 'connectors') => set({ customize: section, settings: null }),
      closeCustomize: () => set({ customize: null }),
      setTheme: (theme) => set({ theme }),
      setDensity: (density) => set({ density }),
      setSidebarWidth: (width) =>
        set({ sidebarWidth: Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Math.round(width))) }),
      toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
      setPaneWidth: (width, max) =>
        set({ paneWidth: Math.min(max, Math.max(PANE_MIN, Math.round(width))) }),
    }),
    {
      name: 'gantry.ui',
      // v1: the pane opens at half the window instead of a fixed 440 px.
      version: 1,
      migrate: (persisted, version) => {
        const s = persisted as Persisted;
        return version < 1 ? { ...s, paneWidth: PANE_DEFAULT } : s;
      },
      partialize: (s) => ({
        theme: s.theme,
        density: s.density,
        sidebarWidth: s.sidebarWidth,
        sidebarCollapsed: s.sidebarCollapsed,
        paneWidth: s.paneWidth,
      }),
    },
  ),
);
