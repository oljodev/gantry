// Where the API lives. Same-origin by default (dev proxy, or FastAPI serving
// the built SPA). On a split deployment — e.g. the frontend on Cloudflare
// Pages and the backend on a server or behind a Cloudflare Tunnel — set
// VITE_API_BASE to the backend's origin (e.g. https://api.gantry.oljo.dev)
// at build time.

export const API_BASE = (import.meta.env.VITE_API_BASE as string | undefined)?.replace(
  /\/+$/,
  '',
) ?? ''

/** Absolute (or same-origin) URL for an API path like "/api/tasks". */
export function apiUrl(path: string): string {
  return `${API_BASE}${path}`
}

/** ws:// or wss:// URL for a WebSocket path, honoring API_BASE's host. */
export function apiWsUrl(path: string): string {
  const origin = API_BASE || location.origin
  const url = new URL(origin)
  const scheme = url.protocol === 'https:' ? 'wss' : 'ws'
  return `${scheme}://${url.host}${path}`
}

/** True if the browser is showing a bundle with no backend configured and
 *  none reachable at the same origin (used for friendlier offline states). */
export class BackendUnreachableError extends Error {
  constructor(path: string, detail: string) {
    super(
      `Backend not reachable at ${API_BASE || location.origin} (requesting ${path}): ${detail}. ` +
        `If the frontend is deployed separately from the API, set VITE_API_BASE to the backend origin and rebuild.`,
    )
    this.name = 'BackendUnreachableError'
  }
}
