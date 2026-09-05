/** The connector showcase is driven by this list and nothing else.
 *  Adding, removing or swapping a connector is a one-line change here.
 *  `logo` is a path under public/connectors/ once an official asset is confirmed and cleared for use;
 *  until then it stays null and the tile shows a monogram in the category tint. First-party connectors
 *  use a Phosphor glyph instead of a monogram because they are Gantry's own. */
export type Category = 'local' | 'developer' | 'productivity' | 'data' | 'communication' | 'web';
export type FirstPartyIcon = 'folder' | 'code' | 'terminal' | 'globe';

export interface ConnectorEntry {
  name: string;
  slug: string;
  category: Category;
  logo: string | null;
  firstParty?: boolean;
  icon?: FirstPartyIcon;
}

export const categories: { id: Category | 'all'; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'local', label: 'Local' },
  { id: 'developer', label: 'Developer' },
  { id: 'productivity', label: 'Productivity' },
  { id: 'data', label: 'Data' },
  { id: 'communication', label: 'Communication' },
  { id: 'web', label: 'Web' },
];

export const connectors: ConnectorEntry[] = [
  { name: 'Filesystem', slug: 'filesystem', category: 'local', logo: null, firstParty: true, icon: 'folder' },
  { name: 'Code editor', slug: 'code-editor', category: 'local', logo: null, firstParty: true, icon: 'code' },
  { name: 'Shell', slug: 'shell', category: 'local', logo: null, firstParty: true, icon: 'terminal' },
  { name: 'Web', slug: 'web', category: 'local', logo: null, firstParty: true, icon: 'globe' },
  { name: 'GitHub', slug: 'github', category: 'developer', logo: null },
  { name: 'GitLab', slug: 'gitlab', category: 'developer', logo: null },
  { name: 'Linear', slug: 'linear', category: 'developer', logo: null },
  { name: 'Jira', slug: 'jira', category: 'developer', logo: null },
  { name: 'Sentry', slug: 'sentry', category: 'developer', logo: null },
  { name: 'Vercel', slug: 'vercel', category: 'developer', logo: null },
  { name: 'Cloudflare', slug: 'cloudflare', category: 'developer', logo: null },
  { name: 'Playwright', slug: 'playwright', category: 'developer', logo: null },
  { name: 'Docker', slug: 'docker', category: 'developer', logo: null },
  { name: 'Context7', slug: 'context7', category: 'developer', logo: null },
  { name: 'Google Drive', slug: 'google-drive', category: 'productivity', logo: null },
  { name: 'Google Calendar', slug: 'google-calendar', category: 'productivity', logo: null },
  { name: 'Gmail', slug: 'gmail', category: 'productivity', logo: null },
  { name: 'Notion', slug: 'notion', category: 'productivity', logo: null },
  { name: 'Figma', slug: 'figma', category: 'productivity', logo: null },
  { name: 'Asana', slug: 'asana', category: 'productivity', logo: null },
  { name: 'Todoist', slug: 'todoist', category: 'productivity', logo: null },
  { name: 'Supabase', slug: 'supabase', category: 'data', logo: null },
  { name: 'PostgreSQL', slug: 'postgresql', category: 'data', logo: null },
  { name: 'MongoDB', slug: 'mongodb', category: 'data', logo: null },
  { name: 'Neon', slug: 'neon', category: 'data', logo: null },
  { name: 'Airtable', slug: 'airtable', category: 'data', logo: null },
  { name: 'Stripe', slug: 'stripe', category: 'data', logo: null },
  { name: 'Slack', slug: 'slack', category: 'communication', logo: null },
  { name: 'Discord', slug: 'discord', category: 'communication', logo: null },
  { name: 'Microsoft Teams', slug: 'microsoft-teams', category: 'communication', logo: null },
  { name: 'Brave Search', slug: 'brave-search', category: 'web', logo: null },
  { name: 'Exa', slug: 'exa', category: 'web', logo: null },
  { name: 'Firecrawl', slug: 'firecrawl', category: 'web', logo: null },
  { name: 'Tavily', slug: 'tavily', category: 'web', logo: null },
];
