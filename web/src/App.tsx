import { Link, Outlet } from 'react-router-dom'

export function App() {
  return (
    <div className="min-h-screen">
      <header className="sticky top-0 z-10 border-b border-zinc-800 bg-zinc-950/90 backdrop-blur">
        <div className="mx-auto flex h-12 max-w-7xl items-center gap-3 px-4">
          <Link to="/" className="flex items-center gap-2 font-semibold tracking-tight">
            <span className="text-amber-400" aria-hidden>
              ⌬
            </span>
            <span>Gantry</span>
          </Link>
          <span className="text-xs text-zinc-500">durable agent orchestration</span>
        </div>
      </header>
      <main className="mx-auto max-w-7xl px-4 py-6">
        <Outlet />
      </main>
    </div>
  )
}
