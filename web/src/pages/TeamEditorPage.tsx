import { useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { TriangleAlert } from 'lucide-react'
import { createTeam, getTeam, listAgents, updateTeam } from '../api/client'
import type { AgentProfile } from '../api/types'
import { field, primaryButton, secondaryButton } from '../components/forms'
import {
  addChild,
  emptyNode,
  flatten,
  fromApi,
  isComplete,
  removeNode,
  reparent,
  setProfile,
  toApi,
  type EditNode,
  type FlatNode,
} from '../lib/teamTree'

export function TeamEditorPage() {
  const { teamId } = useParams<{ teamId: string }>()
  const navigate = useNavigate()
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const [root, setRoot] = useState<EditNode>(() => emptyNode())
  const [profiles, setProfiles] = useState<AgentProfile[]>([])
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loaded, setLoaded] = useState(!teamId)

  useEffect(() => {
    document.title = teamId ? 'Gantry — edit team' : 'Gantry — new team'
    listAgents().then(setProfiles).catch(console.error)
    if (teamId) {
      getTeam(teamId)
        .then((team) => {
          setName(team.name)
          setDescription(team.description)
          setRoot(fromApi(team.root))
          setLoaded(true)
        })
        .catch((err) => setError(String(err)))
    }
  }, [teamId])

  const rows = useMemo(() => flatten(root), [root])
  const byId = useMemo(() => new Map(profiles.map((p) => [p.id, p])), [profiles])

  const save = async () => {
    setBusy(true)
    setError(null)
    try {
      const body = { name: name.trim(), description: description.trim(), root: toApi(root) }
      if (teamId) await updateTeam(teamId, body)
      else await createTeam(body)
      navigate('/teams')
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  if (!loaded && !error) return <p className="text-sm text-zinc-600">loading…</p>

  return (
    <div className="flex max-w-3xl flex-col gap-4">
      <h1 className="text-lg font-semibold tracking-tight">
        {teamId ? 'Edit team' : 'New team'}
      </h1>
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        <input
          className={field}
          placeholder="team name"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <input
          className={field}
          placeholder="description (optional)"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
        />
      </div>

      <div className="flex flex-col gap-1.5">
        <div className="flex items-baseline gap-2">
          <h2 className="text-sm font-semibold text-zinc-400">Structure</h2>
          <span className="text-xs text-zinc-600">
            root delegates downward — parents need "may delegate" enabled
          </span>
        </div>
        {rows.map((row) => (
          <TreeNodeCard
            key={row.node.key}
            row={row}
            rows={rows}
            profiles={profiles}
            byId={byId}
            onSetProfile={(key, id) => setRoot((r) => setProfile(r, key, id))}
            onAddChild={(key) => setRoot((r) => addChild(r, key))}
            onRemove={(key) => setRoot((r) => removeNode(r, key) ?? r)}
            onReparent={(key, parent) => setRoot((r) => reparent(r, key, parent))}
          />
        ))}
      </div>

      {profiles.length === 0 && (
        <p className="text-xs text-amber-400">
          You have no agents yet —{' '}
          <Link to="/agents" className="underline">
            create some agents
          </Link>{' '}
          to fill the tree.
        </p>
      )}

      <div className="flex items-center gap-2">
        <button
          onClick={() => void save()}
          disabled={busy || !name.trim() || !isComplete(root)}
          className={primaryButton}
        >
          {teamId ? 'Save team' : 'Create team'}
        </button>
        <Link to="/teams" className={secondaryButton}>
          Cancel
        </Link>
        {!isComplete(root) && (
          <span className="text-xs text-zinc-600">assign an agent to every node to save</span>
        )}
        {error && <span className="max-w-md truncate text-xs text-red-400">{error}</span>}
      </div>
    </div>
  )
}

function TreeNodeCard({
  row,
  rows,
  profiles,
  byId,
  onSetProfile,
  onAddChild,
  onRemove,
  onReparent,
}: {
  row: FlatNode
  rows: FlatNode[]
  profiles: AgentProfile[]
  byId: Map<string, AgentProfile>
  onSetProfile: (key: string, profileId: string) => void
  onAddChild: (key: string) => void
  onRemove: (key: string) => void
  onReparent: (key: string, newParentKey: string) => void
}) {
  const { node, depth } = row
  const profile = node.profile_id ? byId.get(node.profile_id) : undefined
  const isRoot = depth === 0
  // Valid new parents: any node that is not this one and not in its subtree.
  const subtreeKeys = new Set(flatten(node).map((f) => f.node.key))
  const parentOptions = rows.filter((r) => !subtreeKeys.has(r.node.key))

  return (
    <div
      className="flex flex-wrap items-center gap-2 rounded-lg border border-zinc-800 bg-zinc-900/40 px-3 py-2"
      style={{ marginLeft: depth * 24 }}
    >
      {!isRoot && (
        <span className="text-zinc-700" aria-hidden>
          └
        </span>
      )}
      <select
        className="rounded-md border border-zinc-800 bg-zinc-900 px-2 py-1 text-sm focus:border-amber-600 focus:outline-none"
        value={node.profile_id ?? ''}
        onChange={(e) => onSetProfile(node.key, e.target.value)}
        aria-label={isRoot ? 'Root agent' : 'Agent'}
      >
        <option value="" disabled>
          {isRoot ? 'choose root agent…' : 'choose agent…'}
        </option>
        {profiles.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
            {p.role ? ` — ${p.role}` : ''}
          </option>
        ))}
      </select>
      {profile && (
        <span className="text-xs text-zinc-600">
          {profile.can_spawn ? 'delegates' : 'works alone'}
          {node.children.length > 0 && !profile.can_spawn && (
            <span className="ml-1 inline-flex items-center gap-1 align-middle text-amber-400">
              <TriangleAlert className="h-3 w-3" aria-hidden />
              needs "may delegate"
            </span>
          )}
        </span>
      )}
      <span className="grow" />
      <button
        onClick={() => onAddChild(node.key)}
        className="rounded border border-zinc-700 px-2 py-0.5 text-[11px] text-zinc-300 transition hover:bg-zinc-800"
        title="Add a subordinate agent"
      >
        + child
      </button>
      {!isRoot && (
        <>
          <select
            className="rounded border border-zinc-800 bg-zinc-900 px-1.5 py-0.5 text-[11px] text-zinc-400 focus:outline-none"
            value=""
            onChange={(e) => e.target.value && onReparent(node.key, e.target.value)}
            aria-label="Move under"
          >
            <option value="">move under…</option>
            {parentOptions.map((option) => (
              <option key={option.node.key} value={option.node.key}>
                {option.node.profile_id
                  ? (byId.get(option.node.profile_id)?.name ?? 'unassigned')
                  : 'unassigned'}
                {option.depth === 0 ? ' (root)' : ''}
              </option>
            ))}
          </select>
          <button
            onClick={() => onRemove(node.key)}
            className="rounded border border-red-900 px-2 py-0.5 text-[11px] text-red-300 transition hover:bg-red-950"
            title="Remove this node and its subtree"
          >
            ×
          </button>
        </>
      )}
    </div>
  )
}
