import { Link, NavLink } from 'react-router-dom'
import {
  ArrowLeft,
  BarChart3,
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
import { projectPath, useProjectId } from '../lib/project'
import type { ConnectionState } from '../api/stream'

interface NavItem {
  to: string
  icon: LucideIcon
  label: string
  end?: boolean
  badge?: number
}

const CONNECTION_TONE: Record<ConnectionState, [string, string]> = {
  connecting: ['connecting', 'bg-zinc-600'],
  live: ['live', 'bg-emerald-500'],
  reconnecting: ['reconnecting', 'bg-amber-500 animate-pulse'],
  ended: ['ended', 'bg-zinc-600'],
}

export function Sidebar({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { connection, approvals, questions } = useAppData()
  const { authEnabled, me, signOut } = useAuth()
  const { theme, toggle } = useTheme()
  const projectId = useProjectId()
  const [connLabel, connDot] = CONNECTION_TONE[connection]
  const pendingHitl = approvals.length + questions.length

  const main: NavItem[] = [
    { to: projectPath(projectId), icon: LayoutDashboard, label: 'Dashboard', end: true },
    { to: projectPath(projectId, 'runs'), icon: ListOrdered, label: 'Runs' },
    { to: projectPath(projectId, 'launch'), icon: Play, label: 'Launch run' },
  ]
  const ai: NavItem[] = [
    { to: projectPath(projectId, 'agents'), icon: Network, label: 'Tree' },
    { to: projectPath(projectId, 'skills'), icon: Sparkles, label: 'Skills' },
    {
      to: projectPath(projectId, 'approved'),
      icon: CheckCheck,
      label: 'Approved',
      badge: pendingHitl,
    },
  ]

  return (
    <>
      {open && (
        <button
          aria-label="Close menu"
          onClick={onClose}
          className="fixed inset-0 z-20 bg-black/60 lg:hidden"
        />
      )}
      <aside
        className={`fixed inset-y-0 left-0 z-30 flex w-56 flex-col border-r border-zinc-800 bg-zinc-950 transition-transform lg:translate-x-0 ${
          open ? 'translate-x-0' : '-translate-x-full'
        }`}
      >
        <Link
          to={projectPath(projectId)}
          onClick={onClose}
          className="flex h-12 shrink-0 items-center gap-2 border-b border-zinc-800 px-4 font-semibold tracking-tight"
        >
          <Hexagon className="h-4 w-4 text-amber-400" aria-hidden />
          <span>Gantry</span>
        </Link>
        {/* Back to the project picker — the "home" the in-project shell sits on. */}
        <Link
          to="/"
          onClick={onClose}
          className="flex shrink-0 items-center gap-2 border-b border-zinc-800/60 px-4 py-2 text-xs text-zinc-500 transition hover:text-zinc-300"
        >
          <ArrowLeft className="h-3.5 w-3.5" aria-hidden />
          All projects
        </Link>

        <nav className="flex flex-1 flex-col gap-0.5 overflow-y-auto px-2 py-3 text-sm">
          <Section items={main} onClose={onClose} />
          <p className="mt-3 px-2.5 pb-1 text-[10px] font-semibold tracking-wider text-zinc-600">
            AI
          </p>
          <Section items={ai} onClose={onClose} />
        </nav>

        {/* Usage + Settings pinned above the footer, per the IA. */}
        <div className="shrink-0 border-t border-zinc-800 px-2 py-2">
          <Section
            items={[
              { to: projectPath(projectId, 'usage'), icon: BarChart3, label: 'Usage' },
              { to: projectPath(projectId, 'settings'), icon: Settings, label: 'Settings' },
            ]}
            onClose={onClose}
          />
        </div>

        <div className="shrink-0 border-t border-zinc-800 p-3 text-xs">
          <div className="flex items-center gap-2 text-zinc-500">
            <span className={`h-1.5 w-1.5 rounded-full ${connDot}`} aria-hidden />
            <span>{connLabel}</span>
            <span className="grow" />
            <button
              onClick={toggle}
              title={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
              aria-label={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
              className="grid h-6 w-6 place-items-center rounded-md text-zinc-500 transition hover:bg-zinc-900 hover:text-zinc-200"
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

function Section({ items, onClose }: { items: NavItem[]; onClose: () => void }) {
  return (
    <>
      {items.map((item) => (
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
          <span className="grow">{item.label}</span>
          {item.badge ? (
            <span className="rounded-full bg-amber-600 px-1.5 text-[10px] font-semibold text-zinc-950">
              {item.badge}
            </span>
          ) : null}
        </NavLink>
      ))}
    </>
  )
}
