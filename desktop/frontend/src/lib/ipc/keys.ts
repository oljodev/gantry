/** Query keys mirror command names (docs/plan/01 §5). */
export const keys = {
  appInfo: ['app_info'] as const,
  settings: ['settings'] as const,
  secretStore: ['secret_store_status'] as const,
  dataInfo: ['data_info'] as const,
  providers: ['providers'] as const,
  models: (providerId: string) => ['models', providerId] as const,
  chats: ['chats'] as const,
  chat: (chatId: string) => ['chat', chatId] as const,
  search: (query: string) => ['search', query] as const,
  systemPrompt: (chatId: string) => ['system_prompt', chatId] as const,
  pendingInteractions: ['pending_interactions'] as const,
  toolCall: (callId: string) => ['tool_call', callId] as const,
  artifacts: (chatId: string) => ['artifacts', chatId] as const,
  /** The library across every chat; `['artifacts']` as a prefix invalidates every list. */
  allArtifacts: ['artifacts', 'all'] as const,
  artifact: (artifactId: string, version: number | undefined) =>
    ['artifact', artifactId, version ?? 'current'] as const,
};
