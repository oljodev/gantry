---
name: code-review
description: Review a change the way a careful colleague would: correctness first, then the failure the author cannot see, then the cost of maintaining it, with every comment naming a concrete scenario rather than a preference. Use when reviewing code, a diff, a pull request or a patch, when asked what is wrong with a piece of code, or before merging.
license: FSL-1.1-ALv2
metadata:
  gantry-triggers: "code review, review, pull request, diff, patch, merge, critique, look over"
  gantry-always: "false"
  gantry-version: "1"
  author: Gantry
---

# Reviewing a change

## When to use

Someone asks for a review of a diff, a file, a pull request or a patch — or asks what is wrong
with code that already works. Not for writing new code, and not for style questions a formatter
already answers.

## Order

Review in this order and stop at the first level that has something serious. A review that opens
with naming is a review the author will not finish reading.

1. **Correctness.** Does it do what it claims, for the inputs it will actually receive? Look for
   the off-by-one, the unhandled `None`, the error swallowed by a bare `catch`, the integer that
   can overflow, the `await` that is missing.
2. **The edges the author could not see.** Empty input, one element, the maximum, concurrent
   callers, a partial failure halfway through, a retry that is not idempotent, a cancellation
   between two writes.
3. **Security and data.** Untrusted input reaching a query, a path, a shell or a template. A
   secret in a log line. A permission check that happens after the side effect.
4. **The cost of keeping it.** Will the next person understand why this is here? Is there a
   simpler shape with the same behaviour? Is a new dependency worth what it buys?
5. **Consistency.** Does it read like the code around it — same idioms, same naming, same error
   handling? A correct change in a foreign style is still a cost.

## How to write a comment

Every comment names a **scenario**, not a preference:

- Weak: "this should use a map".
- Better: "with 10,000 rows this is quadratic — the inner `find` runs per row. A map keyed by
  `id` built once makes it linear."

- Weak: "error handling could be better".
- Better: "if `parse` fails here the function returns `Ok(default)`, so a malformed config file
  starts the app with silent defaults instead of refusing. Was that intended?"

Say what you verified and what you did not. "I did not run it" is useful information.

## Pitfalls

- **Reviewing the author.** The change is the subject.
- **Inventing a failure.** Before writing a bug, construct the input that triggers it. If you
  cannot, say "I think, but I could not construct it" rather than asserting it.
- **A wall of equal-weight comments.** Rank them: what blocks the merge, what to fix now, what
  is a note. Three ranked comments beat twenty flat ones.
- **Missing the absent thing.** The most common real defect is something that is not in the
  diff: the test that was not added, the caller that was not updated, the migration that has no
  rollback, the error path that was never written.
