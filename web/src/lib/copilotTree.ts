// Apply a co-pilot tree proposal: create the agents it describes (reusing any
// that already exist by name), then create the team wiring them together.
// Returns an undo function that deletes the team and the agents this created.

import {
  createAgent,
  createTeam,
  deleteAgent,
  deleteTeam,
  listAgents,
} from '../api/client'
import type { AgentProfileCreate, TeamNode } from '../api/types'

interface ProposalNode {
  name?: unknown
  role?: unknown
  system_prompt?: unknown
  can_spawn?: unknown
  gated_tools?: unknown
  skills?: unknown
  children?: unknown
}

function strList(v: unknown): string[] {
  return Array.isArray(v) ? v.map((x) => String(x)) : []
}

function collectNodes(node: ProposalNode, out: Map<string, ProposalNode>): void {
  const name = String(node.name ?? '').trim()
  if (name && !out.has(name)) out.set(name, node)
  for (const child of (node.children as ProposalNode[] | undefined) ?? []) collectNodes(child, out)
}

function toProfileBody(projectId: string, node: ProposalNode): AgentProfileCreate {
  return {
    project_id: projectId,
    name: String(node.name ?? '').trim(),
    role: String(node.role ?? ''),
    system_prompt: node.system_prompt ? String(node.system_prompt) : null,
    provider_id: null,
    model: null,
    max_steps: null,
    can_spawn: Boolean(node.can_spawn) || strList(node.children).length > 0,
    gated_tools: strList(node.gated_tools),
    skills: strList(node.skills),
  }
}

/** Create agents + team from a proposal. Returns an undo function. */
export async function applyTreeProposal(
  projectId: string,
  team: { name?: unknown; description?: unknown; root?: unknown },
): Promise<() => Promise<void>> {
  const root = (team.root ?? {}) as ProposalNode
  const unique = new Map<string, ProposalNode>()
  collectNodes(root, unique)

  const idByName = new Map<string, string>()
  const createdAgentIds: string[] = []
  let existing: Awaited<ReturnType<typeof listAgents>> | null = null

  for (const [name, node] of unique) {
    try {
      const agent = await createAgent(toProfileBody(projectId, node))
      idByName.set(name, agent.id)
      createdAgentIds.push(agent.id)
    } catch {
      // Name already exists (or another error) — reuse the existing agent so
      // the tree still wires up; we won't delete it on revert.
      existing = existing ?? (await listAgents(projectId))
      const match = existing.find((a) => a.name === name)
      if (!match) throw new Error(`could not create or find agent "${name}"`)
      idByName.set(name, match.id)
    }
  }

  const toTeamNode = (node: ProposalNode): TeamNode => ({
    profile_id: idByName.get(String(node.name ?? '').trim()) ?? '',
    children: ((node.children as ProposalNode[] | undefined) ?? []).map(toTeamNode),
  })

  const created = await createTeam({
    project_id: projectId,
    name: String(team.name ?? 'New team'),
    description: String(team.description ?? ''),
    root: toTeamNode(root),
  })

  return async () => {
    await deleteTeam(created.id)
    for (const id of createdAgentIds) {
      await deleteAgent(id).catch(() => {})
    }
  }
}
