// Supabase client for GitHub OAuth. When the env vars are absent the whole
// auth layer disables itself — matching the backend, which only enforces
// JWTs when GANTRY_SUPABASE_URL is set.

import { createClient, type SupabaseClient } from '@supabase/supabase-js'

const url = import.meta.env.VITE_SUPABASE_URL as string | undefined
const key = import.meta.env.VITE_SUPABASE_KEY as string | undefined

export const authEnabled = Boolean(url && key)

export const supabase: SupabaseClient | null = authEnabled ? createClient(url!, key!) : null

/**
 * The current access token, or null when auth is disabled / signed out.
 * supabase-js caches the session and auto-refreshes it, so calling this per
 * request or per WebSocket (re)connect is cheap and always fresh.
 */
export async function getAccessToken(): Promise<string | null> {
  if (!supabase) return null
  const { data } = await supabase.auth.getSession()
  return data.session?.access_token ?? null
}
