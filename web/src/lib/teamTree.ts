// Pure tree-editing logic behind the team builder. All operations are
// immutable (return new trees) so React state is a single root node.
// Editor nodes carry client-side keys; the API shape (TeamNode) has none.

import type { TeamNode, TeamNodeOut } from '../api/types'

export interface EditNode {
  key: string
  profile_id: string | null
  children: EditNode[]
}

let counter = 0
export function nextKey(): string {
  counter += 1
  return `n${counter}`
}

export function emptyNode(): EditNode {
  return { key: nextKey(), profile_id: null, children: [] }
}

/** Editor tree from the API's expanded team tree. */
export function fromApi(node: TeamNodeOut): EditNode {
  return {
    key: nextKey(),
    profile_id: node.profile.id,
    children: node.children.map(fromApi),
  }
}

/** API write shape; throws if any node has no profile assigned. */
export function toApi(node: EditNode): TeamNode {
  if (!node.profile_id) throw new Error('every node needs an agent assigned')
  return { profile_id: node.profile_id, children: node.children.map(toApi) }
}

export function isComplete(node: EditNode): boolean {
  return node.profile_id !== null && node.children.every(isComplete)
}

export function countNodes(node: EditNode): number {
  return 1 + node.children.reduce((sum, child) => sum + countNodes(child), 0)
}

export interface FlatNode {
  node: EditNode
  depth: number
  parentKey: string | null
}

/** Depth-first flatten for rendering as indented rows. */
export function flatten(
  node: EditNode,
  depth = 0,
  parentKey: string | null = null,
): FlatNode[] {
  return [
    { node, depth, parentKey },
    ...node.children.flatMap((child) => flatten(child, depth + 1, node.key)),
  ]
}

function contains(node: EditNode, key: string): boolean {
  return node.key === key || node.children.some((child) => contains(child, key))
}

function findNode(node: EditNode, key: string): EditNode | null {
  if (node.key === key) return node
  for (const child of node.children) {
    const found = findNode(child, key)
    if (found) return found
  }
  return null
}

function map(node: EditNode, fn: (n: EditNode) => EditNode): EditNode {
  return fn({ ...node, children: node.children.map((child) => map(child, fn)) })
}

export function addChild(root: EditNode, parentKey: string, child?: EditNode): EditNode {
  const node = child ?? emptyNode()
  return map(root, (n) => (n.key === parentKey ? { ...n, children: [...n.children, node] } : n))
}

export function setProfile(root: EditNode, key: string, profileId: string): EditNode {
  return map(root, (n) => (n.key === key ? { ...n, profile_id: profileId } : n))
}

/** Remove a subtree. Returns null when asked to remove the root itself. */
export function removeNode(root: EditNode, key: string): EditNode | null {
  if (root.key === key) return null
  return map(root, (n) => ({
    ...n,
    children: n.children.filter((child) => child.key !== key),
  }))
}

/**
 * Move a subtree under a new parent. No-op (returns the input) when the move
 * would be degenerate: unknown keys, moving under itself or its own subtree,
 * or moving the root.
 */
export function reparent(root: EditNode, key: string, newParentKey: string): EditNode {
  if (key === root.key || key === newParentKey) return root
  const moving = findNode(root, key)
  if (!moving || !findNode(root, newParentKey)) return root
  if (contains(moving, newParentKey)) return root
  const without = removeNode(root, key)
  if (!without) return root
  return addChild(without, newParentKey, moving)
}
