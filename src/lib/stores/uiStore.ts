import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type ThemePref = 'system' | 'light' | 'dark';
export type Density = 'comfortable' | 'compact';

interface UiState {
  theme: ThemePref;
  density: Density;
  sidebarWidth: number;
  sidebarCollapsed: boolean;
  setTheme: (theme: ThemePref) => void;
  setDensity: (density: Density) => void;
  setSidebarWidth: (width: number) => void;
  toggleSidebar: () => void;
}

export const SIDEBAR_MIN = 200;
export const SIDEBAR_MAX = 320;
export const SIDEBAR_DEFAULT = 240;

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
      setTheme: (theme) => set({ theme }),
      setDensity: (density) => set({ density }),
      setSidebarWidth: (width) =>
        set({ sidebarWidth: Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Math.round(width))) }),
      toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
    }),
    {
      name: 'gantry.ui',
      partialize: (s) => ({
        theme: s.theme,
        density: s.density,
        sidebarWidth: s.sidebarWidth,
        sidebarCollapsed: s.sidebarCollapsed,
      }),
    },
  ),
);
