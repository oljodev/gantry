---
name: commit-messages
description: Write a commit message that says what changed and why, in the imperative present tense, with a subject under 72 characters and a body that explains the reasoning rather than restating the diff. Use when committing, writing or rewriting a commit message, preparing a pull request description, or when the user mentions git, a commit, staging or a changelog entry.
license: FSL-1.1-ALv2
metadata:
  gantry-triggers: "commit, commit message, git, pull request, changelog, squash, amend"
  gantry-always: "false"
  gantry-version: "1"
  author: Gantry
---

# Commit messages

## When to use

Any time a change is being recorded: a commit, an amend, a squash, a pull-request description.
Not for release notes, which are written for users rather than for the next reader of the log.

## The shape

```
Subject in the imperative, under 72 characters

What the reader cannot see in the diff: why this change, what it replaces,
what was considered and rejected, and anything that will look wrong later
without an explanation.

Wrapped at 72 columns, blank line between paragraphs.
```

## Rules

1. **Imperative present.** "Add the retry", not "Added" or "Adds". The subject completes the
   sentence "applying this commit will …".
2. **The subject is a claim, not a category.** `fix: bug` says nothing. `Stop retrying a 401`
   says what the reader needs before deciding whether to keep reading.
3. **The body is for the why.** The diff already says what changed. The body says what problem
   it solves, what alternative was rejected and why, and which constraint made the obvious
   approach wrong. If there is nothing to say beyond the subject, write no body.
4. **One coherent change per commit.** If the body needs the word "also", consider two commits.
5. **Name the consequence.** Behaviour that changes for the user, a migration that runs, a
   setting whose default moved: say it in the body, in its own paragraph.
6. **No mechanical prefixes** unless the repository already uses them. Match the log you are
   writing into: read the last ten commits before writing the eleventh.

## Examples

Weak:

```
fix stuff in the auth module
```

Better:

```
Stop retrying a 401 as if it were a timeout

The retry wrapper treated every error from the token endpoint as transient,
so a wrong key produced three identical failures and a confusing sixty-second
wait before the user saw "invalid key". Only network errors and 5xx are
retried now; a 401 fails on the first attempt with the provider's own message.
```

## Pitfalls

- A body that paraphrases the diff line by line. If a reader can get it from `git show`, it does
  not belong in the message.
- "Minor fixes", "cleanup", "updates". They make `git log` unsearchable, which is the one thing
  the log is for.
- Writing the message before the change is finished; the last thing you learn is usually the
  thing worth writing down.
