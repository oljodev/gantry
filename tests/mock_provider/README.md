# Gantry Mock LLM Provider

A local, OpenAI-compatible provider for stress-testing Gantry's orchestration,
circuit breakers, and budget sentinels for **$0** in real token spend. It serves
`POST /v1/chat/completions` over SSE and picks a scripted scenario from the
request's `model` field.

Because the mock only scripts the model's *tool calls* — never the tool *results*,
which Gantry's real sandbox produces — it provokes the genuine breakers instead of
faking them (e.g. it tells the agent to `write_file` the same broken Python each
step; the real write-time AST check emits the recurring diagnostic that trips the
repair-wave breaker).

## Run it

```
python -m tests.mock_provider --port 8000
```

## Point Gantry at it

Model strings map to scenarios; LiteLLM routes an OpenAI-compatible base with the
`openai/` prefix and strips it before sending, so the mock sees `mock/happy-path`.

1. **A `LOCAL` provider (exercises the real provider path).** Create a `Provider`
   in the workspace with `provider_type=LOCAL`, `base_url=http://localhost:8000/v1`,
   any dummy key, `default_model=mock/happy-path`; launch a task/team with that
   `provider_id`. `providers.resolve_model` prefixes it to `openai/mock/...`.
2. **Env fallback (no provider row).** For a task without `provider_id` the worker
   uses the env-configured shared client:
   `export OPENAI_API_BASE=http://localhost:8000/v1` (LiteLLM's OpenAI base var;
   `OPENAI_BASE_URL` is also honored) and use models like `openai/mock/happy-path`.

## Scenarios

| Model | Behavior | What it verifies |
|---|---|---|
| `mock/happy-path` | read_file -> write_file (valid) -> final answer | Clean 2-step completion. |
| `mock/repair-loop` | writes the same broken Python every step | Repair-wave breaker escalates (`TaskStalled`) after 3 identical diagnostics. |
| `mock/budget-runaway` | streams an unbounded token wall + periodic usage, never finishes | Finite step-cap halt today; the mid-stream `$`-sentinel once that lands. |

### Phase 2 — chaos scenarios

| Model | Behavior | What it verifies |
|---|---|---|
| `mock/hallucinated-tool` | calls a non-existent tool every step | Unknown-tool diagnostic feeds the repair-wave breaker (`TaskStalled`). |
| `mock/passive-read-loop` | only ever reads, never edits | The passive-read breaker fails a leaf worker before the step cap. |
| `mock/git-conflict` | serves as the LLM conflict **resolver** (union-merges markers) | Two colliding branches are resolved, staging verifies + publishes. |
| `mock/missing-branch` | a worker "claiming success" | The `MissingBranch` gate blocks an integration with an undelivered branch (exercised at the merge layer). |
| `mock/rate-limit-429` | HTTP 429 for N requests, then success | Transient 429s are retried transparently (client-layer backoff); the run recovers. |
| `mock/corrupted-sse` | a malformed SSE chunk for N requests, then a clean stream | A corrupted stream raises and is retried, never a hard failure. |

`mock/rate-limit-429` and `mock/corrupted-sse` are transport-level (status codes /
malformed bodies), handled in `app.py` with a per-app request counter (see
`GET /counters`); the rest are token scenarios in `scenarios.py`.

### Phase 3 — real-world edge cases

| Model | Behavior | What it verifies |
|---|---|---|
| `mock/context-window-overflow` | a few reads, then 400 `context_length_exceeded`, then success | Gantry force-compacts the history and retries in the same step (a 400 is otherwise permanent). |
| `mock/lazy-leader-fast-exit` | leader reads one file, reports success, spawns nothing | The leader delivery gate rejects the premature empty exit. |
| `mock/partial-tool-json-truncation` | a tool call with cut-off JSON arguments | The parser emits a `MalformedToolCall` diagnostic (no crash); repeats trip the repair breaker. |
| `mock/worker-timeout-hang` | stops sending bytes after the opening frame | A bounded request timeout fails the call and the queue re-claims the task. |

These four ship with new `src/` safeguards: a per-request LLM timeout
(`llm_request_timeout_seconds`), a `MalformedToolCall` diagnostic in the loop,
in-loop context-overflow recovery (compact + retry), and the leader delivery gate.

The registry in `scenarios.py` is a `dict[model -> (step, messages) -> Turn]`; add a
new token scenario by adding one function and one dict entry.

## In tests

```python
from .mock_provider.server import running_server
from gantry.runtime.llm import LiteLLMClient

async with running_server() as base_url:
    client = LiteLLMClient(api_key="mock-key", api_base=base_url, prompt_caching=False)
    ...  # drive run_agent_task with model "openai/mock/happy-path"
```

See `tests/test_mock_provider.py` (mock-level) and `tests/test_mock_provider_e2e.py`
(drives the real agent loop + real breakers against the live mock).
