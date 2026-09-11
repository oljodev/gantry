/** Query keys mirror command names (docs/plan/01 §5). */
export const keys = {
  appInfo: ['app_info'] as const,
  settings: ['settings'] as const,
  secretStore: ['secret_store_status'] as const,
  guardrails: ['guardrails'] as const,
  guardDecisions: ['guard-decisions'] as const,
  dataInfo: ['data_info'] as const,
  providers: ['providers'] as const,
  catalog: ['catalog'] as const,
  connectors: ['connectors'] as const,
  runtimes: (catalogId: string) => ['runtimes', catalogId] as const,
  chatConnectors: (chatId: string) => ['chat_connectors', chatId] as const,
  models: (providerId: string) => ['models', providerId] as const,
  chats: ['chats'] as const,
  chat: (chatId: string) => ['chat', chatId] as const,
  search: (query: string) => ['search', query] as const,
  systemPrompt: (chatId: string) => ['system_prompt', chatId] as const,
  pendingInteractions: ['pending_interactions'] as const,
  toolCall: (callId: string) => ['tool_call', callId] as const,
  grants: (chatId: string) => ['grants', chatId] as const,
  artifacts: (chatId: string) => ['artifacts', chatId] as const,
  /** The library across every chat; `['artifacts']` as a prefix invalidates every list. */
  allArtifacts: ['artifacts', 'all'] as const,
  artifact: (artifactId: string, version: number | undefined) =>
    ['artifact', artifactId, version ?? 'current'] as const,
  /** What a code session changed; the file diffs hang under the same prefix (16 §5). */
  changes: (chatId: string) => ['changes', chatId] as const,
  fileDiff: (chatId: string, path: string) => ['changes', chatId, path] as const,
};
