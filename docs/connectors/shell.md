# Shell

The first-party connector that runs commands: builds, tests, git, scripts, package managers.

It is the most capable and the most dangerous thing in Gantry. A command runs as the user, with
the user's privileges, and once it is running Gantry cannot constrain what it does. Everything
below follows from taking that seriously rather than pretending otherwise.

Status: **built 2026-09-08** — both tools, the classifier, the environment capture, the caps,
the deadline and the process-tree kill, with the permission matrix of §8 holding through the
engine. What is not built is the interface half (§10: the command row, the drawer with colour and
the decision trail) and the guardrail floor of §8, which arrive with the rest of M7. Three
decisions changed while building; each is marked **revised** where it stands. The security
boundary in `docs/connectors/filesystem.md` §4 does the heavy lifting for paths, and this
document does not repeat it. Replaces the paragraph in `docs/plan/03-connector-system.md` §5.

---

## 1. Decisions

| # | Decision | Why |
|---|----------|-----|
| D1 | **No window ever appears on screen.** Not a console, not a terminal, not a flash. | Olav's explicit requirement. It is also the difference between an app and a script: a window appearing when the model runs `git status` is unacceptable, and on Windows it is the default unless you stop it (§3). |
| D2 | **The environment is captured once, at startup, from the login shell.** Individual commands run in a shell that does not re-read the user's startup files. | The user gets their real `PATH` and their toolchain managers. Gantry does not re-execute arbitrary user startup code on every command, which is both slow and the thing that would otherwise make the classifier meaningless (§6). |
| D3 | **Standard input is closed. There is no terminal emulation in the first version.** | A command that asks a question gets end-of-file and exits, rather than hanging until it times out. Honest and predictable. See §9 for what this costs. |
| D4 | **The whole process tree is killed, always.** On cancel, on timeout, on app exit. | Killing only the shell leaves its children running. That is how a stopped test run keeps holding a port. |
| D5 | **A non-zero exit code is a result, not an error.** | A failing test suite is exactly what the model asked to see. Only a command that could not be started is a tool error. Conflating the two teaches the model to treat real output as a malfunction. |
| D6 | **Output is capped, and the model gets the beginning and the end with the middle elided and counted.** | Errors are at the end; the command and its early context are at the start. The middle of a build log is the least useful part, and a model handed two megabytes of it learns nothing. Nothing is ever cut silently. |
| D7 | **The classifier proves a command string is read-only. It does not prove execution is.** Stated plainly in the interface. | §6. This is a real limit, and a guarantee that overstates itself is worse than one that does not. |
| D8 | **The working directory is an attached folder while the chat has one, and the home folder when it has none. What the command then does is not constrained.** | §7. The scope boundary is real for the connectors that go through it and does not extend to a subprocess. Saying so is the only honest position. *Corrected 2026-09-10*: refusing to run at all without a folder was a rule with nothing behind it — a question about the machine (`lscpu`, `free -h`) is not about a project, and the command could `cd` anywhere in any case. |
| D9 | **Gantry's own secrets are never placed in a command's environment.** | Provider keys and connector credentials have no business in a subprocess. If a command needs a token the user puts it there themselves. |
| D10 | **Long-running and background processes are deferred**, and the tool says so when a command hits the ceiling. | §9. Dev servers are a real use, and doing them properly needs machinery the first version does not have. |

## 2. The tools

Two, and the second exists only to stop the first.

### `run_command`

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `command` | string | required | A single command line, as the user would type it |
| `cwd` | string | the chat's primary folder, or the home folder when none is attached | Must resolve inside an attached folder while the chat has one |
| `timeout_ms` | integer | 120000 | Maximum 600000 |
| `env` | map | — | Extra variables for this command only |

Returns: exit code, stdout, stderr, duration, the resolved working directory, whether output was
truncated and by how much, whether it was killed, and whether the classifier judged it read-only.

Tier is `execute`, except when the classifier proves the command read-only, in which case it is
`read` for permission purposes. That single fact is what makes Plan mode usable.

### `kill_command`

| Parameter | Type | Notes |
|-----------|------|-------|
| `call_id` | string | The call to stop |

Tier `write`. Kills the tree (D4). Mostly the user presses Stop and this never gets used, but a
model that started something it now knows is wrong should be able to stop it without asking a
person to intervene.

## 3. Running a command without a window

This is the part most likely to be got wrong, so it is spelled out.

