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
};
