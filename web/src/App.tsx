import { useState } from 'react'
import { Outlet } from 'react-router-dom'
import { Hexagon, Menu } from 'lucide-react'
import { API_BASE } from './api/base'
import { Sidebar } from './components/Sidebar'
import { AppDataProvider, useAppData } from './state/AppDataProvider'

export function App() {
  const [menuOpen, setMenuOpen] = useState(false)
  return (
    <AppDataProvider>
      <div className="min-h-screen">
        <Sidebar open={menuOpen} onClose={() => setMenuOpen(false)} />
        {/* The sidebar is fixed, so the shell reserves its width instead of
            sitting beside it in flow. */}
        <div className="min-w-0 lg:pl-56">
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
          <main className="mx-auto max-w-7xl px-4 py-6">
            <Outlet />
          </main>
        </div>
      </div>
    </AppDataProvider>
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