**Windows.** A graphical application spawning a console program pops a console window by default.
The process must be created with the flag that suppresses it. Two related mistakes to avoid: do
not detach the process, because that severs the pipes that carry its output; and never route a
command through the shell's own launcher verb, which exists specifically to open a new window.
Get this wrong and a black rectangle flashes on screen every time the model runs anything, which
is exactly what D1 forbids.

Also on Windows, the process is created in its own group and, better, attached to a job object,
so that killing it kills everything it started. Signalling a process tree on Windows is
otherwise unreliable.

**macOS and Linux.** Spawning a child process does not open a window, so D1 is satisfied by
default. The child is put in its own process group so the group can be signalled as a unit.

The one case no platform can prevent is a command that itself launches a graphical program.
Nothing stops `open`, `xdg-open` or a browser being invoked. That is the user's own command doing
what it says, not Gantry opening a terminal, and it is not worth crippling the connector to
prevent.

**Which shell** (**revised 2026-09-08**; this said the login shell, and building it on a machine
whose login shell is fish showed why that is wrong). Which shell *has the environment* and which
shell *runs the command* are two questions, and D2 only answers the first.

Models write POSIX command lines — `a && b`, `2>&1`, `VAR=x cmd` — and every one of those is a
syntax error in fish. A user whose login shell is fish, nushell or xonsh would watch almost every
command fail on grammar rather than on merit, and the failures would look like the model's fault.
So: the login shell is asked for its environment (D2, unchanged), and commands run in **bash where
it exists, `/bin/sh` otherwise**. On Windows, PowerShell 7 when present, falling back to Windows
PowerShell, both with `-NoProfile -NonInteractive`.

The shell that ran is reported in the result, and so is the login shell that supplied the
environment, because "it worked in my terminal" and "it worked in Gantry" differing by shell is
otherwise a mystery — and now they can differ by two shells.

**The environment.** Resolved once at startup by asking the login shell what it has, because a
graphical application on macOS otherwise inherits a nearly empty `PATH` and cannot find anything
the user installed. That captured environment, plus any `env` the call supplies, is what a
command gets. Nothing else, and never a Gantry secret (D9).

## 4. Output

Streamed line by line to the activity feed as it arrives, so a long build is visibly alive.

| Limit | Value |
|-------|-------|
| Live window in the interface | 400 lines, with the full log a click away |
| Captured per stream | 2 MB, after which capture stops and says so |
| Returned to the model | The head and the tail, with the elided middle counted (D6) |

Standard output and standard error are captured separately and stay distinguishable, because
"this failed" and "this printed a warning" are different things and interleaving them destroys
the distinction. The full log goes to a blob so the drawer can show all of it without the model
paying for it.

Terminal escape sequences are preserved in what is stored, so the drawer can render colour, and
stripped from what the model sees, where they are noise. Note that with no terminal attached most
tools will not emit colour anyway, which is a small loss and part of the price of D3.

## 5. Timeouts and cancellation

The default is two minutes and the ceiling is ten. On timeout the tree is killed (D4) and the
result says so explicitly, along with whatever output arrived before the end, because a test run
that hung after printing the failure is still telling you which test hung.

Cancelling a turn kills any running command the same way. The killed result is recorded rather
than discarded, so the transcript stays complete and can be replayed to any provider.

A command that hits the ceiling gets a specific message saying it was too long-running rather
than that it failed, and pointing at the deferred capability in §9. A model that reads "timed
out" will retry the same thing; a model that reads "this looks like a long-running process, which
this tool cannot host" will do something else.

## 6. The classifier, and what it is worth

Before a command runs it is classified. If every part of it is provably read-only, it is treated
as a `read` tool for permission purposes, which is what lets Plan mode allow `git status` while
refusing everything else.

The method: split the command line into segments on the operators that chain commands, and
require every segment's program to be on a small allowlist of things that observe without
changing anything, with no redirection, no privilege escalation, and no command substitution
anywhere. Anything not proven read-only is `execute`. The list is short and deliberately
conservative: file and directory inspection, the read-only subcommands of the common version
control and package tools, and a handful of system queries.

**A call that sets environment variables is never proven read-only** (added 2026-09-08, after an
adversarial audit of the built connector demonstrated all three of these). The classifier reads
the command *string*, and an environment decides what that string resolves to: `PATH` picks which
`ls` runs, `BASH_ENV` names a file the shell sources before it, and an exported shell function
replaces the command outright. Each turns a proven-read-only `ls` into arbitrary code that
Auto-edit would have run without asking. So the verdict does not survive an `env`, in the
connector's own result and in the permission engine, which apply the same rule for the same
reason.

