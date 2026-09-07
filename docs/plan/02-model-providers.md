# 02 — Model provider abstraction

## 1. Goals and the build-vs-borrow decision

Goals: one internal request/response/stream model; five provider accounts (Anthropic, OpenAI, Google, xAI, OpenRouter) with room for custom OpenAI-compatible endpoints; first-class tool calling including streamed arguments; reasoning and provider server tools carried through without the rest of the app caring.

Non-goals: embeddings, batch, fine-tuning, audio.

**Build order (decided 2026-09-07).** The `openai_chat` client with the `openrouter` profile shipped first, in M1, because the developer tests only through OpenRouter; Anthropic, OpenAI Responses and Gemini follow in M4. Nothing in the trait or the types depends on the order.

**Decision: write the layer, do not adopt a unification crate.** The Rust ecosystem has usable multi-provider crates (`genai` is the most complete, and its source is a good reference for provider quirks). None of them expose what Gantry's agent loop needs as first-class concepts: append-only transcript rules on Anthropic (mid-conversation `system` messages, `tool_addition`/`tool_removal`, `defer_loading`, cache breakpoints), partial tool-argument streaming for live previews, provider server tools as opaque replayable parts, or Gemini's Interactions API. Wrapping a crate and patching around it costs more than owning ~4 clients that each map one well-documented HTTP API onto our types. LiteLLM-style Python layers are excluded by the brief.

The layer is four client implementations for five providers:

| Provider | Client | API |
|----------|--------|-----|
| Anthropic | `anthropic` | Messages API (`POST /v1/messages`, SSE) |
| OpenAI | `openai_responses` | Responses API (`POST /v1/responses`, SSE) |
| xAI, OpenRouter, custom endpoints | `openai_chat` | Chat Completions (`POST /v1/chat/completions`, SSE) with a per-vendor `CompatProfile` |
| Google | `gemini` | Interactions API (`POST /v1beta/interactions`, SSE) |

Why Chat Completions for xAI and OpenRouter when both also offer `/v1/responses`: Chat Completions is the path with the widest model coverage on OpenRouter and the most stable third-party semantics. A `prefer_responses` flag on the profile is the upgrade path; it does not change the trait.

## 2. The trait and the internal types

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &ProviderId;                       // "anthropic", "openai", "xai", "openrouter", "google", "custom:<ulid>"
    fn kind(&self) -> ProviderKind;                    // Anthropic | OpenAiResponses | OpenAiChat | Gemini
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    fn capabilities(&self, model: &str) -> ModelCapabilities;
    async fn stream(&self, req: ChatRequest) -> Result<ChatStream, ProviderError>;
}
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, ProviderError>> + Send>>;
```

Every request is streamed. Non-streaming is a `collect()` over the stream, used by the judge and the title generator.

```rust
pub struct ChatRequest {
    pub model: String,
    pub system: SystemPrompt,                 // frozen text; deltas travel as System messages (see §6)
    pub messages: Vec<Message>,               // the provider-neutral transcript
    pub tools: Vec<ToolSpec>,                 // namespaced names; schema sanitized inside the client
    pub tool_choice: ToolChoice,              // Auto | None | Required | Named(String)
    pub max_output_tokens: u32,
    pub reasoning: Reasoning,                 // Off | Adaptive { effort: Effort, show_summary: bool }
    pub server_tools: Vec<ServerTool>,        // WebSearch { max_uses, allowed_domains, blocked_domains } | WebFetch
    pub cache: CacheHints,                    // breakpoint placement policy for Anthropic; no-op elsewhere
    pub metadata: RequestMetadata,            // chat_id, turn_id (for logging), app name/version
    pub provider_options: serde_json::Value,  // escape hatch merged into the wire request
}

pub struct Message {
    pub id: MessageId,
    pub role: Role,                           // User | Assistant | Tool | System
    pub parts: Vec<ContentPart>,
    pub origin: Option<ProviderKind>,         // which provider produced an assistant message
}

pub enum ContentPart {
    Text { text: String },
    Image { source: MediaSource, mime: String },
    Document { source: MediaSource, mime: String, name: String },
    ToolCall { id: CallId, name: String, args: serde_json::Value },
    ToolResult { call_id: CallId, content: Vec<ResultPart>, is_error: bool },
    Thinking { text: String, signature: Option<String>, provider: ProviderKind }, // opaque; replayed only to its provider
    ProviderOpaque { provider: ProviderKind, kind: String, json: serde_json::Value }, // server tool use/result blocks, citations
    SystemNote { text: String },                              // role System: instruction change mid-chat
    ToolSetChange { added: Vec<String>, removed: Vec<String> }, // role System: attached/detached connectors
}

