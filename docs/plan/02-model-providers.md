# 02 — Model provider abstraction

## 1. Goals and the build-vs-borrow decision

Goals: one internal request/response/stream model; five provider accounts (Anthropic, OpenAI, Google, xAI, OpenRouter) with room for custom OpenAI-compatible endpoints; first-class tool calling including streamed arguments; reasoning and provider server tools carried through without the rest of the app caring.

Non-goals: embeddings, batch, fine-tuning, audio.

**Build order (decided 2026-09-07).** The `openai_chat` client with the `openrouter` profile shipped first, in M1, because the developer tests only through OpenRouter; Anthropic, OpenAI Responses and Gemini followed in M4 (all four clients exist as of 2026-09-07). Nothing in the trait or the types depends on the order.

**Status of the live verification.** Only OpenRouter has been exercised against a real key. The Anthropic, OpenAI and Gemini clients are pinned by fixtures written to the documented wire formats (`tests/fixtures/<provider>/`, `tests/replay.rs`, `tests/projection.rs`); the first run of `tests/live.rs` with each key is where a renamed field shows up, and the fixtures are replaced by real captures then.

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
    pub server_tools: Vec<ServerTool>,        // WebSearch { max_uses } for now; domains and WebFetch later
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
    ToolCall { id: CallId, name: String, args: serde_json::Value, signature: Option<String> }, // signature: Gemini's thought signature, echoed on replay
    ToolResult { call_id: CallId, content: Vec<ResultPart>, is_error: bool },
    Thinking { text: String, signature: Option<String>, provider: ProviderKind, item_id: Option<String> }, // opaque; replayed only to its provider; item_id is OpenAI's rs_ id
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

Capabilities come from the provider's model list where it carries them (Anthropic's `/v1/models` has `max_input_tokens` and `capabilities`; OpenRouter's list has context, pricing and `supported_parameters`; xAI's `/v1/language-models` has modalities and prices; Gemini's `/v1beta/models` has token limits) merged with a shipped `desktop/assets/models/overrides.toml` for what the APIs do not say. The merge happens in every client's `list_models` and again when the cached list is read, so an edit to the file applies without a refresh; entries name a provider id or client kind and an exact id or a `prefix*`, later entries refine earlier ones. Unknown model → conservative defaults (tools on, everything else off).

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
- **Every tool call id round-trips unchanged** to the same provider; to a different provider (model switch mid-chat) ids are sanitized but kept (`wire_call_id`: the assistant message's `origin` decides, and a result takes the treatment of the call it answers).
- **Thinking left behind is said.** When a chat's model changed since its previous turn and the last reply had thinking, the runner emits a `provider.notice` of kind `thinking_dropped` ("Thinking context reset") before the first request, since that reasoning is not sent to the new model.
- **Argument deltas are best-effort.** Anthropic, OpenAI (both APIs) and Gemini 3+ stream partial tool arguments; Chat Completions upstreams behind OpenRouter vary, and xAI's current documentation states streaming works for all text models without the tool-calling restriction its older pages carried (verified in the M4 conformance run). Consumers of `ToolCallArgsDelta` (the code-editor live preview, the artifact panel) must work when zero deltas arrive and only `ToolCallEnd` does; 13 §2 describes the buffered fallback.

## 4. Per-provider notes

### Anthropic (Messages API)

- Beta headers Gantry sends when the model supports them: `mid-conversation-tool-changes-2026-07-01` (tool set deltas), `thinking-binding-controls-2026-08-01` (only on requests that knowingly changed the prefix), `context-management-2025-06-27` or `compact-2026-01-12` (context, see §6).
- Thinking is adaptive on 4.6+ models (`thinking: {type: "adaptive"}`, `effort` from the chat's Thinking setting through `output_config.effort`); earlier models get `{type: "enabled", budget_tokens}` sized from the effort and kept below `max_tokens`. The style is decided by the model's `reasoning` capability (`overrides.toml`), falling back to the version in the id. "Off" omits `thinking` entirely.
- The mid-conversation `system` message for `SystemNote`s, `eager_input_streaming` on tools flagged `stream_args`, and `stop_details.category` on refusals are implemented to this document and await the first live conformance run.
- **Append-only transcript.** Current Claude models bind thinking blocks to the exact prefix (`system`, `tools`, prior messages) that produced them; editing the prefix invalidates later blocks (a 400 on accounts created after 2026-08-31, enforced for everyone on later models). Consequences implemented in §6: the system prompt is frozen per chat; instruction changes are `SystemNote` → `{"role":"system"}` messages; tool-set changes go through `tool_addition`/`tool_removal` for tools declared up front with `defer_loading: true`; old tool results are never deleted client-side; compaction is server-side or "simple" (summary replaces everything).
- `refusal` stop reasons are surfaced as a distinct assistant state (not an error), with `stop_details.category` when present. Forced `tool_choice` is never used on Claude Fable 5.1.
- `eager_input_streaming: true` on the code-editor and filesystem write tools so a file's content streams into the activity feed while the model writes it.
- Cache breakpoints: after the system prompt, after the tool list, and on the last user message. The prefix stays byte-identical across turns by construction (frozen system prompt, sorted deterministic tool list, no timestamps in the prefix).

### OpenAI (Responses API)

- Stateless by choice: `store: false`, no `previous_response_id`. Gantry's transcript is authoritative and privacy is simpler on a BYOK desktop app. Therefore `include: ["reasoning.encrypted_content"]` is requested and `reasoning` items are replayed between tool rounds; without them reasoning models lose their chain across tool calls.
- Items: `message`, `function_call`, `function_call_output`, `reasoning`, `web_search_call`. Streaming events consumed: `response.output_item.added/done`, `response.output_text.delta`, `response.function_call_arguments.delta/done`, `response.completed`, `error`.
- `strict: true` only for first-party tools flagged strict-compatible. `parallel_tool_calls: true`.
- `reasoning.effort` is sent for low, medium and high (max maps to high); "Off" leaves the model's default in place, because the accepted lowest level differs per family (`minimal` on gpt-5, `none` on gpt-5.1 and later) and the wrong one is a 400. `summary: "auto"` gives the thinking block its text.
- The model list carries ids only; ids that are plainly not chat models (embeddings, audio, images, realtime, moderation) are dropped before `overrides.toml` fills in the rest.
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
    pub model_categories: &'static [&'static str],  // kinds the plain list leaves out (§5)
    pub media_endpoints: bool,           // /images, /audio/speech, /videos beside the chat one
}
```

Shipped profiles: `xai` (`https://api.x.ai/v1`; `reasoning_effort`, `stream_options.include_usage`, the model list from `/v1/language-models` whose prices divide by 10 000 to dollars per million tokens), `openrouter` (`https://openrouter.ai/api/v1`; web search through `plugins: [{ id: "web" }]`), `custom` (user-supplied base URL and optional key, `custom:<ulid>` rows added from Settings; this is how Ollama or LM Studio get in without a new client). The profile also carries `models_path`, `web_search` and `key_optional`.

