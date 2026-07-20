import { useState } from 'react'
import { Outlet } from 'react-router-dom'
import { Sidebar } from './components/Sidebar'
import { AppDataProvider } from './state/AppDataProvider'

export function App() {
  const [menuOpen, setMenuOpen] = useState(false)
  return (
    <AppDataProvider>
      <div className="flex min-h-screen">
        <Sidebar open={menuOpen} onClose={() => setMenuOpen(false)} />
        <div className="min-w-0 flex-1">
          <header className="sticky top-0 z-10 flex h-12 items-center gap-3 border-b border-zinc-800 bg-zinc-950/90 px-4 backdrop-blur lg:hidden">
            <button onClick={() => setMenuOpen(true)} aria-label="Open menu" className="text-lg">
              ☰
            </button>
            <span className="font-semibold tracking-tight">
              <span className="text-amber-400" aria-hidden>
                ⌬{' '}
              </span>
              Gantry
            </span>
          </header>
          <main className="mx-auto max-w-7xl px-4 py-6">
            <Outlet />
          </main>
        </div>
      </div>
    </AppDataProvider>
  )
}
