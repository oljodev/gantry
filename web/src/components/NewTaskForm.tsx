import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { createTask } from '../api/client'

const field =
  'w-full rounded-md border border-zinc-800 bg-zinc-900 px-3 py-1.5 text-sm ' +
  'placeholder:text-zinc-600 focus:border-amber-600 focus:outline-none'

export function NewTaskForm() {
  const navigate = useNavigate()
  const [goal, setGoal] = useState('')
  const [repoUrl, setRepoUrl] = useState('')
  const [model, setModel] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!goal.trim()) return
    setBusy(true)
    setError(null)
    try {
      const task = await createTask({
        goal: goal.trim(),
        repo_url: repoUrl.trim() || undefined,
        model: model.trim() || undefined,
      })
      navigate(`/tasks/${task.id}`)
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  return (
    <form
      onSubmit={submit}
      className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4"
      aria-label="New task"
    >
      <h2 className="mb-3 text-sm font-semibold text-zinc-400">Launch a task</h2>
      <div className="flex flex-col gap-2">
        <textarea
          className={`${field} min-h-16 font-mono`}
          placeholder="Goal — what should the agent do?"
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          required
        />
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          <input
            className={`${field} font-mono`}
            placeholder="repo_url (optional, e.g. git@github.com:org/repo.git)"
            value={repoUrl}
            onChange={(e) => setRepoUrl(e.target.value)}
          />
          <input
            className={`${field} font-mono`}
            placeholder="model (optional, defaults to server setting)"
            value={model}
            onChange={(e) => setModel(e.target.value)}
          />
        </div>
        <div className="flex items-center gap-3">
          <button
            type="submit"
            disabled={busy || !goal.trim()}
            className="rounded-md bg-amber-600 px-4 py-1.5 text-sm font-semibold text-zinc-950 transition hover:bg-amber-500 disabled:opacity-40"
          >
            {busy ? 'Enqueuing…' : 'Enqueue'}
          </button>
          {error && <span className="text-xs text-red-400">{error}</span>}
        </div>
      </div>
    </form>
  )
}