### Google (Gemini Interactions API)

- Endpoint `POST https://generativelanguage.googleapis.com/v1beta/interactions` with `"stream": true`. The Interactions API is Google's default surface since June 2026; `generateContent` is legacy and not targeted.
- Stateless by default: `store: false` and the whole history in `input`. `previous_interaction_id` is a later optimization, not a dependency.
- Request: `model`, `system_instruction`, `input` (turns and `function_result` items), `tools` (function declarations and built-ins such as `google_search`), `generation_config` (`tool_choice`, `thinking_level`, `temperature`).
- Output steps: `model_output` (text), `thought` (summaries), `function_call {id, name, arguments}`. Results go back as `function_result {call_id, name, result: [...]}`.
- Streaming: `interaction.created`, `step.start`, `step.delta` (`text`, `thought_summary`, `arguments_delta`, `thought_signature`, `image`), `step.stop`, `interaction.completed` (usage), `error`, `done`. Unknown event types are ignored by design; Google documents that new types will appear.
- Thought signatures are captured per step and echoed back on the corresponding replayed item: a `function_call` item carries `thought_signature`, a thought is replayed as a `thought` content part with its signature. Missing signatures are a validation error on Gemini 3 models. The signed call is re-issued whole as a `ProviderBlock` at `step.stop`, so the runner's transcript keeps the signature on the `ToolCall` part.
- `function_result` items name the function, so the projection looks the name up from the call with the same id. `SystemNote`s are appended to `system_instruction` (stateless, so allowed); `thinking_level` maps Off → `minimal` (Gemini cannot switch thinking off), Max → `high`.
- Auth is `x-goog-api-key`; the model list is `GET /v1beta/models` filtered to `generateContent` models named `gemini*`.
- Schema sanitizer: inline `$ref`, drop `additionalProperties`, `patternProperties`, `$schema`, `examples`; keep `enum`, `anyOf`, `format` where documented.

