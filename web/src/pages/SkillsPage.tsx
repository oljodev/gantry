// Skills authoring: create, edit, and delete the project's skills. A skill is
// markdown appended to an agent's system prompt, auto-selected when its match
// keywords appear in a run's goal.

import { useCallback, useEffect, useState } from 'react'
import { Pencil, Plus, Sparkles, Trash2, Wand2 } from 'lucide-react'
import { createSkill, deleteSkill, listSkills, updateSkill } from '../api/client'
import type { Skill } from '../api/types'
import { field, primaryButton, secondaryButton } from '../components/forms'
import { Markdown } from '../components/Markdown'
import { useProjectId } from '../lib/project'
import { useCopilot } from '../state/CopilotProvider'

type Prefill = { name?: string; description?: string; match?: string[]; body?: string }

export function SkillsPage() {
  const projectId = useProjectId()
  const { open, close } = useCopilot()
  const [skills, setSkills] = useState<Skill[] | null>(null)
  const [editing, setEditing] = useState<Skill | 'new' | null>(null)
  const [prefill, setPrefill] = useState<Prefill | null>(null)
  const [error, setError] = useState<string | null>(null)

  const reload = useCallback(() => {
    listSkills(projectId).then(setSkills).catch(console.error)
  }, [projectId])

  useEffect(() => {
    document.title = 'Gantry — skills'
    reload()
  }, [reload])

  // Close the co-pilot when leaving the page — its apply handler edits this
  // page's form state.
  useEffect(() => close, [close])

  const openCopilot = () =>
    open({
      kind: 'skill',
      title: 'Skill co-pilot',
      projectId,
      context: editing && editing !== 'new' ? JSON.stringify(editing) : '',
      onApply: async (proposal) => {
        const skill = (proposal.skill ?? {}) as Prefill
        setPrefill(skill)
        setEditing('new')
        return async () => {
          setEditing(null)
          setPrefill(null)
        }
      },
    })

  const remove = (skill: Skill) =>
    deleteSkill(skill.id)
      .then(() => {
        setError(null)
        reload()
      })
      .catch((err) => setError(String(err)))

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center gap-3">
        <div>
          <h1 className="text-lg font-semibold tracking-tight">Skills</h1>
          <p className="mt-1 text-sm text-zinc-500">
            Reusable instructions injected into an agent's prompt — automatically when a goal
            mentions a match keyword, or when picked explicitly.
          </p>
        </div>
        <span className="grow" />
        <button
          onClick={openCopilot}
          className={`flex items-center gap-1.5 ${secondaryButton}`}
        >
          <Wand2 className="h-4 w-4" aria-hidden />
          Co-pilot
        </button>
        <button
          onClick={() => {
            setPrefill(null)
            setEditing('new')
          }}
          className={`flex items-center gap-1.5 ${primaryButton}`}
        >
          <Plus className="h-4 w-4" aria-hidden />
          New skill
        </button>
      </div>
      {error && <p className="text-xs text-red-400">{error}</p>}

      {editing && (
        <SkillForm
          key={prefill ? 'prefilled' : editing === 'new' ? 'new' : editing.id}
          skill={editing === 'new' ? null : editing}
          initial={editing === 'new' ? prefill : null}
          projectId={projectId}
          onDone={() => {
            setEditing(null)
            setPrefill(null)
            reload()
          }}
          onCancel={() => setEditing(null)}
        />
      )}

      {skills === null ? (
        <p className="text-sm text-zinc-600">loading…</p>
      ) : skills.length === 0 ? (
        <div className="rounded-lg border border-dashed border-zinc-800 py-14 text-center">
          <Sparkles className="mx-auto h-7 w-7 text-zinc-600" aria-hidden />
          <p className="mt-3 text-sm text-zinc-500">No skills yet.</p>
        </div>
      ) : (
        <div className="grid gap-3 md:grid-cols-2">
          {skills.map((skill) => (
            <div
              key={skill.id}
              className="flex flex-col gap-2 rounded-lg border border-zinc-800 bg-zinc-900/40 p-4"
            >
              <div className="flex items-center gap-2">
                <Sparkles className="h-4 w-4 text-amber-400" aria-hidden />
                <h2 className="font-semibold">{skill.name}</h2>
                <span className="grow" />
                <button
                  onClick={() => setEditing(skill)}
                  aria-label="Edit"
                  className="text-zinc-500 transition hover:text-zinc-200"
                >
                  <Pencil className="h-3.5 w-3.5" aria-hidden />
                </button>
                <button
                  onClick={() => void remove(skill)}
                  aria-label="Delete"
                  className="text-zinc-500 transition hover:text-red-300"
                >
                  <Trash2 className="h-3.5 w-3.5" aria-hidden />
                </button>
              </div>
              {skill.description && <p className="text-sm text-zinc-400">{skill.description}</p>}
              {skill.match.length > 0 && (
                <div className="flex flex-wrap gap-1">
                  {skill.match.map((m) => (
                    <span
                      key={m}
                      className="rounded-full bg-zinc-800 px-2 py-0.5 text-[10px] text-zinc-400"
                    >
                      {m}
                    </span>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function SkillForm({
  skill,
  initial,
  projectId,
  onDone,
  onCancel,
}: {
  skill: Skill | null
  initial?: Prefill | null
  projectId: string
  onDone: () => void
  onCancel: () => void
}) {
  const [name, setName] = useState(skill?.name ?? initial?.name ?? '')
  const [description, setDescription] = useState(skill?.description ?? initial?.description ?? '')
  const [match, setMatch] = useState((skill?.match ?? initial?.match ?? []).join(', '))
  const [body, setBody] = useState(skill?.body ?? initial?.body ?? '')
  const [preview, setPreview] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    setBusy(true)
    setError(null)
    const payload = {
      project_id: projectId,
      name: name.trim(),
      description: description.trim(),
      match: match
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean),
      body,
    }
    try {
      if (skill) await updateSkill(skill.id, payload)
      else await createSkill(payload)
      onDone()
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  return (
    <form
      onSubmit={submit}
      className="flex flex-col gap-3 rounded-lg border border-zinc-800 bg-zinc-900/40 p-4"
    >
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="flex flex-col gap-1 text-xs text-zinc-500">
          Name
          <input className={field} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="flex flex-col gap-1 text-xs text-zinc-500">
          Match keywords (comma-separated)
          <input
            className={field}
            value={match}
            onChange={(e) => setMatch(e.target.value)}
            placeholder="commit, changelog, release"
          />
        </label>
      </div>
      <label className="flex flex-col gap-1 text-xs text-zinc-500">
        Description
        <input
          className={field}
          value={description}
          onChange={(e) => setDescription(e.target.value)}
        />
      </label>
      <div className="flex items-center gap-2 text-xs text-zinc-500">
        <span>Body (markdown, appended to the system prompt)</span>
        <span className="grow" />
        <button
          type="button"
          onClick={() => setPreview((p) => !p)}
          className="text-zinc-400 underline underline-offset-2 hover:text-zinc-200"
        >
          {preview ? 'edit' : 'preview'}
        </button>
      </div>
      {preview ? (
        <div className="min-h-40 rounded-md border border-zinc-800 bg-zinc-900 px-3 py-2">
          <Markdown>{body || '_nothing yet_'}</Markdown>
        </div>
      ) : (
        <textarea
          className={`${field} min-h-40 font-mono`}
          value={body}
          onChange={(e) => setBody(e.target.value)}
        />
      )}
      <div className="flex items-center gap-2">
        <button type="submit" disabled={busy || !name.trim()} className={primaryButton}>
          {skill ? 'Save skill' : 'Create skill'}
        </button>
        <button type="button" onClick={onCancel} className={secondaryButton}>
          Cancel
        </button>
        {error && <span className="text-xs text-red-400">{error}</span>}
      </div>
    </form>
  )
}
