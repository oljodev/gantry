import { create } from 'zustand';

import type { RenderReport } from '@/bindings';
import type { ConsoleLine } from '@/features/artifacts/bridge';
import { DEFAULT_ZOOM } from '@/features/artifacts/zoom';

/** The parts of an artifact call's arguments seen so far (13 §2). */
export interface StreamingArtifact {
  callId: string;
  chatId: string;
  tool: string;
  artifactId?: string;
  type?: string;
  title?: string;
  language?: string;
  content: string;
  done: boolean;
}

export interface Problem {
  at: number;
  level: string;
  text: string;
}

export interface ArtifactView {
  mode: 'rendered' | 'source';
  /** The version shown; `undefined` follows the current one. */
  version?: number;
  editing: boolean;
  draft: string;
  problems: Problem[];
  /**
   * The artifact has been stopped by hand (13 §5, hang risk 2): its frame is unmounted and
   * stays unmounted until Run. Per artifact, not per version, and cleared whenever the panel
   * moves to a different version.
   */
  stopped: boolean;
  /**
   * Page zoom for this box (13 §4), on the ladder in `zoom.ts`. Per artifact rather than per
   * panel: two artifacts open side by side are two different things to look at, and a diagram
   * somebody zoomed to 200 % should not decide how the next one opens.
   */
  zoom: number;
  /** Reports sent for `${version}`, so each version is reported once. */
  reported: Record<string, RenderReport>;
}

interface ArtifactState {
  /** Open artifact tabs per chat, in order. */
  openByChat: Record<string, string[]>;
  activeByChat: Record<string, string | undefined>;
  views: Record<string, ArtifactView>;
  /** Calls whose arguments are streaming, by call id. */
  streaming: Record<string, StreamingArtifact>;
  /** Call id → artifact id, once the create call's result is known. */
  createdBy: Record<string, string>;
  open: (chatId: string, artifactId: string, activate?: boolean) => void;
  close: (chatId: string, artifactId: string) => void;
  activate: (chatId: string, artifactId: string) => void;
  patchView: (artifactId: string, patch: Partial<ArtifactView>) => void;
  addProblem: (artifactId: string, line: ConsoleLine) => void;
  clearProblems: (artifactId: string) => void;
  markReported: (artifactId: string, version: number, report: RenderReport) => void;
  setStreaming: (s: StreamingArtifact) => void;
  finishStreaming: (callId: string) => void;
  noteCreated: (callId: string, artifactId: string) => void;
  dropStreaming: (callId: string) => void;
}

export const emptyView = (): ArtifactView => ({
  mode: 'rendered',
  editing: false,
  draft: '',
  problems: [],
  stopped: false,
  zoom: DEFAULT_ZOOM,
  reported: {},
});

export const useArtifactStore = create<ArtifactState>()((set) => ({
  openByChat: {},
  activeByChat: {},
  views: {},
  streaming: {},
  createdBy: {},
  open: (chatId, artifactId, activate = true) =>
    set((s) => {
      const open = s.openByChat[chatId] ?? [];
      const next = open.includes(artifactId) ? open : [...open, artifactId];
      return {
        openByChat: { ...s.openByChat, [chatId]: next },
        activeByChat: activate ? { ...s.activeByChat, [chatId]: artifactId } : s.activeByChat,
        views: s.views[artifactId] ? s.views : { ...s.views, [artifactId]: emptyView() },
      };
    }),
  close: (chatId, artifactId) =>
    set((s) => {
      const open = (s.openByChat[chatId] ?? []).filter((id) => id !== artifactId);
      const active =
        s.activeByChat[chatId] === artifactId ? open[open.length - 1] : s.activeByChat[chatId];
      return {
        openByChat: { ...s.openByChat, [chatId]: open },
        activeByChat: { ...s.activeByChat, [chatId]: active },
      };
    }),
  activate: (chatId, artifactId) =>
    set((s) => ({ activeByChat: { ...s.activeByChat, [chatId]: artifactId } })),
  patchView: (artifactId, patch) =>
    set((s) => ({
      views: { ...s.views, [artifactId]: { ...(s.views[artifactId] ?? emptyView()), ...patch } },
    })),
  addProblem: (artifactId, line) =>
    set((s) => {
      const view = s.views[artifactId] ?? emptyView();
      const problems = [...view.problems, { at: Date.now(), ...line }].slice(-200);
      return { views: { ...s.views, [artifactId]: { ...view, problems } } };
    }),
  clearProblems: (artifactId) =>
    set((s) => ({
      views: {
        ...s.views,
        [artifactId]: { ...(s.views[artifactId] ?? emptyView()), problems: [] },
      },
    })),
  markReported: (artifactId, version, report) =>
    set((s) => {
      const view = s.views[artifactId] ?? emptyView();
      return {
        views: {
          ...s.views,
          [artifactId]: { ...view, reported: { ...view.reported, [String(version)]: report } },
        },
      };
    }),
  setStreaming: (streaming) =>
    set((s) => ({ streaming: { ...s.streaming, [streaming.callId]: streaming } })),
  finishStreaming: (callId) =>
    set((s) => {
      const cur = s.streaming[callId];
      if (!cur) return {};
      return { streaming: { ...s.streaming, [callId]: { ...cur, done: true } } };
    }),
  noteCreated: (callId, artifactId) =>
    set((s) => ({ createdBy: { ...s.createdBy, [callId]: artifactId } })),
  dropStreaming: (callId) =>
    set((s) => {
      const streaming = { ...s.streaming };
      delete streaming[callId];
      return { streaming };
    }),
}));

/** The streaming call, if any, that targets this artifact (an update or edit in flight). */
export function streamingFor(
  streaming: Record<string, StreamingArtifact>,
  createdBy: Record<string, string>,
  artifactId: string,
): StreamingArtifact | undefined {
  return Object.values(streaming).find(
    (s) => !s.done && (s.artifactId === artifactId || createdBy[s.callId] === artifactId),
  );
}
