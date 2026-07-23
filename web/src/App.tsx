import { useState } from 'react'
import { Outlet, useLocation } from 'react-router-dom'
import { Hexagon, Menu } from 'lucide-react'
import { API_BASE } from './api/base'
import { ApprovalToasts } from './components/ApprovalToasts'
import { CopilotDock } from './components/CopilotDock'
import { QuestionToasts } from './components/QuestionToasts'
import { Sidebar } from './components/Sidebar'
import { AppDataProvider, useAppData } from './state/AppDataProvider'
import { CopilotProvider, useCopilot } from './state/CopilotProvider'
import { useProjectId } from './lib/project'

export function App() {
  const projectId = useProjectId()
  return (
    <AppDataProvider projectId={projectId}>
      <CopilotProvider>
        <Shell />
      </CopilotProvider>
    </AppDataProvider>
  )
}

function Shell() {
  const [menuOpen, setMenuOpen] = useState(false)
  const { config, width } = useCopilot()
  // The run page is a full-bleed, full-height 3-column workspace; every other
  // page is a centred, scrolling document.
  const fullBleed = /\/tasks\/[^/]+$/.test(useLocation().pathname)
  return (
    <div className="min-h-screen">
      <Sidebar open={menuOpen} onClose={() => setMenuOpen(false)} />
      {/* The sidebar is fixed on the left, so the shell reserves its width; the
          co-pilot dock is fixed on the right, so when open the shell reserves
          its width too — content is pushed aside, never covered. */}
      <div
        className="min-w-0 transition-[padding] duration-150 lg:pl-56"
        style={{ paddingRight: config ? width : 0 }}
      >
        <header className="sticky top-0 z-10 flex h-12 items-center gap-3 border-b border-zinc-800 bg-zinc-950/90 px-4 backdrop-blur lg:hidden">
          <button onClick={() => setMenuOpen(true)} aria-label="Open menu">
            <Menu className="h-5 w-5" aria-hidden />
          </button>
          <span className="flex items-center gap-2 font-semibold tracking-tight">
            <Hexagon className="h-4 w-4 text-amber-400" aria-hidden />
            Gantry
          </span>
        </header>
        <OfflineBanner />
        {fullBleed ? (
          <main className="h-[100dvh] lg:overflow-hidden">
            <Outlet />
          </main>
        ) : (
          <main className="mx-auto max-w-7xl px-4 py-6">
            <Outlet />
          </main>
        )}
      </div>
      <CopilotDock />
      <ApprovalToasts />
      <QuestionToasts />
    </div>
  )
}

function OfflineBanner() {
  const { online } = useAppData()
  if (online !== false) return null
  return (
    <div className="border-b border-amber-900/60 bg-amber-950/40 px-4 py-2 text-center text-xs text-amber-200">
      Backend not reachable{API_BASE ? ` at ${API_BASE}` : ''}. The dashboard is static; it needs
      the Gantry API running. See the deployment notes in <span className="font-mono">CLAUDE.md</span>.
    </div>
  )
}
