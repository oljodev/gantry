export type ConnectorCategory =
  'local' | 'developer' | 'productivity' | 'data' | 'communication' | 'web';

export interface ConnectorEntry {
  id: string;
  name: string;
  category: ConnectorCategory;
  kind: 'native' | 'mcp';
  does: string;
  installed?: boolean;
  status?: 'connected' | 'needs_reconnect' | 'runtime_missing';
}

export const CATEGORIES: { id: ConnectorCategory; label: string }[] = [
  { id: 'local', label: 'Local' },
  { id: 'developer', label: 'Developer' },
  { id: 'productivity', label: 'Productivity' },
  { id: 'data', label: 'Data' },
  { id: 'communication', label: 'Communication' },
  { id: 'web', label: 'Web' },
];

/** The website's directory reduced to what the browse grid needs (website/src/data/connectors.ts). */
export const connectors: ConnectorEntry[] = [
  {
    id: 'filesystem',
    name: 'Filesystem',
    category: 'local',
    kind: 'native',
    does: 'Read, search and write files in attached folders',
    installed: true,
    status: 'connected',
  },
  {
    id: 'code-editor',
    name: 'Code editor',
    category: 'local',
    kind: 'native',
    does: 'Precise, reviewable source edits with undo',
    installed: true,
    status: 'connected',
  },
  {
    id: 'shell',
    name: 'Shell',
    category: 'local',
    kind: 'native',
    does: 'Run commands with streaming output and kill',
    installed: true,
    status: 'connected',
  },
  {
    id: 'web',
    name: 'Web',
    category: 'web',
    kind: 'native',
    does: 'Search the web and read pages as text',
  },
  {
    id: 'github',
    name: 'GitHub',
    category: 'developer',
    kind: 'mcp',
    does: 'Issues, pull requests, code and reviews',
    installed: true,
    status: 'connected',
  },
  {
    id: 'gitlab',
    name: 'GitLab',
    category: 'developer',
    kind: 'mcp',
    does: 'Projects, merge requests and pipelines',
  },
  {
    id: 'linear',
    name: 'Linear',
    category: 'developer',
    kind: 'mcp',
    does: 'Issues, projects and cycles',
  },
  {
    id: 'jira',
    name: 'Jira',
    category: 'developer',
    kind: 'mcp',
    does: 'Issues, boards and sprints',
  },
  {
    id: 'sentry',
    name: 'Sentry',
    category: 'developer',
    kind: 'mcp',
    does: 'Errors, traces and releases',
  },
  {
    id: 'vercel',
    name: 'Vercel',
    category: 'developer',
    kind: 'mcp',
    does: 'Deployments, logs and domains',
  },
  {
    id: 'cloudflare',
    name: 'Cloudflare',
    category: 'developer',
    kind: 'mcp',
    does: 'Workers, Pages, DNS and KV',
  },
  {
    id: 'playwright',
    name: 'Playwright',
    category: 'developer',
    kind: 'mcp',
    does: 'Drive a real browser',
    installed: true,
    status: 'runtime_missing',
  },
  {
    id: 'docker',
    name: 'Docker',
    category: 'developer',
    kind: 'mcp',
    does: 'Containers, images and logs',
  },
  {
    id: 'context7',
    name: 'Context7',
    category: 'developer',
    kind: 'mcp',
    does: 'Up-to-date library documentation',
  },
  {
    id: 'google-drive',
    name: 'Google Drive',
    category: 'productivity',
    kind: 'mcp',
    does: 'Files, folders and document text',
    installed: true,
    status: 'needs_reconnect',
  },
  {
    id: 'google-calendar',
    name: 'Google Calendar',
    category: 'productivity',
    kind: 'mcp',
    does: 'Events and availability',
  },
  {
    id: 'gmail',
    name: 'Gmail',
    category: 'productivity',
    kind: 'mcp',
    does: 'Search, read and draft mail',
  },
  {
    id: 'notion',
    name: 'Notion',
    category: 'productivity',
    kind: 'mcp',
    does: 'Pages, databases and search',
  },
  {
    id: 'figma',
    name: 'Figma',
    category: 'productivity',
    kind: 'mcp',
    does: 'Files, frames and design tokens',
  },
  { id: 'asana', name: 'Asana', category: 'productivity', kind: 'mcp', does: 'Tasks and projects' },
  {
    id: 'todoist',
    name: 'Todoist',
    category: 'productivity',
    kind: 'mcp',
    does: 'Tasks, projects and labels',
  },
  {
    id: 'supabase',
    name: 'Supabase',
    category: 'data',
    kind: 'mcp',
    does: 'Projects, SQL and edge functions',
  },
  {
    id: 'postgresql',
    name: 'PostgreSQL',
    category: 'data',
    kind: 'mcp',
    does: 'Query and inspect a database',
  },
  {
    id: 'mongodb',
    name: 'MongoDB',
    category: 'data',
    kind: 'mcp',
    does: 'Collections, documents and indexes',
  },
  {
    id: 'neon',
    name: 'Neon',
    category: 'data',
    kind: 'mcp',
    does: 'Serverless Postgres and branches',
  },
  {
    id: 'airtable',
    name: 'Airtable',
    category: 'data',
    kind: 'mcp',
    does: 'Bases, tables and records',
  },
  {
    id: 'stripe',
    name: 'Stripe',
    category: 'data',
    kind: 'mcp',
    does: 'Customers, payments and subscriptions',
  },
  {
    id: 'slack',
    name: 'Slack',
    category: 'communication',
    kind: 'mcp',
    does: 'Channels, messages and search',
  },
  {
    id: 'discord',
    name: 'Discord',
    category: 'communication',
    kind: 'mcp',
    does: 'Servers, channels and messages',
  },
  {
    id: 'microsoft-teams',
    name: 'Microsoft Teams',
    category: 'communication',
    kind: 'mcp',
    does: 'Teams, channels and chats',
  },
  {
    id: 'brave-search',
    name: 'Brave Search',
    category: 'web',
    kind: 'mcp',
    does: 'Web search with your own key',
  },
  { id: 'exa', name: 'Exa', category: 'web', kind: 'mcp', does: 'Neural search over the web' },
  {
    id: 'firecrawl',
    name: 'Firecrawl',
    category: 'web',
    kind: 'mcp',
    does: 'Crawl and scrape sites as clean text',
  },
  {
    id: 'tavily',
    name: 'Tavily',
    category: 'web',
    kind: 'mcp',
    does: 'Search and extract for agents',
  },
];

export const connectorName = (id: string) => connectors.find((c) => c.id === id)?.name ?? id;
