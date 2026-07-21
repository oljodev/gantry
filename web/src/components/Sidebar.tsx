import { Link, NavLink } from 'react-router-dom'
import {
  Bot,
  CheckCheck,
  Hexagon,
  LayoutDashboard,
  ListOrdered,
  Moon,
  Network,
  Play,
  Settings,
  Sparkles,
  Sun,
  type LucideIcon,
} from 'lucide-react'
import { useAuth } from '../auth/AuthProvider'
import { useAppData } from '../state/AppDataProvider'
import { useTheme } from '../lib/theme'
import type { ConnectionState } from '../api/stream'

const NAV: Array<{ to: string; icon: LucideIcon; label: string; end?: boolean }> = [
  { to: '/', icon: LayoutDashboard, label: 'Dashboard', end: true },
  { to: '/runs', icon: ListOrdered, label: 'Runs' },
  { to: '/launch', icon: Play, label: 'Launch' },
  { to: '/agents', icon: Bot, label: 'Agents' },
  { to: '/teams', icon: Network, label: 'Teams' },
  { to: '/approvals', icon: CheckCheck, label: 'Approvals' },
  { to: '/skills', icon: Sparkles, label: 'Skills' },
  { to: '/settings', icon: Settings, label: 'Settings' },
]

const CONNECTION_TONE: Record<ConnectionState, [string, string]> = {
  connecting: ['connecting', 'bg-zinc-600'],
  live: ['live', 'bg-emerald-500'],
  reconnecting: ['reconnecting', 'bg-amber-500 animate-pulse'],
  ended: ['ended', 'bg-zinc-600'],
}

export function Sidebar({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { approvals, connection } = useAppData()
  const { authEnabled, me, signOut } = useAuth()
  const { theme, toggle } = useTheme()
  const [connLabel, connDot] = CONNECTION_TONE[connection]

  return (
    <>
      {open && (
        <button
          aria-label="Close menu"
          onClick={onClose}
          className="fixed inset-0 z-20 bg-black/60 lg:hidden"
        />
      )}
      {/* Always `fixed`: the sidebar is pinned to the viewport so long task
          traces scroll underneath it rather than dragging it off-screen. The
          shell compensates with lg:pl-56. */}
      <aside
        className={`fixed inset-y-0 left-0 z-30 flex w-56 flex-col border-r border-zinc-800 bg-zinc-950 transition-transform lg:translate-x-0 ${
          open ? 'translate-x-0' : '-translate-x-full'
        }`}
      >
        <Link
          to="/"
          onClick={onClose}
          className="flex h-12 shrink-0 items-center gap-2 border-b border-zinc-800 px-4 font-semibold tracking-tight"
        >
          <Hexagon className="h-4 w-4 text-amber-400" aria-hidden />
          <span>Gantry</span>
        </Link>
        {/* Only the nav scrolls; the brand and footer stay put. */}
        <nav className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 py-3 text-sm">
          {NAV.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              onClick={onClose}
              className={({ isActive }) =>
                `flex items-center gap-2.5 rounded-md px-2.5 py-1.5 transition ${
                  isActive
                    ? 'bg-zinc-900 text-amber-300'
                    : 'text-zinc-400 hover:bg-zinc-900/60 hover:text-zinc-200'
                }`
              }
            >
              <item.icon className="h-4 w-4 shrink-0" aria-hidden />
              <span>{item.label}</span>
              {item.to === '/approvals' && approvals.length > 0 && (
                <span className="ml-auto rounded-full bg-purple-900/80 px-1.5 py-0.5 text-[10px] font-semibold text-purple-200">
                  {approvals.length}
                </span>
              )}
            </NavLink>
          ))}
        </nav>
        <div className="shrink-0 border-t border-zinc-800 p-3 text-xs">
          <div className="flex items-center gap-2 text-zinc-500">
            <span className={`h-1.5 w-1.5 rounded-full ${connDot}`} aria-hidden />
            <span>{connLabel}</span>
            <span className="grow" />
            <button
              onClick={toggle}
              title={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
              aria-label={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
              className="-m-1.5 rounded-md p-2.5 text-zinc-500 transition hover:bg-zinc-900 hover:text-zinc-200"
            >
              {theme === 'dark' ? (
                <Sun className="h-3.5 w-3.5" aria-hidden />
              ) : (
                <Moon className="h-3.5 w-3.5" aria-hidden />
              )}
            </button>
          </div>
          {authEnabled && me?.email && (
            <div className="mt-2 flex items-center justify-between gap-2">
              <span className="truncate font-mono text-zinc-400" title={me.email}>
                {me.email}
              </span>
              <button
                onClick={() => void signOut()}
                className="shrink-0 text-zinc-600 transition hover:text-zinc-300"
              >
                sign out
              </button>
            </div>
          )}
        </div>
      </aside>
    </>
  )
}
