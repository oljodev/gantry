import { describe, expect, it } from 'vitest'
import type { Task } from '../api/types'
import { deriveTitle, taskTitle } from './title'

describe('deriveTitle', () => {
  it('pairs a leading verb with the first filename mentioned', () => {
    expect(deriveTitle('Refactor board.py so no file exceeds 250 lines')).toBe('Refactor board.py')
    expect(deriveTitle('Create pieces.py with the piece classes')).toBe('Create pieces.py')
  })

  it('falls back to the first three words when there is no filename', () => {
    expect(deriveTitle('Add a login form to the dashboard')).toBe('Add a login')
  })

  it('strips surrounding punctuation but keeps the filename dot', () => {
    expect(deriveTitle('Update `gui.py`, then run the tests')).toBe('Update gui.py')
  })

  it('capitalizes and clamps overly long titles', () => {
    const t = deriveTitle('reticulate the splines across every subsystem thoroughly')
    expect(t[0]).toBe('R')
    expect(t.length).toBeLessThanOrEqual(32)
  })

  it('handles an empty or whitespace goal', () => {
    expect(deriveTitle('')).toBe('Task')
    expect(deriveTitle('   \n  ')).toBe('Task')
  })

  it('reads only the first line of a multi-line goal', () => {
    expect(deriveTitle('Fix util.ts\n\nlots of extra detail here')).toBe('Fix util.ts')
  })
})

function task(payload: Record<string, unknown>): Task {
  return { id: 'abcdef12-0000-0000-0000-000000000000', payload } as Task
}

describe('taskTitle', () => {
  it('prefers an explicit payload title', () => {
    expect(taskTitle(task({ title: 'API layer', goal: 'coordinate the whole api' }))).toBe(
      'API layer',
    )
  })

  it('derives from the goal when no explicit title is set', () => {
    expect(taskTitle(task({ goal: 'Refactor board.py now' }))).toBe('Refactor board.py')
  })

  it('ignores a blank explicit title and derives instead', () => {
    expect(taskTitle(task({ title: '   ', goal: 'Add pieces.py module' }))).toBe('Add pieces.py')
  })

  it('falls back to a short id when there is neither', () => {
    expect(taskTitle(task({}))).toBe('abcdef12')
  })
})
