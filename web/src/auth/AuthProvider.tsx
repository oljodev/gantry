import type { Session } from '@supabase/supabase-js'
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from 'react'
import { getMe, putGithubToken } from '../api/client'
import type { Me } from '../api/types'
import { authEnabled, supabase } from '../lib/supabase'

interface AuthContextValue {
  authEnabled: boolean
  /** null until the initial session restore resolves (or when signed out). */
  session: Session | null
  /** Backend's verdict on the current token (allowlist check). */
  me: Me | null
  loading: boolean
  signInWithGitHub: () => void
  signOut: () => Promise<void>
}

const AuthContext = createContext<AuthContextValue>({
  authEnabled: false,
  session: null,
  me: null,
  loading: false,
  signInWithGitHub: () => {},
  signOut: async () => {},
})

// eslint-disable-next-line react-refresh/only-export-components
export function useAuth(): AuthContextValue {
  return useContext(AuthContext)
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [session, setSession] = useState<Session | null>(null)
  const [me, setMe] = useState<Me | null>(null)
  const [loading, setLoading] = useState(authEnabled)

  useEffect(() => {
    if (!supabase) return
    // Initial restore: without this, every reload flashes the login page.
    supabase.auth.getSession().then(({ data }) => {
      setSession(data.session)
      setLoading(false)
    })
    const { data: subscription } = supabase.auth.onAuthStateChange((event, next) => {
      setSession(next)
      // provider_token is only delivered right after the OAuth redirect —
      // store it immediately or lose private-repo access until reconnect.
      if (event === 'SIGNED_IN' && next?.provider_token) {
        putGithubToken(next.provider_token).catch((error) =>
          console.error('failed to store GitHub token', error),
        )
      }
    })
    return () => subscription.subscription.unsubscribe()
  }, [])

  useEffect(() => {
    if (!authEnabled || !session) {
      setMe(null)
      return
    }
    getMe().then(setMe).catch(console.error)
  }, [session])

  const signInWithGitHub = useCallback(() => {
    void supabase?.auth.signInWithOAuth({
      provider: 'github',
      options: { scopes: 'repo', redirectTo: location.origin },
    })
  }, [])

  const signOut = useCallback(async () => {
    await supabase?.auth.signOut()
  }, [])

  return (
    <AuthContext.Provider
      value={{ authEnabled, session, me, loading, signInWithGitHub, signOut }}
    >
      {children}
    </AuthContext.Provider>
  )
}
