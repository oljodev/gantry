import { describe, expect, it } from 'vitest'
import type { TeamNodeOut } from '../api/types'
import {
  addChild,
  countNodes,
  emptyNode,
  flatten,
  fromApi,
  isComplete,
  removeNode,
  reparent,
  setProfile,
  toApi,
  type EditNode,
} from './teamTree'

function tree(): EditNode {
  // root(a) -> [child1(b) -> [leaf(c)], child2(d)]
  return {
    key: 'root',
    profile_id: 'a',
    children: [
      { key: 'child1', profile_id: 'b', children: [{ key: 'leaf', profile_id: 'c', children: [] }] },
      { key: 'child2', profile_id: 'd', children: [] },
    ],
  }
}

describe('teamTree', () => {
  it('addChild appends under the right parent without mutating', () => {
    const root = tree()
    const next = addChild(root, 'child2', { key: 'new', profile_id: null, children: [] })
    expect(next.children[1].children.map((n) => n.key)).toEqual(['new'])
    expect(root.children[1].children).toEqual([]) // original untouched
  })

  it('setProfile updates exactly one node', () => {
    const next = setProfile(tree(), 'leaf', 'z')
    expect(flatten(next).map((f) => f.node.profile_id)).toEqual(['a', 'b', 'z', 'd'])
  })

  it('removeNode drops the whole subtree', () => {
    const next = removeNode(tree(), 'child1')
    expect(next).not.toBeNull()
    expect(flatten(next!).map((f) => f.node.key)).toEqual(['root', 'child2'])
  })

  it('removeNode of the root returns null', () => {
    expect(removeNode(tree(), 'root')).toBeNull()
  })

  it('reparent moves a subtree', () => {
    const next = reparent(tree(), 'leaf', 'child2')
    expect(flatten(next).map((f) => `${f.node.key}@${f.depth}`)).toEqual([
      'root@0',
      'child1@1',
      'child2@1',
      'leaf@2',
    ])
  })

  it('reparent into its own subtree is a no-op', () => {
    const root = tree()
    expect(reparent(root, 'child1', 'leaf')).toBe(root)
    expect(reparent(root, 'child1', 'child1')).toBe(root)
    expect(reparent(root, 'root', 'child2')).toBe(root)
  })

  it('flatten yields depth-first order with parents', () => {
    expect(flatten(tree()).map((f) => [f.node.key, f.depth, f.parentKey])).toEqual([
      ['root', 0, null],
      ['child1', 1, 'root'],
      ['leaf', 2, 'child1'],
      ['child2', 1, 'root'],
    ])
  })

  it('toApi round-trips fromApi', () => {
    const api: TeamNodeOut = {
      profile: { id: 'p1' } as TeamNodeOut['profile'],
      children: [{ profile: { id: 'p2' } as TeamNodeOut['profile'], children: [] }],
    }
    expect(toApi(fromApi(api))).toEqual({
      profile_id: 'p1',
      children: [{ profile_id: 'p2', children: [] }],
    })
  })

  it('toApi throws on unassigned nodes; isComplete flags them', () => {
    const incomplete = addChild(tree(), 'root', emptyNode())
    expect(isComplete(incomplete)).toBe(false)
    expect(() => toApi(incomplete)).toThrow()
    expect(isComplete(tree())).toBe(true)
    expect(countNodes(incomplete)).toBe(5)
  })
})