pub enum ResultPart { Text(String), Json(serde_json::Value), Image { data: Vec<u8>, mime: String }, Resource { handle: ResourceHandle, summary: String } }

pub struct ToolSpec {
    pub name: String,                 // model-facing, e.g. "filesystem__read_file"
    pub description: String,
    pub input_schema: serde_json::Value,
    pub strict: bool,                 // only true for first-party tools whose schema satisfies strict rules
    pub deferred: bool,               // Anthropic defer_loading
    pub stream_args: bool,            // Anthropic eager_input_streaming; used by write-heavy tools
}
```

### Stream events

```rust
pub enum StreamEvent {
    MessageStart { provider_message_id: Option<String> },
    TextDelta { index: u32, text: String },
    ThinkingDelta { index: u32, text: String },
    ThinkingSignature { index: u32, signature: String },
    ToolCallStart { index: u32, id: CallId, name: String },
    ToolCallArgsDelta { index: u32, json_fragment: String },
    ToolCallEnd { index: u32, args: serde_json::Value },      // parsed, complete arguments
    ProviderBlock { index: u32, part: ContentPart },           // opaque blocks to persist and replay verbatim
    Usage(Usage),
    MessageEnd { stop_reason: StopReason },
}
pub enum StopReason { EndTurn, ToolUse, MaxTokens, Refusal { category: Option<String> }, ContentFilter, PauseTurn, Cancelled, Other(String) }
pub struct Usage { input: u64, output: u64, cache_read: u64, cache_write: u64, reasoning: u64 }
```

`index` identifies a content block within the assistant message; every provider has an equivalent (Anthropic block index, OpenAI output item index, Gemini step index, Chat Completions tool-call index).

### Capabilities

```rust
pub struct ModelCapabilities {
    pub tools: bool, pub parallel_tools: bool, pub streams_tool_args: bool,
    pub vision: bool, pub pdf_input: bool,
    pub reasoning: ReasoningSupport,          // None | Effort { levels } | Budget
    pub server_web_search: bool, pub server_web_fetch: bool,
    pub native_tools: Vec<NativeToolKind>,    // AnthropicTextEditor, AnthropicBash, OpenAiApplyPatch, OpenAiShell
    pub prompt_caching: CacheSupport,         // Explicit | Automatic | None
    pub structured_output: bool,
    pub tool_changes_in_conversation: bool,   // Anthropic tool_addition/tool_removal
    pub context_window: u32, pub max_output: u32,
    pub pricing: Option<Pricing>,
}
```

Capabilities come from the provider's model list where it carries them (Anthropic's `/v1/models` has `max_input_tokens` and `capabilities`; OpenRouter's list has context, pricing and `supported_parameters`) merged with a shipped `desktop/assets/models/overrides.toml` for what the APIs do not say. Unknown model → conservative defaults (tools on, everything else off).

## 3. Tool-calling normalization

This is the part that has to be right, so it is written out per provider.

| Aspect | Anthropic Messages | OpenAI Responses | Chat Completions (xAI, OpenRouter, custom) | Gemini Interactions |
|--------|-------------------|------------------|--------------------------------------------|---------------------|
| System prompt | top-level `system` (text blocks, may carry `cache_control`) | `instructions` string | first message with role `system` (or `developer` per profile) | `system_instruction` |
| Tool definition | `{name, description, input_schema, strict?, defer_loading?, eager_input_streaming?}` | `{type:"function", name, description, parameters, strict}` | `{type:"function", function:{name, description, parameters}}` | `{type:"function", name, description, parameters}` |
| Name rules | `^[a-zA-Z0-9_-]{1,64}$` | 64 chars, same charset | same | letters/digits/`_`/`-`/`.`, 64 chars | 
| Schema dialect | JSON Schema 2020-12 | JSON Schema; strict needs `additionalProperties:false` + all properties required | JSON Schema (vendor-dependent leniency) | OpenAPI-style subset: no `$ref`/`$defs`, limited keywords; sanitizer inlines refs and drops unsupported keys |
| Tool call in output | `tool_use` block `{id: toolu_…, name, input}` | `function_call` item `{id, call_id: call_…, name, arguments: string}` | `message.tool_calls[] {id, function:{name, arguments: string}}` | `function_call` step `{id, name, arguments: object}` |
| Argument streaming | `content_block_delta` → `input_json_delta.partial_json`; finer with `eager_input_streaming` | `response.function_call_arguments.delta` / `.done` | `delta.tool_calls[i].function.arguments` fragments | `step.delta` → `{type:"arguments_delta", arguments: string}` (Gemini 3+) |
| Tool result placement | one `user` message holding **all** parallel `tool_result` blocks `{tool_use_id, content, is_error}` | `function_call_output` items `{call_id, output}` | one `tool` message per call `{tool_call_id, content}` directly after the assistant message | `function_result` items `{call_id, name, result:[{type:"text",…}]}` |
| Call ids | provider-assigned; any `[A-Za-z0-9_-]` string round-trips | same | same; some OpenRouter upstreams emit empty ids → synthesize `gantry_<ulid>` | same |
| Parallel calls | yes; `disable_parallel_tool_use` to stop it | yes; `parallel_tool_calls` | yes (vendor-dependent) | yes; `thought_signature` is attached only to the first `function_call`; keep order |
| Tool choice | `{type: auto|any|tool|none}`; `any`/`tool` are rejected by Claude Fable 5.1 → downgrade to `auto` + instruction | `auto|required|none|{type:"function",name}` | same | `generation_config.tool_choice`: `auto|any|none|validated`, plus `allowed_tools` |
| Reasoning | `thinking: {type:"adaptive"}`, `output_config.effort`; `thinking` blocks carry a `signature` and must be replayed verbatim, in an **append-only** history | `reasoning: {effort, summary}`; with `store:false`, request `include: ["reasoning.encrypted_content"]` and replay `reasoning` items between tool rounds | `reasoning_effort` (xAI, OpenAI-style) or OpenRouter's `reasoning: {effort}`; replay of reasoning content is optional | `generation_config.thinking_level`; `thought_signature` deltas must be echoed on replay (Gemini 3 rejects missing signatures) |
| Server tools | `web_search_20260209`, `web_fetch_20260209` (older models: `web_search_20250305`); results arrive as `server_tool_use` + `web_search_tool_result` blocks, replayed as-is | `{type:"web_search"}`; `web_search_call` items | OpenRouter `plugins:[{id:"web"}]`; xAI server tools only via Responses | `google_search` built-in tool |
| Native client-executed tools | `text_editor_20250728` (`str_replace_based_edit_tool`), `bash_20250124` | `apply_patch` (`apply_patch_call` → `apply_patch_call_output`, V4A diffs), `shell` | none | none |
| Caching | explicit `cache_control` breakpoints (max 4); minimum cacheable prefix 512–4096 tokens; prefix must be byte-stable | automatic prefix caching (`prompt_cache_key` optional) | automatic or pass-through (OpenRouter forwards `cache_control` for Anthropic models) | implicit; `previous_interaction_id` for server-held state (not used by default) |
| Stop reasons | `end_turn`, `tool_use`, `max_tokens`, `stop_sequence`, `refusal` (+ `stop_details`), `pause_turn` | `status: completed|incomplete` + `incomplete_details.reason`; tool use inferred from items | `finish_reason: stop|tool_calls|length|content_filter` | `interaction.completed.status`; tool use inferred from steps; `error` events |
| Usage | `message_start.usage`, `message_delta.usage` incl. cache read/write | `response.completed.response.usage` incl. `cached_tokens` | final chunk `usage` when `stream_options.include_usage` | `interaction.completed.interaction.usage` (`total_input_tokens`, `total_output_tokens`, `total_cached_tokens`, `total_thought_tokens`) |
| Files/images | `image` and `document` blocks (base64 or Files API id) | `input_image`, `input_file` | `image_url` data URLs | `image`/`document` input parts |
| Auth headers | `x-api-key`, `anthropic-version`, `anthropic-beta` | `Authorization: Bearer` | `Authorization: Bearer`; OpenRouter also `HTTP-Referer`, `X-Title` | `x-goog-api-key` |
| Model list | `GET /v1/models` (context and capabilities included) | `GET /v1/models` (ids only) | `GET /v1/models` (OpenRouter: context, pricing, params) | `GET /v1beta/models` |

Shared rules enforced by the layer:

- **Names.** Model-facing tool names are `<connector_id>__<tool>` restricted to `^[a-zA-Z][a-zA-Z0-9_-]{0,63}$`. Over-long names are truncated with a 6-char hash suffix; a per-request `ToolNameMap` translates back.
- **Schemas.** `ToolSchemaSanitizer::for_provider(kind)` inlines `$ref`/`$defs`, removes keywords a provider rejects, guarantees `type: object` at the root, and never enables strict mode for MCP-sourced schemas.
- **Results are complete.** A transcript is never sent with a `ToolCall` lacking a `ToolResult`; the agent synthesizes error results on cancel or crash.
- **Parallel results travel together** (Anthropic) or as consecutive items (others); the projection layer decides, callers just append `Tool` messages.
- **Every tool call id round-trips unchanged** to the same provider; to a different provider (model switch mid-chat) ids are sanitized but kept.
- **Argument deltas are best-effort.** Anthropic, OpenAI (both APIs) and Gemini 3+ stream partial tool arguments; Chat Completions upstreams behind OpenRouter vary, and xAI's current documentation states streaming works for all text models without the tool-calling restriction its older pages carried (verified in the M4 conformance run). Consumers of `ToolCallArgsDelta` (the code-editor live preview, the artifact panel) must work when zero deltas arrive and only `ToolCallEnd` does; 13 §2 describes the buffered fallback.

## 4. Per-provider notes

### Anthropic (Messages API)

- Beta headers Gantry sends when the model supports them: `mid-conversation-tool-changes-2026-07-01` (tool set deltas), `thinking-binding-controls-2026-08-01` (only on requests that knowingly changed the prefix), `context-management-2025-06-27` or `compact-2026-01-12` (context, see §6).
- Thinking is adaptive on 4.6+ models; `effort` comes from the chat's Thinking setting through `output_config.effort`. Chats that show reasoning set `display: "summarized"`.
- **Append-only transcript.** Current Claude models bind thinking blocks to the exact prefix (`system`, `tools`, prior messages) that produced them; editing the prefix invalidates later blocks (a 400 on accounts created after 2026-08-31, enforced for everyone on later models). Consequences implemented in §6: the system prompt is frozen per chat; instruction changes are `SystemNote` → `{"role":"system"}` messages; tool-set changes go through `tool_addition`/`tool_removal` for tools declared up front with `defer_loading: true`; old tool results are never deleted client-side; compaction is server-side or "simple" (summary replaces everything).
- `refusal` stop reasons are surfaced as a distinct assistant state (not an error), with `stop_details.category` when present. Forced `tool_choice` is never used on Claude Fable 5.1.
- `eager_input_streaming: true` on the code-editor and filesystem write tools so a file's content streams into the activity feed while the model writes it.
- Cache breakpoints: after the system prompt, after the tool list, and on the last user message. The prefix stays byte-identical across turns by construction (frozen system prompt, sorted deterministic tool list, no timestamps in the prefix).

### OpenAI (Responses API)

- Stateless by choice: `store: false`, no `previous_response_id`. Gantry's transcript is authoritative and privacy is simpler on a BYOK desktop app. Therefore `include: ["reasoning.encrypted_content"]` is requested and `reasoning` items are replayed between tool rounds; without them reasoning models lose their chain across tool calls.
- Items: `message`, `function_call`, `function_call_output`, `reasoning`, `web_search_call`. Streaming events consumed: `response.output_item.added/done`, `response.output_text.delta`, `response.function_call_arguments.delta/done`, `response.completed`, `error`.
- `strict: true` only for first-party tools flagged strict-compatible. `parallel_tool_calls: true`.
- Native `apply_patch` and `shell` tool types are the post-MVP mapping target for the code-editor and shell connectors (T8).

### OpenAI-compatible (Chat Completions)

One client, many `CompatProfile`s:

```rust
pub struct CompatProfile {
    pub base_url: Url,
    pub auth: AuthStyle,                 // Bearer
    pub extra_headers: Vec<(String, String)>,   // OpenRouter attribution headers
    pub system_role: &'static str,       // "system" or "developer"
    pub reasoning_param: ReasoningParam, // OpenAiEffort | OpenRouterObject | None
    pub supports_stream_usage: bool,     // stream_options.include_usage
    pub supports_parallel_flag: bool,
    pub supports_strict: bool,
    pub models_parser: ModelsParser,     // Plain | OpenRouter
    pub tool_id_quirk: ToolIdQuirk,      // None | SynthesizeIfEmpty
}
```

Shipped profiles: `xai` (`https://api.x.ai/v1`), `openrouter` (`https://openrouter.ai/api/v1`), `custom` (user-supplied base URL and optional key; this is also how Ollama or LM Studio get in later without a new client).

