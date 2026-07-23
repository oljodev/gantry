// Turn a co-pilot tree proposal into real agents + team wiring, scoped to the
// team the user has open. Applying edits that team IN PLACE (updating its
// agents and re-wiring its tree) rather than creating a new one — so proposing
// changes to an existing team never collides on the unique team name. Returns
// an undo function that restores the prior state.

import {
  createAgent,
  createTeam,
  deleteAgent,
  deleteTeam,
  listAgents,
  listProviders,
  updateAgent,
  updateTeam,
} from '../api/client'
import type {
  AgentProfile,
  AgentProfileCreate,
  Provider,
  Team,
  TeamNode,
  TeamNodeOut,
} from '../api/types'

interface ProposalNode {
  name?: unknown
  role?: unknown
  system_prompt?: unknown
  model?: unknown
  can_spawn?: unknown
  gated_tools?: unknown
  skills?: unknown
  children?: unknown
}

function strList(v: unknown): string[] {
  return Array.isArray(v) ? v.map((x) => String(x)) : []
}

/** Resolve the co-pilot's chosen model string to a provider in the library. */
function resolveProvider(
  model: string,
  providers: Provider[],
): { provider_id: string | null; model: string | null } {
  if (!model) return { provider_id: null, model: null }
  const exact = providers.find((p) => p.default_model === model)
  if (exact) return { provider_id: exact.id, model }
  const prefix = model.split('/')[0].toLowerCase()
  const byKind = providers.find(
    (p) => p.provider_type === prefix || p.name.toLowerCase() === prefix,
  )
  return { provider_id: byKind?.id ?? null, model }
}

function collectNodes(node: ProposalNode, out: Map<string, ProposalNode>): void {
  const name = String(node.name ?? '').trim()
  if (name && !out.has(name)) out.set(name, node)
  for (const child of (node.children as ProposalNode[] | undefined) ?? []) collectNodes(child, out)
}

function toProfileBody(
  projectId: string,
  node: ProposalNode,
  providers: Provider[],
  teamId?: string,
): AgentProfileCreate {
  const { provider_id, model } = resolveProvider(String(node.model ?? '').trim(), providers)
  return {
    project_id: projectId,
    team_id: teamId ?? null,
    name: String(node.name ?? '').trim(),
    role: String(node.role ?? ''),
    system_prompt: node.system_prompt ? String(node.system_prompt) : null,
    provider_id,
    model,
    // The co-pilot never caps steps unless the user asked (item 8).
    max_steps: null,
    can_spawn: Boolean(node.can_spawn) || strList(node.children).length > 0,
    gated_tools: strList(node.gated_tools),
    skills: strList(node.skills),
  }
}

/** The editable subset of a stored profile, for restoring it on revert. */
function profileToBody(p: AgentProfile): AgentProfileCreate {
  return {
    project_id: p.project_id,
    team_id: p.team_id,
    name: p.name,
    role: p.role,
    system_prompt: p.system_prompt,
    provider_id: p.provider_id,
    model: p.model,
    max_steps: p.max_steps,
    can_spawn: p.can_spawn,
    gated_tools: p.gated_tools,
    skills: p.skills,
  }
}

function outToWriteNode(node: TeamNodeOut): TeamNode {
  return {
    profile_id: node.profile.id,
    children: node.children.map(outToWriteNode),
  }
}

/** Render an existing team as architect-readable context for revision. */
export function teamToContext(team: Team): string {
  const shape = (node: TeamNodeOut): Record<string, unknown> => ({
    name: node.profile.name,
    role: node.profile.role,
    system_prompt: node.profile.system_prompt,
    model: node.profile.model,
    can_spawn: node.profile.can_spawn,
    gated_tools: node.profile.gated_tools,
    skills: node.profile.skills,
    children: node.children.map(shape),
  })
  return JSON.stringify(
    { name: team.name, description: team.description, root: shape(team.root) },
    null,
    2,
  )
}

/**
 * Apply a co-pilot tree proposal. When `existing` is given the proposal edits
 * that team in place; otherwise it creates a new team. Returns an undo function.
 */
export async function applyTreeProposal(
  projectId: string,
  proposed: { name?: unknown; description?: unknown; root?: unknown },
  existing?: Team,
): Promise<() => Promise<void>> {
  const root = (proposed.root ?? {}) as ProposalNode
  const unique = new Map<string, ProposalNode>()
  collectNodes(root, unique)

  // Look only at this team's own library (or the drafts for a new team) — each
  // team's agents are its own, so matching by name stays within the team.
  const [before, providers] = await Promise.all([
    existing
      ? listAgents(projectId, { teamId: existing.id })
      : listAgents(projectId, { unassigned: true }),
    listProviders(),
  ])
  const byName = new Map(before.map((a) => [a.name, a]))

  const idByName = new Map<string, string>()
  const createdAgentIds: string[] = []
  const updatedAgents: AgentProfileCreate[] = [] // prior bodies, to restore on revert

  for (const [name, node] of unique) {
    const body = toProfileBody(projectId, node, providers, existing?.id)
    const match = byName.get(name)
    if (match) {
      updatedAgents.push(profileToBody(match))
      await updateAgent(match.id, body)
      idByName.set(name, match.id)
    } else {
      const created = await createAgent(body)
      idByName.set(name, created.id)
      createdAgentIds.push(created.id)
    }
  }

  const toTeamNode = (node: ProposalNode): TeamNode => ({
    profile_id: idByName.get(String(node.name ?? '').trim()) ?? '',
    children: ((node.children as ProposalNode[] | undefined) ?? []).map(toTeamNode),
  })

  const write = {
    project_id: projectId,
    name: String(proposed.name ?? existing?.name ?? 'New team'),
    description: String(proposed.description ?? existing?.description ?? ''),
    root: toTeamNode(root),
  }

  let restoreTeam: () => Promise<void>
  if (existing) {
    const priorWrite = {
      project_id: projectId,
      name: existing.name,
      description: existing.description,
      root: outToWriteNode(existing.root),
    }
    await updateTeam(existing.id, write)
    restoreTeam = async () => {
      await updateTeam(existing.id, priorWrite)
    }
  } else {
    const created = await createTeam(write)
    restoreTeam = async () => {
      await deleteTeam(created.id)
    }
  }

  return async () => {
    // Restore the team wiring first so no agent we delete is still referenced.
    await restoreTeam()
    for (const body of updatedAgents) {
      const match = byName.get(body.name)
      if (match) await updateAgent(match.id, body).catch(() => {})
    }
    for (const id of createdAgentIds) {
      await deleteAgent(id).catch(() => {})
    }
  }
}
