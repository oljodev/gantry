import { describe, expect, it } from 'vitest'
import { buildCheckoutOptions, defaultPriceId, paddleConfigured } from './paddle'

// Component/DOM-touching pieces (loadPaddle, openCheckout) inject a script tag
// and call window.Paddle — untested at the unit level, matching how
// lib/supabase.ts's env-gated client init is exercised only via `npm run
// build` and manual QA, not a unit test. What IS unit-tested here is the pure
// data transform: what we'd send Paddle, independent of how we get there.

describe('buildCheckoutOptions', () => {
  it('links the purchase to the account via customData.user_id', () => {
    const options = buildCheckoutOptions({ userId: 'user-123', priceId: 'pri_abc' })
    expect(options.items).toEqual([{ priceId: 'pri_abc', quantity: 1 }])
    expect(options.customData).toEqual({ user_id: 'user-123' })
  })

  it('omits the customer block when no email is given', () => {
    const options = buildCheckoutOptions({ userId: 'user-123', priceId: 'pri_abc' })
    expect(options.customer).toBeUndefined()
  })

  it('prefills the customer email when one is given', () => {
    const options = buildCheckoutOptions({
      userId: 'user-123',
      priceId: 'pri_abc',
      email: 'a@example.com',
    })
    expect(options.customer).toEqual({ email: 'a@example.com' })
  })
})

describe('unconfigured Paddle (no real account yet)', () => {
  // The test env has none of the VITE_PADDLE_* build-time vars set — which is
  // also the actual state this integration ships in today. That is the real
  // behaviour under test, not a mock of it.

  it('reports itself as not configured', () => {
    expect(paddleConfigured).toBe(false)
  })

  it('refuses to guess a price id rather than open checkout for nothing', () => {
    expect(() => defaultPriceId()).toThrow(/not configured/)
  })
})
