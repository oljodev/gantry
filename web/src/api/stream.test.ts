import { describe, expect, it } from 'vitest'
import { withToken } from './stream'

describe('withToken', () => {
  it('passes through when there is no token', () => {
    expect(withToken('/api/events/ws', null)).toBe('/api/events/ws')
  })

  it('starts a query when none exists', () => {
    expect(withToken('/api/events/ws', 'abc')).toBe('/api/events/ws?token=abc')
  })

  it('appends to an existing query', () => {
    expect(withToken('/api/tasks/1/events/ws?after_seq=5', 'abc')).toBe(
      '/api/tasks/1/events/ws?after_seq=5&token=abc',
    )
  })

  it('url-encodes the token', () => {
    expect(withToken('/ws', 'a+b/c=')).toBe('/ws?token=a%2Bb%2Fc%3D')
  })
})
