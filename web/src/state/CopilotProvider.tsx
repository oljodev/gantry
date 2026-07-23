// Shared state for the AI co-pilot dock. The dock lives once at the app shell
// (right-docked, resizable, pushes the main content aside rather than covering
// it); pages open it for a specific task by calling open() with a config that
// says what it is drafting and how to apply the proposal.

import { createContext, useCallback, useContext, useMemo, useState } from 'react'

/** Undo a previously applied proposal. */
export type Applied = () => Promise<void>

export interface CopilotConfig {
  kind: 'skill' | 'tree'
  /** Panel title, e.g. "Skill co-pilot" or a team name. */
  title: string
  projectId: string
  /** The team a tree co-pilot is scoped to — also scopes its saved chats. */
  teamId?: string
  /** Current editor state handed to the architect as context (JSON string). */
  context: string
  /** Inject a proposal into the editor; return a function that undoes it. */
  onApply: (proposal: Record<string, unknown>) => Promise<Applied>
}

interface CopilotCtx {
  config: CopilotConfig | null
  width: number
  open: (config: CopilotConfig) => void
  close: () => void
  setWidth: (px: number) => void
}

const WIDTH_KEY = 'gantry.copilot.width'
export const MIN_WIDTH = 340
export const MAX_WIDTH = 760

function initialWidth(): number {
  const saved = Number(localStorage.getItem(WIDTH_KEY))
  return saved >= MIN_WIDTH && saved <= MAX_WIDTH ? saved : 440
}

const Context = createContext<CopilotCtx | null>(null)

export function CopilotProvider({ children }: { children: React.ReactNode }) {
  const [config, setConfig] = useState<CopilotConfig | null>(null)
  const [width, setWidthState] = useState(initialWidth)

  const setWidth = useCallback((px: number) => {
    const clamped = Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, Math.round(px)))
    setWidthState(clamped)
    localStorage.setItem(WIDTH_KEY, String(clamped))
  }, [])

  const open = useCallback((next: CopilotConfig) => setConfig(next), [])
  const close = useCallback(() => setConfig(null), [])

  const value = useMemo(
    () => ({ config, width, open, close, setWidth }),
    [config, width, open, close, setWidth],
  )
  return <Context.Provider value={value}>{children}</Context.Provider>
}

export function useCopilot(): CopilotCtx {
  const ctx = useContext(Context)
  if (!ctx) throw new Error('useCopilot must be used within a CopilotProvider')
  return ctx
}
