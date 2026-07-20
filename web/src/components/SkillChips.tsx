import type { Skill } from '../api/types'

export function SkillChips({
  skills,
  selected,
  onToggle,
  hint = true,
}: {
  skills: Skill[]
  selected: Set<string>
  onToggle: (name: string) => void
  hint?: boolean
}) {
  if (skills.length === 0) return null
  return (
    <div className="flex flex-wrap items-center gap-1.5" aria-label="Skills">
      <span className="text-xs text-zinc-500">skills</span>
      {skills.map((skill) => (
        <button
          key={skill.name}
          type="button"
          title={skill.description}
          onClick={() => onToggle(skill.name)}
          className={`rounded-full border px-2.5 py-0.5 font-mono text-xs transition ${
            selected.has(skill.name)
              ? 'border-amber-600 bg-amber-950/60 text-amber-300'
              : 'border-zinc-800 text-zinc-500 hover:border-zinc-600 hover:text-zinc-300'
          }`}
        >
          {skill.name}
        </button>
      ))}
      {hint && (
        <span className="text-[10px] text-zinc-600">
          (unselected skills still auto-attach when the goal matches)
        </span>
      )}
    </div>
  )
}
