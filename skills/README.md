# Skills

Portable `SKILL.md` files injected into worker system prompts. Each `*.md`
file here (any nesting; READMEs are ignored) is one skill:

```markdown
---
name: my-skill
description: One line shown in the UI picker
match: [keyword, other keyword]
---
Instructions appended to the agent's system prompt.
```

Selection per task: the payload's explicit `skills` list, plus any skill
whose `match` keywords appear in the goal (disable with `auto_skills: false`).
Injection is durable: a `skill_injected` event pins the exact content the run
used, so editing a file here never corrupts old traces, and crash-resume
rebuilds the identical prompt. Set `GANTRY_SKILLS_ROOT` to use another
directory.
