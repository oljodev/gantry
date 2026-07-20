import { useEffect, useState } from 'react'
import { listSkills } from '../api/client'
import type { Skill } from '../api/types'

export function SkillsPage() {
  const [skills, setSkills] = useState<Skill[] | null>(null)

  useEffect(() => {
    document.title = 'Gantry — skills'
    listSkills().then(setSkills).catch(console.error)
  }, [])

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h1 className="text-lg font-semibold tracking-tight">Skills</h1>
        <p className="mt-1 text-sm text-zinc-500">
          Markdown playbooks injected into agent prompts — attach them explicitly at launch, or
          let goal keywords auto-match. Add more by dropping SKILL.md files into{' '}
          <code className="font-mono text-zinc-400">skills/</code>.
        </p>
      </div>
      {skills === null ? (
        <p className="text-sm text-zinc-600">loading…</p>
      ) : skills.length === 0 ? (
        <p className="rounded-lg border border-zinc-800 bg-zinc-900/30 px-4 py-10 text-center text-sm text-zinc-600">
          No skills found in the skills directory.
        </p>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
          {skills.map((skill) => (
            <div key={skill.name} className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
              <h2 className="font-mono text-sm font-semibold text-amber-300">{skill.name}</h2>
              <p className="mt-1 text-sm text-zinc-400">{skill.description}</p>
              {skill.match.length > 0 && (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {skill.match.map((pattern) => (
                    <span
                      key={pattern}
                      className="rounded-full bg-zinc-800 px-2 py-0.5 font-mono text-[10px] text-zinc-400"
                    >
                      {pattern}
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