## 5. Reasoning, server tools and opaque parts

`Thinking` and `ProviderOpaque` parts are tagged with the provider that produced them. Projection keeps them only for that provider and drops them for others, which is what every provider tolerates. Server-tool results (web search) are stored as `ProviderOpaque` and rendered in the activity feed through small per-provider decoders ("Searched the web for …", with sources), so the transparency UI works without the agent understanding each provider's block shapes.

**Image output** (built 2026-09-08). A model whose catalog row says it produces images is asked
for them: the Chat Completions body carries `modalities: ["image", "text"]`, without which an
image model answers with a paragraph about the picture it would have drawn. Pictures come back
whole rather than in deltas, as data URLs in `delta.images`, and become `ContentPart::Image`
blocks through `ProviderBlock`, so they are persisted, replayed to the view and shown in the
answer at the point the model produced them. A hosted `http(s)` URL is *not* turned into a part:
nothing in the app fetches it, and a part pointing at a picture the app never read would be a
lie. Projection drops assistant images on the way back to the provider, so a chat full of
generated pictures does not re-send them.

**Sound** (built 2026-09-10) works the same way one layer down. A model whose row says it
produces audio is asked for it with `modalities: ["audio", "text"]` and an `audio` object naming
a voice and a format; MP3 is the format asked for, because the fragments have to concatenate and
every webview plays it. The sound arrives as `delta.audio` in pieces, each separately
base64-encoded — sticking the strings together would put padding in the middle of the file, so
each is decoded and the *bytes* are joined, and the one file is emitted as a
`ContentPart::Audio` when the message closes. The transcript that comes beside it is streamed as
ordinary text, which is what makes the answer readable while it is still being said and
searchable afterwards. A voice has to be named and no two vendors agree on the names, so the
model's own first listed voice is used where the catalog has one and OpenRouter's documented
`alloy` where it has none.

### Media models: the endpoints that are not `chat/completions`

A model that draws a picture, reads a passage aloud or renders a clip is not a chat model with
an extra output modality. It takes a prompt rather than a conversation, and it answers with a
file. OpenRouter puts each on its own endpoint and — this is the part that matters for the
catalog — leaves all of them out of `GET /models`, which answers with the models its *chat*
endpoint can serve. They are asked for by name instead: `?output_modality=image` (54 models),
`speech` (18), `video` (28), against 437 for text. `audio` names something else again: a chat
model that answers in sound, which is the case above.

| Kind | Endpoint | Shape |
|------|----------|-------|
| Image, no text | `POST /images` | `{model, prompt, aspect_ratio?, quality?}` → `data: [{b64_json, media_type}]`, `usage.cost` |
| Speech | `POST /audio/speech` | `{model, input, voice?, response_format}` → the audio file itself |
| Video | `POST /videos` → `GET /videos/{id}` | `{model, prompt, aspect_ratio?, resolution?, duration?}` → a job id, polled to `completed`, then the clip downloaded |

**What a model lets you choose, and what it charges**, come from two more listings —
`GET /videos/models` and `GET /images/models` — which the plain list replaces with nothing:
`supported_aspect_ratios`, `supported_resolutions`, `supported_durations` and `pricing_skus` on a
video model, and a `supported_parameters` map of enums and ranges on an image one. They are
merged onto the models the category queries produced, by id, and land in `ModelCapabilities`
(`aspect_ratios`, `resolutions`, `durations`, `qualities`, beside `voices`) so the dialog can
offer a model exactly what that model takes and nothing else.

`pricing_skus` has some thirty key shapes across the 28 video models. Only the ones that really
are per-second are read — `duration_seconds…` in dollars, `cents_per_second_output…` and
`cents_per_video_output_second…` in cents — keyed by the resolution named in the rest of the key.
A model priced by the token or by the megapixel-second cannot be turned into a per-second figure
without knowing what it will produce, so it shows no price at all rather than an invented one.
Everything else follows from that number: the row shows the span (`$0.05–$0.28 / s`), and the
options strip shows what the clip as configured comes to (`8 s at 1080p ≈ $1.60`), which is the
moment a person is deciding to spend it.

