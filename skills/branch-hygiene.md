---
name: branch-hygiene
description: Deliver clean, reviewable branches
match: [branch, push, deliver, pr, pull request]
---
Delivery discipline for your task branch:

- Deliver exclusively via git_commit_push — never raw `git push` through
  bash, and never touch branches other than your own task branch.
- Keep the working tree clean of build artifacts and scratch files before
  committing (`ls` and remove or .gitignore them; ask yourself whether each
  file belongs in review).
- Prefer one coherent commit per delivered unit of work; commit again rather
  than amending history.
- Your final message must name the branch and summarize what a reviewer will
  see on it.
