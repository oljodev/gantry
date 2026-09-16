import { create } from 'zustand';
import { persist } from 'zustand/middleware';

import type { CustomizeSection, Section } from '@/features/settings/sections';

export type ThemePref = 'system' | 'light' | 'dark';
export type Density = 'comfortable' | 'compact';
/** Which half of the app is showing (docs/plan/16 §4). */
export type Surface = 'chat' | 'code';

interface UiState {
  theme: ThemePref;
  density: Density;
  sidebarWidth: number;
  sidebarCollapsed: boolean;
  paneWidth: number;
  surface: Surface;
  /** Where each surface was last, so switching back returns to it rather than to its index. */
  lastRoute: Record<Surface, string | null>;
  setSurface: (surface: Surface) => void;
  rememberRoute: (surface: Surface, path: string) => void;
  /**
   * Models chosen lately, newest first, as `provider/model`. Kept here rather than in settings:
   * favourites are a decision worth syncing, recents are a trace of this machine's use.
   */
  recentModels: string[];
  rememberModel: (key: string) => void;
  /**
   * Chats where the user has confirmed turning the guard off (04 §5). Once per chat, and here
   * rather than in settings because it is a record of one answer on one machine, not a
   * preference worth syncing.
   */
  unguarded: string[];
  rememberUnguarded: (chatId: string) => void;
  /**
   * The incognito chat on screen, or null (15 A21). Held here rather than read off the route so
   * that leaving the route can delete it: the session lives exactly as long as it is showing,
   * and a component-lifecycle cleanup would fire on StrictMode's second mount in development.
   * Deliberately not persisted — a restart must not resurrect one.
   */
  incognitoChatId: string | null;
  setIncognitoChat: (chatId: string | null) => void;
  /**
   * Whether the first-launch steps have been walked through or skipped (15 A20). Here rather
   * than in settings because it is not a preference: it records that a screen has been seen, on
   * this machine, the way `unguarded` records an answer given once. The gate that reads it also
   * requires the chat list to be empty, so clearing this storage cannot make an established
   * install sit through onboarding again.
   */
  onboarded: boolean;
  finishOnboarding: () => void;
  /**
   * A folder chosen during onboarding, before any chat exists to attach it to. The welcome
   * screen picks it up and clears it, which is where the roots chosen from the `+` menu already
   * wait for the first message.
   */
  pendingRoots: string[];
  setPendingRoots: (roots: string[]) => void;
  /** The open section of each dialog, or null when it is closed (15 A18). */
  settings: Section | null;
  customize: CustomizeSection | null;
  /**
   * What to look for once the Customize dialog is open, when something opened it *at* a thing
   * — the palette landing on one connector, say. The section seeds its own search from this
   * and it stays set until the next open, so opening the dialog by hand finds it null.
   */
  customizeFind: string | null;
  openSettings: (section?: Section) => void;
  closeSettings: () => void;
  openCustomize: (section?: CustomizeSection, find?: string) => void;
  closeCustomize: () => void;
  setTheme: (theme: ThemePref) => void;
  setDensity: (density: Density) => void;
  setSidebarWidth: (width: number) => void;
  toggleSidebar: () => void;
  setPaneWidth: (width: number, max: number) => void;
}

type Persisted = Pick<
  UiState,
  | 'theme'
  | 'density'
  | 'onboarded'
  | 'pendingRoots'
  | 'sidebarWidth'
  | 'sidebarCollapsed'
  | 'paneWidth'
  | 'surface'
  | 'lastRoute'
  | 'recentModels'
>;

/** Enough to hold the models in rotation without the list becoming a second favourites. */
export const RECENT_MODELS = 8;

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
      surface: 'chat',
      lastRoute: { chat: null, code: null },
      setSurface: (surface) => set({ surface }),
      rememberRoute: (surface, path) =>
        set((s) => ({ surface, lastRoute: { ...s.lastRoute, [surface]: path } })),
      recentModels: [],
      rememberModel: (key) =>
        set((s) => ({
          recentModels: [key, ...s.recentModels.filter((k) => k !== key)].slice(0, RECENT_MODELS),
        })),
      incognitoChatId: null,
      setIncognitoChat: (incognitoChatId) => set({ incognitoChatId }),
      onboarded: false,
      finishOnboarding: () => set({ onboarded: true }),
      pendingRoots: [],
      setPendingRoots: (pendingRoots) => set({ pendingRoots }),
      settings: null,
      customize: null,
      customizeFind: null,
      // Only one of the two is ever open: they are the same kind of surface.
      openSettings: (section = 'general') => set({ settings: section, customize: null }),
      closeSettings: () => set({ settings: null }),
      openCustomize: (section = 'connectors', find = undefined) =>
        set({ customize: section, settings: null, customizeFind: find ?? null }),
      closeCustomize: () => set({ customize: null }),
      unguarded: [],
      rememberUnguarded: (chatId) =>
        set((s) => (s.unguarded.includes(chatId) ? s : { unguarded: [...s.unguarded, chatId] })),
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
        surface: s.surface,
        lastRoute: s.lastRoute,
        recentModels: s.recentModels,
        unguarded: s.unguarded,
        onboarded: s.onboarded,
        pendingRoots: s.pendingRoots,
      }),
    },
  ),
);
