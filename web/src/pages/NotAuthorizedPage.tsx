import { useAuth } from '../auth/AuthProvider'

export function NotAuthorizedPage() {
  const { me, signOut } = useAuth()
  return (
    <div className="flex min-h-screen items-center justify-center px-4">
      <div className="w-full max-w-sm rounded-xl border border-zinc-800 bg-zinc-900/40 p-8 text-center">
        <div className="mb-3 text-3xl" aria-hidden>
          🔒
        </div>
        <h1 className="text-lg font-semibold">Not authorized</h1>
        <p className="mt-2 text-sm text-zinc-400">
          {me?.email ? (
            <>
              <span className="font-mono text-zinc-200">{me.email}</span> is not on this Gantry
              instance's allowlist.
            </>
          ) : (
            'This account is not on the allowlist.'
          )}
        </p>
        <p className="mt-2 text-xs text-zinc-600">
          Ask the operator to add your email to <code className="font-mono">GANTRY_ALLOWED_EMAILS</code>.
        </p>
        <button
          onClick={() => void signOut()}
          className="mt-6 rounded-lg border border-zinc-700 px-4 py-2 text-sm text-zinc-300 transition hover:bg-zinc-900"
        >
          Sign out
        </button>
      </div>
    </div>
  )
}