Speech models are billed **per character read**, not per token sent — the provider reports it in
the same `prompt` field, and Deepgram's `0.00003` is its published $0.030 per thousand characters
— so the row says `/ M chars`. A choice made in the strip is remembered against the model
(`ChatSettings.model_options`, keyed `provider/model`), not against the chat: a voice is a
property of the voice model you picked, and choosing it again in every new chat is work software
should not ask for. Nothing is sent unless it was picked — a model's own default beats a guess.

`media::route` decides from the model's output modalities alone: video wins, then speech, then
image *without* text — a model that answers with a picture and a paragraph is a chat model that
draws, and keeps the chat endpoint. All three answer on the same `ChatStream` the chat client
returns, so a turn never learns which kind of model it is talking to.

Three things follow from the shape rather than from taste:

- **Only the last user message is sent.** These endpoints take one string. Pasting the
  conversation into it would put the model's own past answers into its next picture.
- **A clip takes from half a minute to several**, so the video route emits a `Notice` event
  every ten seconds while it polls (05 §2: shown live, never persisted) and gives up after
  fifteen minutes with the job id named. Dropping the stream — which is what cancelling a turn
  does — stops the polling, because each poll is one step of the stream rather than a task
  running beside it.
- **What comes back is parked in the blob store**, not kept inside the message: the live event
  carries the bytes so the answer appears at once, and what is written down carries a hash. A
  transcript holding thirty megabytes of base64 is read whole, out of SQLite, every time the
  chat is opened. The ceiling is 32 MB per file, refused with its size named.

Neither sound nor video goes back to a provider: nothing accepts them as input, and a message
whose only part was one would project to nothing at all, so projection replaces them with a
sentence saying what happened.

Two things are known to be unsettled until the first live run, both cheap to change: whether a
music model (Lyria) accepts the `voice` the `audio` object carries, and whether the speech
endpoint's error path really is JSON on a route that otherwise answers with a file — the client
checks the content type and says so either way.

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

### As built (2026-09-11)

`gantry-agent/src/context.rs`, with the summarizer's instructions in
`desktop/assets/prompts/compaction.md`. Five things the paragraph above did not settle.

**A marker, never a deletion.** Compaction appends one `System` message carrying
`ContentPart::Compacted { summary, up_to, replaced, artifacts }`. `up_to` names the last message
the summary covers; `context::live` drops everything up to it when a turn loads the transcript,
and nothing else in the app changes. The rows stay in the database and stay on the screen, which
is what the append-only rule of §1 requires and also what a person needs — a model that "forgot"
something written three rows above looks broken unless the reason is on screen between them.

**The marker is appended last and projected first.** A summary of the beginning belongs at the
beginning, but an append-only transcript can only grow at the end. `live` resolves this: it
returns the marker, then the messages the marker does not cover. So the database ordering and
the request ordering can both be right.

**Compaction runs between turns, never inside one.** A turn's tool calls and their results have
to reach the model together, and summarizing a conversation the assistant is in the middle of is
how a tool loop loses the thread. The check runs when a turn finishes and sizes the request the
*next* turn will make, after the answer is on screen — so it costs the user no waiting, and the
marker arrives with the chat's next refresh. What defends a single runaway turn is the tool
result cap and `max_tool_rounds`, not this.

**The estimate prefers what the provider charged for.** At the end of a turn the last round's
`usage` is a measurement, not a guess: `input + cache_read + output` is very close to the next
request's prefix. Counting characters at four per token, plus the system prompt and the tool
schemas, is the fallback for a provider that reported nothing — good to perhaps ±25%, which is
why the threshold is three quarters of the window rather than the edge of it. A model whose
window nobody knows is assumed to have 128k.

**The cut lands on a turn boundary.** Keep-tail keeps the last three turns; the cut is always
before a user message, so a tool call never ends up inside the summary while its result stays in
the transcript. Anthropic keeps none, per the rule above, which makes "simple compaction" the
same code with `keep = 0`. A span shorter than four messages is not worth a model call.

Two things are deliberately not built. **Anthropic's server-side context editing and server-side
compaction** are beta request shapes that cannot be verified without an Anthropic key, and
writing unverifiable JSON against a beta API is worse than not writing it: Anthropic gets simple
compaction, which is the documented last resort and is provider-neutral code. **The full tool
output as a blob** is still missing, so a capped result is capped everywhere rather than capped
for the model and whole in the drawer; the activity row keeps what streamed.

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