**What this actually guarantees, and what it does not.** It proves a property of the *string*.
It does not prove a property of the *execution*, because a shell can be made to resolve a name to
something other than the program you expect. This is why D2 matters: by running commands in a
shell that does not read the user's startup files, the gap between "the string names a read-only
program" and "a read-only program ran" is closed for the ordinary case. It is not closed for a
program on the allowlist that has been replaced on disk, and that is a limit worth stating rather
than papering over.

The interface says this in plain words. "Gantry checked that this command only reads" is a
defensible claim. "This command cannot change anything" is not, and should never appear.

The allowlist lives in `gantry-core/src/command.rs` today, next to the parsing that uses it, and
moves to the guardrails file with the rest of M7 so it can be inspected and extended without a
release. It also parts company with `docs/plan/03-connector-system.md` §5 in one place: that
paragraph allows a bare "version flag" on anything, and `./deploy.sh --version` runs `deploy.sh`.
Version flags are allowed only for a listed set of toolchain programs, and a program named by
path is never proven at all.

**The wrapper hole, closed rather than noted.** A program that runs another program — `env`,
`nice`, `xargs`, `timeout`, `nohup`, `time` — cannot be on the allowlist as itself, or
`env rm -rf /` would pass by naming `env`. Each is unwrapped, its own options and any `VAR=value`
prefixes skipped, and whatever it was going to run is classified instead. `sudo`, `doas`, `su`,
`chroot` and `setsid` are refused outright rather than unwrapped: what follows them is not the
user's own privileges.

## 7. Scope, honestly

The working directory must resolve inside an attached folder, using the same containment
machinery as the filesystem connector. A chat with no folder attached runs in the user's home
folder, and there names any directory that exists: with nothing attached there is no boundary to
hold, and pretending otherwise would only cost the user the answer to a question about their own
machine.

That is the entire extent of it. Once a command is running it can touch anything the user can
touch, in any directory, over the network. Gantry does not sandbox it, and the first version does
not attempt to.

This is stated in the connector's risk notes, in the install dialog, and in the permission card,
because a user who believes the shell is confined to their project folder has been misled by
omission. The protections that actually apply are the permission mode, the guardrails, and the
fact that every command is shown before it runs.

## 8. Permissions and guardrails

| Situation | Manual | Auto-edit | Plan | Auto unguarded | Auto guarded |
|-----------|--------|-----------|------|----------------|--------------|
| Classified read-only | Ask | Allow | Ask | Allow | Allow |
| Everything else | Ask | Ask | **Refused** | Allow | Judged |

Never `parallel_safe`. Two commands from one batch running at once in the same directory is a
race the model did not intend and cannot reason about.

The permission card shows the command verbatim, the working directory, and whether the classifier
cleared it. Verbatim matters: a summarised or reformatted command is a command the user did not
actually approve.

Grants can be scoped to a command prefix, so "allow `npm test` for this chat" is one decision
rather than twenty. The prefix matches on the parsed program and its leading arguments, not on
raw text, so a grant cannot be widened by appending something after a semicolon.

The guardrail floor still applies in every mode including unguarded Auto: the small list of
catastrophic patterns in `desktop/assets/guardrails/defaults.toml`. Recursive deletion of a root
or a home directory, force-pushing to a default branch, writing to a raw device, piping a
download straight into a shell. These prompt even when nothing else does, and the list is
editable and can be emptied by a user who means it.

## 9. What the first version cannot do

Stated here because a builder needs to know the edges, and because the tool's own failure
messages point at them.

**No terminal emulation.** Standard input is closed (D3), so anything that expects a person
interacting with it will not work: an interactive rebase, a password prompt, a scaffolding tool
that asks questions, a pager. The failure is fast and clear rather than a hang, and the message
says the command appears to want input.

**No background processes.** A development server, a file watcher, a long build: all of these hit
the ten-minute ceiling and are killed. This is the most obvious gap and the one most worth
closing next. Doing it properly means starting a process detached from the turn, giving it an
identity that survives, letting the model read its accumulated output without blocking, and
stopping it reliably at the end of the session. That is a real amount of machinery.

**No sandboxing.** §7.

## 10. What the user sees

