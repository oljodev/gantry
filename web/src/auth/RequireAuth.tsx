import type { ReactNode } from 'react'
import { Navigate, useLocation } from 'react-router-dom'
import { NotAuthorizedPage } from '../pages/NotAuthorizedPage'
import { useAuth } from './AuthProvider'

export function RequireAuth({ children }: { children: ReactNode }) {
  const { authEnabled, session, me, loading } = useAuth()
  const location = useLocation()

  if (!authEnabled) return <>{children}</>
  if (loading) {
    return (
      <div className="flex min-h-screen items-center justify-center text-sm text-zinc-500">
        <span className="animate-pulse">Restoring session…</span>
      </div>
    )
  }
  if (!session) {
    return <Navigate to="/login" replace state={{ from: location.pathname }} />
  }
  if (me && !me.allowed) return <NotAuthorizedPage />
  return <>{children}</>
}
