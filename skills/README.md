# Skills

Bundled skills: text-only playbooks the agent loads when they fit the task. The format is the
Agent Skills `SKILL.md` (agentskills.io) with Gantry's fields under `metadata`, exactly as
described in `docs/plan/12-skills-and-memory.md` §A2. Schema: `schemas/skill-frontmatter.schema.json`.

```
skills/<name>/
  SKILL.md            required · frontmatter (name, description, metadata.gantry-*) + Markdown body
  references/*.md     optional · text documents loaded only when the model asks for them
```

Rules: `name` equals the folder name; nothing executable, no scripts, no binary assets; body under
32 KB; references 64 KB each, at most 10. `gantry-agent`'s `build.rs` embeds every skill here and
`cargo xtask validate-skills` checks the frontmatter in CI. The starter set arrives with M12.