### Google (Gemini Interactions API)

- Endpoint `POST https://generativelanguage.googleapis.com/v1beta/interactions` with `"stream": true`. The Interactions API is Google's default surface since June 2026; `generateContent` is legacy and not targeted.
- Stateless by default: `store: false` and the whole history in `input`. `previous_interaction_id` is a later optimization, not a dependency.
- Request: `model`, `system_instruction`, `input` (turns and `function_result` items), `tools` (function declarations and built-ins such as `google_search`), `generation_config` (`tool_choice`, `thinking_level`, `temperature`).
- Output steps: `model_output` (text), `thought` (summaries), `function_call {id, name, arguments}`. Results go back as `function_result {call_id, name, result: [...]}`.
- Streaming: `interaction.created`, `step.start`, `step.delta` (`text`, `thought_summary`, `arguments_delta`, `thought_signature`, `image`), `step.stop`, `interaction.completed` (usage), `error`, `done`. Unknown event types are ignored by design; Google documents that new types will appear.
- Thought signatures are captured per step and echoed back on the corresponding replayed item. Missing signatures are a validation error on Gemini 3 models.
- Schema sanitizer: inline `$ref`, drop `additionalProperties`, `patternProperties`, `$schema`, `examples`; keep `enum`, `anyOf`, `format` where documented.

