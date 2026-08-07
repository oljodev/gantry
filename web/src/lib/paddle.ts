// Paddle Billing (Merchant of Record) checkout — the inline Overlay Checkout,
// loaded lazily via Paddle.js so nothing pays for it until a user actually
// clicks "Buy credits".
//
// Configuration is build-time env, matching how Supabase is wired (see
// lib/supabase.ts): when the vars are absent — the state this ships in today,
// ahead of the real Paddle account existing — the feature disables itself
// rather than rendering a button that can't work.

const CLIENT_TOKEN = import.meta.env.VITE_PADDLE_CLIENT_TOKEN as string | undefined
const DEFAULT_PRICE_ID = import.meta.env.VITE_PADDLE_DEFAULT_PRICE_ID as string | undefined
const ENVIRONMENT = import.meta.env.VITE_PADDLE_ENVIRONMENT as string | undefined

export const paddleConfigured = Boolean(CLIENT_TOKEN && DEFAULT_PRICE_ID)

export function defaultPriceId(): string {
  if (!DEFAULT_PRICE_ID) throw new Error('Paddle is not configured (VITE_PADDLE_DEFAULT_PRICE_ID)')
  return DEFAULT_PRICE_ID
}

// --- the Paddle.js surface we actually use --------------------------------
// Minimal by design: just enough of Paddle's global to type-check our calls,
// not a full SDK mirror.

interface PaddleCheckoutItem {
  priceId: string
  quantity?: number
}

interface PaddleCheckoutOptions {
  items: PaddleCheckoutItem[]
  customData?: Record<string, string>
  customer?: { email: string }
}

interface PaddleGlobal {
  Environment: { set: (env: string) => void }
  Initialize: (options: { token: string }) => void
  Checkout: { open: (options: PaddleCheckoutOptions) => void }
}

declare global {
  interface Window {
    Paddle?: PaddleGlobal
  }
}

const SCRIPT_SRC = 'https://cdn.paddle.com/paddle/v2/paddle.js'

let loading: Promise<PaddleGlobal> | null = null

/** Injects and initializes Paddle.js on first call; every later call reuses
 *  the same instance. The promise is reset on failure (script blocked, network
 *  down) so a transient failure doesn't permanently break the button for the
 *  rest of the session — the next click gets a fresh attempt. */
export function loadPaddle(): Promise<PaddleGlobal> {
  if (!CLIENT_TOKEN) return Promise.reject(new Error('Paddle client token is not configured'))
  if (window.Paddle) return Promise.resolve(window.Paddle)
  if (loading) return loading

  loading = new Promise<PaddleGlobal>((resolve, reject) => {
    const existing = document.querySelector<HTMLScriptElement>(`script[src="${SCRIPT_SRC}"]`)
    const script = existing ?? document.createElement('script')
    script.src = SCRIPT_SRC
    script.async = true
    script.addEventListener('load', () => {
      if (!window.Paddle) {
        reject(new Error('Paddle.js loaded but window.Paddle is missing'))
        return
      }
      if (ENVIRONMENT === 'sandbox') window.Paddle.Environment.set('sandbox')
      window.Paddle.Initialize({ token: CLIENT_TOKEN })
      resolve(window.Paddle)
    })
    script.addEventListener('error', () => reject(new Error('failed to load Paddle.js')))
    if (!existing) document.head.appendChild(script)
  })
  loading.catch(() => {
    loading = null
  })
  return loading
}

/** The options passed to `Paddle.Checkout.open`, as a pure data transform —
 *  split out from `openCheckout` so it is testable without touching Paddle.js
 *  or the DOM at all.
 *
 * `customData.user_id` is the ONLY link between a Paddle purchase and a Gantry
 * account — Paddle echoes it back verbatim on the `transaction.completed` /
 * `subscription.created` webhook, which is how the backend knows whose balance
 * to credit (see `gantry.billing.paddle.extract_grant_intent`). The key name
 * must match exactly on both sides. */
export function buildCheckoutOptions(options: {
  userId: string
  email?: string
  priceId: string
}): PaddleCheckoutOptions {
  return {
    items: [{ priceId: options.priceId, quantity: 1 }],
    customData: { user_id: options.userId },
    ...(options.email ? { customer: { email: options.email } } : {}),
  }
}

/** Open the inline Overlay Checkout for one price (the default configured
 *  price when none is given). */
export async function openCheckout(options: {
  userId: string
  email?: string
  priceId?: string
}): Promise<void> {
  const paddle = await loadPaddle()
  paddle.Checkout.open(
    buildCheckoutOptions({ ...options, priceId: options.priceId ?? defaultPriceId() }),
  )
}