A command row shows the command and its working directory, then the last lines of output
scrolling as it runs, with elapsed time and a Stop button. When it finishes the row collapses to
the command, its exit code and its duration.

The drawer holds the full log with colour, both streams distinguishable, the resolved shell, the
environment variables that were added for this call by name, and the decision trail: who allowed
this and on what basis.

Exit code zero is unremarkable and should look it. A non-zero exit is marked but not alarming,
because it is frequently the expected answer. A killed or timed-out command is visually distinct
from both, since it is the one case where the output is incomplete.

## 11. When it fails

| Condition | What the model is told |
|-----------|------------------------|
| Ran, exited non-zero | The exit code and both streams. Not an error (D5) |
| Working directory outside the attached folders | Which folder would need adding, as the filesystem connector does |
| No folder attached, and no home folder either | That there is nowhere to run, and how to attach one |
| Program not found | That, plus a note that the environment came from the login shell, which is the usual cause |
| Timed out | That it exceeded the limit, that it was killed, the output so far, and that long-running processes are not supported |
| Killed by the user | That, plainly |
| Refused by Plan mode | That it was not proven read-only, and what the classifier objected to |
| Refused by a guardrail | Which pattern matched |
| Wants input | That standard input is closed and the command appears to be waiting for a person |
| Output truncated | How much was captured and how much was dropped |

## 12. Manifest

| Field | Value |
|-------|-------|
| `risk.network` | `internet`. A command can do anything, including reach the network |
| `risk.local_system` | `execute` |
| `risk.default_tool_tier` | `execute` |
| `risk.notes` | That commands run as the user with the user's privileges, where the working directory comes from and that the command's behaviour is not constrained, and that no terminal window is ever opened |
| `tools` | `tools_generated: true`, as the other native connectors do (**revised**): the code defines `run_command` and `kill_command`, neither `parallel_safe`, `run_command` with `plan_mode: classify`, and the card reads them from there so it cannot describe a tool that does not exist |
| `prompt.system_addendum` | Prefer the filesystem and editor tools over shell equivalents, because their results are structured and their changes are revertible; standard input is closed, so do not run interactive commands; long-running processes are not supported |

## 13. Testing

| Layer | How |
|-------|-----|
| The classifier | A fixture corpus of command lines with expected verdicts, including the ones designed to slip past: chained operators, substitution, redirection, unusual quoting, and an allowlisted program with dangerous arguments. Pure function, no processes |
| Running | Against small scripts committed as fixtures that exit with known codes, print known output to each stream, and sleep. No dependency on any tool being installed |
| Timeout and kill | A script that spawns a child and ignores termination, asserting the whole tree is gone afterwards. This is the test that catches the D4 mistake |
| Output caps | A script that prints far more than the cap, asserting the head and tail survive and the elision is counted correctly |
| No window | Cannot be asserted from a test. It goes on the manual release checklist for Windows, where it is the one platform that gets it wrong by default |
| The shell that runs commands | That it is bash or `sh` whatever the developer's login shell is — the test that would have caught the fish mistake before a user did |
| Scope | That a working directory outside the attached folders is refused, reusing the filesystem corpus; and that a chat with no folder runs in the home folder |

## 14. Changes this forces elsewhere

| Document | Change |
|----------|--------|
| `03-connector-system.md` §5 | The `shell` paragraph is replaced by a pointer here. The `shell` parameter is dropped from `run_command`; the shell is chosen by platform and reported, not selected by the model |
| `09-roadmap.md` M7 | Unchanged in scope, but the no-window requirement belongs on the M7 checklist and on the release checklist, since it is platform-specific and invisible in tests |
| `desktop/assets/guardrails/defaults.toml` | Gains the read-only allowlist alongside the hard-deny and always-confirm lists |

## 15. Open questions

1. **Background processes.** The clearest gap. Worth deciding early whether it lands right after
   M7 or waits, because the answer changes how the runner is structured.
2. **The read-only allowlist's contents.** Short and conservative to begin with, extended from
   real use rather than guessed at now.
3. ~~**Whether `cwd` should default to the chat's primary folder or be required.**~~ *Answered
   2026-09-08: it defaults to the chat's first folder, and to the home folder when the chat has
   no folder at all (2026-09-10).* A model made to name the folder every
   time names it wrongly — and an audit trail of a folder the model guessed is worth less than
   one of the folder the user attached. The resolved directory is in the result either way, so
   nothing is hidden by defaulting.
