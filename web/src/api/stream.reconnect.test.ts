import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Hermetic: never depend on whether the developer has web/.env.local set —
// the auth layer is mocked so both auth-on and auth-off paths are exercised
// deliberately.
const auth = { authEnabled: false, token: null as string | null }
vi.mock('../lib/supabase', () => ({
  get authEnabled() {
    return auth.authEnabled
  },
  getAccessToken: async () => auth.token,
}))

const { openFirehose } = await import('./stream')

// Minimal fake WebSocket that records instances and lets the test drive
// lifecycle callbacks.
class FakeWebSocket {
  static instances: FakeWebSocket[] = []
  onopen: (() => void) | null = null
  onclose: ((e: { code: number }) => void) | null = null
  onmessage: ((e: { data: string }) => void) | null = null
  closed = false
  url: string
  constructor(url: string) {
    this.url = url
    FakeWebSocket.instances.push(this)
  }
  close() {
    this.closed = true
  }
  open() {
    this.onopen?.()
  }
  serverClose(code = 4401) {
    this.onclose?.({ code })
  }
}

async function flush() {
  await Promise.resolve()
  await Promise.resolve()
}

/** Advance fake time until a new socket appears; returns ms waited. */
async function waitForNextSocket(limitMs = 20_000): Promise<number> {
  const before = FakeWebSocket.instances.length
  let waited = 0
  while (FakeWebSocket.instances.length === before && waited < limitMs) {
    await vi.advanceTimersByTimeAsync(50)
    await flush()
    waited += 50
  }
  return waited
}

describe('websocket reconnect behaviour', () => {
  beforeEach(() => {
    FakeWebSocket.instances = []
    auth.authEnabled = false
    auth.token = null
    vi.stubGlobal('WebSocket', FakeWebSocket as unknown as typeof WebSocket)
    vi.stubGlobal('location', {
      origin: 'http://localhost:8400',
      protocol: 'http:',
      host: 'localhost:8400',
    })
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
    vi.unstubAllGlobals()
  })

  it('backs off exponentially when the server keeps closing immediately', async () => {
    const stop = openFirehose(() => {})
    await flush()
    expect(FakeWebSocket.instances).toHaveLength(1)

    // Simulate accept-then-immediate-4401 (what an auth-rejected socket does).
    // Gaps must grow instead of pinning at the 500ms floor.
    const gaps: number[] = []
    for (let i = 0; i < 4; i++) {
      const ws = FakeWebSocket.instances.at(-1)!
      ws.open()
      ws.serverClose(4401)
      gaps.push(await waitForNextSocket())
    }
    expect(gaps[0]).toBeLessThan(gaps[1])
    expect(gaps[1]).toBeLessThan(gaps[2])
    expect(gaps[2]).toBeLessThan(gaps[3])
    expect(gaps.at(-1)!).toBeGreaterThanOrEqual(1000)
    stop()
  })

  it('resets the backoff after a stable connection', async () => {
    const stop = openFirehose(() => {})
    await flush()
    const ws = FakeWebSocket.instances.at(-1)!
    ws.open()
    await vi.advanceTimersByTimeAsync(5_000) // stay open past STABLE_MS
    ws.serverClose(1006)
    expect(await waitForNextSocket()).toBeLessThanOrEqual(600)
    stop()
  })

  it('does not open a doomed socket when auth is on but no token is available', async () => {
    auth.authEnabled = true
    auth.token = null
    const stop = openFirehose(() => {})
    await flush()
    // A tokenless socket would just be 4401'd — none should be created at all.
    expect(FakeWebSocket.instances).toHaveLength(0)
    await vi.advanceTimersByTimeAsync(2_000)
    await flush()
    expect(FakeWebSocket.instances).toHaveLength(0)
    stop()
  })

  it('connects with the token once a session exists', async () => {
    auth.authEnabled = true
    auth.token = 'jwt-abc'
    const stop = openFirehose(() => {})
    await flush()
    expect(FakeWebSocket.instances).toHaveLength(1)
    expect(FakeWebSocket.instances[0].url).toContain('token=jwt-abc')
    stop()
  })
})
