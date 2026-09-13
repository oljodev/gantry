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
  connectorLogs: (instanceId: string) => ['connector_logs', instanceId] as const,
  connectorConfig: (catalogId: string, instanceId: string) =>
    ['connector_config', catalogId, instanceId] as const,
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
  /** Projects, one project's page, and the two lists it shows (09 M11). */
  projects: ['projects'] as const,
  project: (id: string) => ['project', id] as const,
  projectChats: (id: string) => ['project', id, 'chats'] as const,
  projectArtifacts: (id: string) => ['project', id, 'artifacts'] as const,
  /** The skill library and one skill's body (12 §A3). */
  skills: ['skills'] as const,
  skill: (id: string) => ['skill', id] as const,
  chatSkills: (chatId: string) => ['chat_skills', chatId] as const,
  /** One filtered view of the memory store; `['memories']` invalidates every view (12 §B5). */
  memories: (query: unknown) => ['memories', query] as const,
  /** What a code session changed; the file diffs hang under the same prefix (16 §5). */
  changes: (chatId: string) => ['changes', chatId] as const,
  fileDiff: (chatId: string, path: string) => ['changes', chatId, path] as const,
};
