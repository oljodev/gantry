# Bundled skills

A skill is a playbook: Markdown that is added to the model's context when a message matches it,
and nothing else. Gantry never executes anything it finds in a skill folder and never grants a
skill any capability — see `docs/plan/12-skills-and-memory.md` §A1 for why that constraint is
the whole design.

Everything in this folder is embedded in the binary by `gantry-agent`'s `build.rs` and installed
on first run as `source: bundled`. A bundled skill can be switched off, pinned and read; it
cannot be edited or deleted, because it is not a file on the user's disk. A user who wants a
different version writes their own under another name, which shadows nothing and simply wins on
its own merits.

## The contract

- One folder per skill. **The folder name is the skill's name**, and the `name:` in the
  frontmatter must equal it. Lowercase letters, digits and single hyphens.
- `SKILL.md` is a valid [Agent Skills](https://agentskills.io) file: `---` frontmatter with
  `name` and `description`, then Markdown. Gantry's own fields live under `metadata` with a
  `gantry-` prefix, which the specification reserves for client extensions, so a skill written
  here still opens in Claude Code.
- `description` is the matcher's primary signal (12 §A4). Write what the skill does **and when
  to use it**, in the words a person would type.
- `metadata.gantry-triggers` is a comma-separated list of words and phrases that force a strong
  match. Keep it to terms that are genuinely about this skill.
- Optional `references/*.md` are loaded only when the model asks for one with
  `gantry__read_skill_file`. `scripts/`, `assets/` and anything that is not text do not belong
  here and are refused on import.
- Keep a body under about 500 lines. It is spent from the same context window as the
  conversation.

`cargo xtask validate-skills` checks all of this, and the build fails on a folder that breaks it.