## 5. Reasoning, server tools and opaque parts

`Thinking` and `ProviderOpaque` parts are tagged with the provider that produced them. Projection keeps them only for that provider and drops them for others, which is what every provider tolerates. Server-tool results (web search) are stored as `ProviderOpaque` and rendered in the activity feed through small per-provider decoders ("Searched the web for …", with sources), so the transparency UI works without the agent understanding each provider's block shapes.

## 6. Transcript → request projection

`Transcript` in `gantry-agent` is an append-only list of `Message`s plus per-chat frozen context. `project(provider, model)` produces a `ChatRequest`:

1. Start from the chat's `system_snapshot` (frozen at chat creation: base prompt, project instructions at that time, permission-mode guidance, connector inventory summary). Later changes are `System` messages in the list.
2. Walk messages in order. `Tool` messages following an assistant tool call are grouped per provider rule. `Thinking`/`ProviderOpaque` from other providers are dropped. Missing tool results are synthesized as errors.
3. `SystemNote` → Anthropic `{"role":"system","content":[…]}`; OpenAI `developer` item; Chat Completions a mid-list `system` message (profiles that reject it get the note merged into the first system message, which costs the cache on that vendor only); Gemini appends to `system_instruction` (stateless, so allowed).
4. `ToolSetChange` → Anthropic `tool_addition`/`tool_removal` blocks when every added tool was declared with `defer_loading` at the chat's first Anthropic request; otherwise the tool array is rebuilt and the request carries `thinking.block_binding.prefix_mismatch_behavior: "drop_block"`, and `input_transformations` from the response is logged as a `provider.notice` event. Other providers rebuild `tools`.
5. Apply `ContextBudget` (below), then cache hints.

