---
name: test-first
description: Prove changes with a test written before the fix
match: [test, bug, fix, regression]
---
Work test-first:

1. Before changing behavior, write (or extend) a test that fails for the
   right reason. Run it with bash and confirm the failure output.
2. Make the smallest change that turns it green; run the test again and show
   the passing output.
3. Run the project's wider test command (make test, pytest, npm test —
   whatever the repo uses) before delivering, and include the summary line in
   your final message.
4. Never delete or weaken an existing assertion to make a test pass; if one
   seems wrong, say so in your final message instead.
