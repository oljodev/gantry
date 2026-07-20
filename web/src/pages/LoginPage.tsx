import { Navigate } from 'react-router-dom'
import { useAuth } from '../auth/AuthProvider'

export function LoginPage() {
  const { authEnabled, session, loading, signInWithGitHub } = useAuth()
  if (!authEnabled || (session && !loading)) return <Navigate to="/" replace />

  return (
    <div className="flex min-h-screen items-center justify-center px-4">
      <div className="w-full max-w-sm rounded-xl border border-zinc-800 bg-zinc-900/40 p-8 text-center">
        <div className="mb-2 text-4xl text-amber-400" aria-hidden>
          ⌬
        </div>
        <h1 className="text-xl font-semibold tracking-tight">Gantry</h1>
        <p className="mt-1 mb-8 text-sm text-zinc-500">durable agent orchestration</p>
        <button
          onClick={signInWithGitHub}
          className="w-full rounded-lg bg-zinc-100 px-4 py-2.5 text-sm font-medium text-zinc-900 transition hover:bg-white"
        >
          Continue with GitHub
        </button>
        <p className="mt-4 text-xs text-zinc-600">
          Requests the <code className="font-mono">repo</code> scope so workers can clone your
          private repositories.
        </p>
      </div>
    </div>
  )
}