**Context budget.** Estimate tokens (last known usage + chars/4 for new content). Tool results are capped at ingestion (default 50 KB, head and tail kept, the full output lives in a blob and is viewable in the UI); capping at ingestion is not a history edit. When the estimate passes 75% of the context window:

- Anthropic: server-side context editing (`clear_tool_uses_20250919`) first; server-side compaction when available; client-side "simple compaction" as the last resort (a summary produced by the judge model replaces the whole history, then the transcript continues append-only from a compaction marker message). Never keep-tail compaction.
- Others: keep-tail compaction (summarize older turns, keep the last N turns verbatim). The compaction marker is a `System` message so the UI can show "Earlier conversation summarized".

## 7. Errors, retries, timeouts

```rust
pub enum ProviderError {
    Auth, RateLimited { retry_after: Option<Duration> }, Overloaded,
    InvalidRequest { message: String }, ContextTooLong, NotFound { model: String },
    Network(String), StreamInterrupted, Refused { category: Option<String> }, Unknown { status: u16, body: String },
}
```

Retry: up to 3 attempts with jittered backoff on `RateLimited`, `Overloaded`, `Network` **before the first byte**. After output has started, a failure ends the turn with an `error` event and a "Retry" affordance; silently re-sending would duplicate side effects the model may have already reasoned about. Timeouts: connect 15 s, first token 60 s, idle 120 s between stream events, no overall cap.

## 8. Testing

- Recorded SSE fixtures per provider (`tests/fixtures/<provider>/*.sse`) replayed through the real parsers; every row of the normalization table has a fixture.
- A `MockProvider` scripted with `StreamEvent`s for `gantry-agent` tests (tool loops, cancellation, permission paths).
- An opt-in live smoke test (`--features live`) that runs a 12-scenario conformance list against real keys: text, parallel tools, tool error, refusal, max tokens, cancel mid-stream, image input, thinking replay across a tool round, cache hit on turn two, server web search, tool-set change, model switch mid-chat.
