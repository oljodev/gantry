// Supabase client for GitHub OAuth. When the env vars are absent the whole
// auth layer disables itself — matching the backend, which only enforces
// JWTs when GANTRY_SUPABASE_URL is set.
//
// The client is built lazily on first use, not at import time: constructing a
// SupabaseClient eagerly spins up a realtime WebSocket, which breaks under
// Node < 22 (no global WebSocket) and would drag unit tests that merely import
// the API layer into building a client they never use.

import { createClient, type SupabaseClient } from '@supabase/supabase-js'

const url = import.meta.env.VITE_SUPABASE_URL as string | undefined
const key = import.meta.env.VITE_SUPABASE_KEY as string | undefined

export const authEnabled = Boolean(url && key)

let cached: SupabaseClient | null | undefined

export function getSupabase(): SupabaseClient | null {
  if (cached === undefined) {
    cached = authEnabled ? createClient(url!, key!) : null
  }
  return cached
}

/**
 * The current access token, or null when auth is disabled / signed out.
 * supabase-js caches the session and auto-refreshes it, so calling this per
 * request or per WebSocket (re)connect is cheap and always fresh.
 */
export async function getAccessToken(): Promise<string | null> {
  const client = getSupabase()
  if (!client) return null
  const { data } = await client.auth.getSession()
  return data.session?.access_token ?? null
}
