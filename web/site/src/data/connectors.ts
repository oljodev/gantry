/**
 * The connector catalog on the site, and nothing else drives it: the directory, every /connectors/<slug>/ page,
 * the home-page logo cloud and the request-a-connector links. Adding a connector is one entry here.
 *
 * Marks: the build looks each third-party entry up in Simple Icons by `icon` (default: the slug without hyphens);
 * `icon: false` forces two-letter initials. First-party connectors use Gantry's own glyphs.
 */
export const CATEGORY_IDS = ['local', 'developer', 'productivity', 'data', 'communication', 'web'] as const;
export type CategoryId = (typeof CATEGORY_IDS)[number];

export const CATEGORIES: Record<CategoryId, { label: string; blurb: string }> = {
  local: { label: 'Local', blurb: 'Files, editor, shell and fetch on this machine. Built into Gantry.' },
  developer: { label: 'Developer', blurb: 'Repositories, issues, deploys and errors.' },
  productivity: { label: 'Productivity', blurb: 'Documents, calendars, mail and tasks.' },
  data: { label: 'Data', blurb: 'Databases, tables and payments.' },
  communication: { label: 'Communication', blurb: 'Where your team talks.' },
  web: { label: 'Web', blurb: 'Search and read the open web.' },
};

export type Auth = 'none' | 'api-key' | 'oauth' | 'connection-string';
export type Runtime = 'native' | 'mcp-remote' | 'mcp-stdio';
/** `available` has a folder in desktop/connectors/ and ships in the app; `soon` is planned (docs/plan/17-connector-catalog.md). */
export type Status = 'available' | 'soon';

export interface Connector {
  slug: string;
  name: string;
  category: CategoryId;
  status: Status;
  /** One line for tiles. */
  does: string;
  /** A short paragraph for the connector's page. */
  summary: string;
  /** Example actions the agent can take, shown as tool calls. */
  capabilities: string[];
  auth: Auth;
  runtime: Runtime;
  website?: string;
  firstParty?: boolean;
  icon?: string | false;
}

export const connectors: Connector[] = [
  { slug: 'filesystem', name: 'Filesystem', status: 'available', category: 'local', firstParty: true, auth: 'none', runtime: 'native',
    does: 'Read, search and write files in the folders you add',
    summary: 'The agent works inside the folders you attach to a chat and nowhere else. Reads run freely in every mode; writes show up as diffs and follow the chat’s permission mode.',
    capabilities: ['read_file src/lib/auth.rs', 'search "TODO" in src/', 'list_directory crates/', 'write_file docs/notes.md'] },
  { slug: 'code-editor', name: 'Code editor', status: 'available', category: 'local', firstParty: true, auth: 'none', runtime: 'native',
    does: 'Targeted edits shown as diffs, revertible',
    summary: 'Precise, reviewable changes: the agent replaces exact spans of a file, you see the diff in the activity feed before and after, and every edit can be reverted from the feed.',
    capabilities: ['str_replace src/lib/auth.rs', 'insert_after line 42', 'create_file src/lib/limits.rs', 'revert last edit'] },
  { slug: 'shell', name: 'Shell', status: 'available', category: 'local', firstParty: true, auth: 'none', runtime: 'native',
    does: 'Run commands with streaming output',
    summary: 'Builds, tests and scripts in your own shell, with output streaming into the feed as it happens. Commands are execute-tier: they ask first unless the chat is in Auto.',
    capabilities: ['cargo test --package api', 'pnpm build', 'git status', 'npx playwright test'] },
  { slug: 'web', name: 'Web', status: 'soon', category: 'local', firstParty: true, auth: 'none', runtime: 'native',
    does: 'Fetch pages and read them as text',
    summary: 'Fetches a URL and hands the agent the readable text, so it can check documentation or a changelog without a search service.',
    capabilities: ['fetch_url docs.rs/tokio', 'fetch_url github.com/…/releases', 'extract_links'] },
  { slug: 'github', name: 'GitHub', status: 'available', category: 'developer', auth: 'oauth', runtime: 'mcp-remote', website: 'https://github.com',
    does: 'Repositories, issues and pull requests',
    summary: 'Read code and issues, open and review pull requests, and check runs, from the chat. Opening a pull request is an external write and asks first outside Auto.',
    capabilities: ['search_issues "login timeout"', 'get_file_contents README.md', 'create_pull_request', 'list_workflow_runs'] },
  { slug: 'gitlab', name: 'GitLab', status: 'soon', category: 'developer', auth: 'oauth', runtime: 'mcp-remote', website: 'https://gitlab.com',
    does: 'Projects, issues and merge requests',
    summary: 'The same workflow for GitLab: browse projects, read issues and pipelines, open merge requests.',
    capabilities: ['list_issues', 'get_merge_request !42', 'create_merge_request', 'get_pipeline_status'] },
  { slug: 'linear', name: 'Linear', status: 'soon', category: 'developer', auth: 'oauth', runtime: 'mcp-remote', website: 'https://linear.app',
    does: 'Issues, projects and cycles',
    summary: 'Pull the issue you are working on into the chat, update its status when the work lands, and create follow-ups without leaving the conversation.',
    capabilities: ['get_issue ENG-2498', 'search_issues "sync status"', 'create_issue', 'update_issue status'] },
  { slug: 'jira', name: 'Jira', status: 'soon', category: 'developer', auth: 'oauth', runtime: 'mcp-remote', website: 'https://www.atlassian.com/software/jira',
    does: 'Issues and boards',
    summary: 'Read and update Jira issues and boards; comments and transitions are external writes and follow the permission mode.',
    capabilities: ['get_issue PROJ-1234', 'search_jql "assignee = me"', 'add_comment', 'transition_issue'] },
  { slug: 'sentry', name: 'Sentry', status: 'soon', category: 'developer', auth: 'oauth', runtime: 'mcp-remote', website: 'https://sentry.io',
    does: 'Errors and traces',
    summary: 'Bring a crash into the chat with its stack trace and breadcrumbs, then let the agent find and fix the cause in your repository.',
    capabilities: ['list_issues project=api', 'get_issue_details', 'search_events "TypeError"', 'resolve_issue'] },
  { slug: 'vercel', name: 'Vercel', status: 'soon', category: 'developer', auth: 'oauth', runtime: 'mcp-remote', website: 'https://vercel.com',
    does: 'Deployments and logs',
    summary: 'Check deployments, read build and runtime logs, and inspect a project’s environment from the chat.',
    capabilities: ['list_deployments', 'get_deployment_logs', 'get_project', 'list_domains'] },
  { slug: 'cloudflare-bindings', name: 'Cloudflare Workers', status: 'available', category: 'developer', auth: 'oauth', runtime: 'mcp-remote', icon: 'cloudflare', website: 'https://www.cloudflare.com',
    does: 'Workers with their storage: KV, R2 and D1',
    summary: 'Cloudflare\u2019s own hosted server, connected to your account. List and read Workers, KV namespaces, R2 buckets and D1 databases, and create the bindings a Worker needs. Reads are cheap; anything that creates or changes a resource follows the permission mode.',
    capabilities: ['workers_list', 'kv_namespaces_list', 'r2_buckets_list', 'd1_databases_list'] },
  { slug: 'cloudflare-docs', name: 'Cloudflare Docs', status: 'available', category: 'developer', auth: 'none', runtime: 'mcp-remote', icon: 'cloudflare', website: 'https://developers.cloudflare.com',
    does: 'Search Cloudflare\u2019s live documentation',
    summary: 'The current reference for Workers, Pages, DNS, R2 and D1, read from the live documentation rather than a model\u2019s training. No account, no sign-in, nothing of yours is read.',
    capabilities: ['search_cloudflare_documentation "durable objects alarms"', 'migrate_pages_to_workers_guide'] },
  { slug: 'playwright', name: 'Playwright', status: 'soon', category: 'developer', icon: false, auth: 'none', runtime: 'mcp-stdio', website: 'https://playwright.dev',
    does: 'Drive a browser for tests and checks',
    summary: 'A real browser under the agent’s control: open a page, click through a flow, read what is on screen, take a screenshot. Runs locally on the Node you install.',
    capabilities: ['browser_navigate localhost:3000', 'browser_click "Sign in"', 'browser_snapshot', 'browser_take_screenshot'] },
  { slug: 'docker', name: 'Docker', status: 'soon', category: 'developer', auth: 'none', runtime: 'mcp-stdio', website: 'https://www.docker.com',
    does: 'Containers and images',
    summary: 'List and inspect containers and images, read logs, start and stop services on the Docker you already run.',
    capabilities: ['list_containers', 'container_logs api', 'start_container', 'list_images'] },
  { slug: 'context7', name: 'Context7', status: 'soon', category: 'developer', icon: false, auth: 'none', runtime: 'mcp-remote', website: 'https://context7.com',
    does: 'Current documentation for libraries',
    summary: 'Up-to-date, version-specific documentation for the libraries in your project, so the agent codes against the API you actually use.',
    capabilities: ['resolve_library "tokio"', 'get_library_docs tokio@1.40 "graceful shutdown"'] },
  { slug: 'google-drive', name: 'Google Drive', status: 'soon', category: 'productivity', auth: 'oauth', runtime: 'mcp-remote', website: 'https://drive.google.com',
    does: 'Search and read your files',
    summary: 'Search Drive and read documents, sheets and slides as text. Sign in once with Google; the token stays on your machine.',
    capabilities: ['search_files "Q3 plan"', 'read_document', 'read_sheet "Budget"', 'list_recent'] },
  { slug: 'google-calendar', name: 'Google Calendar', status: 'soon', category: 'productivity', auth: 'oauth', runtime: 'mcp-stdio', website: 'https://calendar.google.com',
    does: 'Events and availability',
    summary: 'Read your calendar, find free time and create events. Creating or moving an event is an external write.',
    capabilities: ['list_events today', 'find_free_time', 'create_event', 'update_event'] },
  { slug: 'gmail', name: 'Gmail', status: 'soon', category: 'productivity', auth: 'oauth', runtime: 'mcp-stdio', website: 'https://mail.google.com',
    does: 'Search, read and draft mail',
    summary: 'Search and read mail, and draft replies for you to send. Sending is always an explicit action.',
    capabilities: ['search_mail "from:alice invoice"', 'read_thread', 'create_draft', 'list_labels'] },
  { slug: 'notion', name: 'Notion', status: 'soon', category: 'productivity', auth: 'oauth', runtime: 'mcp-remote', website: 'https://www.notion.so',
    does: 'Pages and databases',
    summary: 'Search your workspace, read pages and databases, and write new pages or rows when a chat produces something worth keeping.',
    capabilities: ['search "Q3 roadmap"', 'get_page', 'query_database', 'create_page'] },
  { slug: 'figma', name: 'Figma', status: 'soon', category: 'productivity', auth: 'oauth', runtime: 'mcp-remote', website: 'https://www.figma.com',
    does: 'Files, frames and comments',
    summary: 'Read a design file, get the structure and styles of a frame, and pull comments into the chat while implementing it.',
    capabilities: ['get_file', 'get_frame "Checkout"', 'get_styles', 'list_comments'] },
  { slug: 'asana', name: 'Asana', status: 'soon', category: 'productivity', auth: 'oauth', runtime: 'mcp-remote', website: 'https://asana.com',
    does: 'Tasks and projects',
    summary: 'Read projects and tasks, create tasks and update their status from the chat.',
    capabilities: ['list_tasks project=Launch', 'get_task', 'create_task', 'complete_task'] },
  { slug: 'todoist', name: 'Todoist', status: 'soon', category: 'productivity', auth: 'api-key', runtime: 'mcp-stdio', website: 'https://todoist.com',
    does: 'Tasks and due dates',
    summary: 'Your task list in the chat: read today’s tasks, add new ones with due dates, close what is done.',
    capabilities: ['list_tasks today', 'add_task "Review PR" tomorrow', 'complete_task', 'list_projects'] },
  { slug: 'supabase', name: 'Supabase', status: 'soon', category: 'data', auth: 'oauth', runtime: 'mcp-remote', website: 'https://supabase.com',
    does: 'Projects, tables and SQL',
    summary: 'Inspect projects and tables, run SQL, read logs and apply migrations. Anything that changes data is an external or destructive write and asks accordingly.',
    capabilities: ['list_tables', 'execute_sql select count(*) from users', 'get_logs api', 'apply_migration'] },
  { slug: 'postgresql', name: 'PostgreSQL', status: 'soon', category: 'data', auth: 'connection-string', runtime: 'mcp-stdio', website: 'https://www.postgresql.org',
    does: 'Query any Postgres database',
    summary: 'Point it at a connection string and the agent can explore the schema and run queries. Read-only by default; writes are a separate, explicit setting.',
    capabilities: ['list_schemas', 'describe_table orders', 'query select …', 'explain analyze …'] },
  { slug: 'mongodb', name: 'MongoDB', status: 'soon', category: 'data', auth: 'connection-string', runtime: 'mcp-stdio', website: 'https://www.mongodb.com',
    does: 'Collections and queries',
    summary: 'Browse databases and collections, run finds and aggregations, and inspect indexes on any MongoDB you can reach.',
    capabilities: ['list_collections', 'find orders {status:"open"}', 'aggregate', 'collection_indexes'] },
  { slug: 'neon', name: 'Neon', status: 'soon', category: 'data', auth: 'oauth', runtime: 'mcp-remote', website: 'https://neon.com',
    does: 'Serverless Postgres branches',
    summary: 'Projects, branches and SQL on Neon. Branching a database for an experiment is one tool call.',
    capabilities: ['list_projects', 'create_branch', 'run_sql', 'describe_table'] },
  { slug: 'airtable', name: 'Airtable', status: 'soon', category: 'data', auth: 'oauth', runtime: 'mcp-remote', website: 'https://airtable.com',
    does: 'Bases, tables and records',
    summary: 'Read bases and tables, search records, and create or update rows when a chat needs to.',
    capabilities: ['list_bases', 'list_records Contacts', 'search_records', 'create_record'] },
  { slug: 'stripe', name: 'Stripe', status: 'soon', category: 'data', auth: 'oauth', runtime: 'mcp-remote', website: 'https://stripe.com',
    does: 'Customers, payments and invoices',
    summary: 'Look up customers, payments and invoices, and answer questions about your Stripe account. Anything that moves money asks first, always.',
    capabilities: ['list_customers', 'retrieve_payment_intent', 'list_invoices', 'create_refund'] },
  { slug: 'slack', name: 'Slack', status: 'soon', category: 'communication', icon: false, auth: 'oauth', runtime: 'mcp-remote', website: 'https://slack.com',
    does: 'Channels, messages and search',
    summary: 'Read channels and threads, search history, and post messages. Posting is an external write and asks first outside Auto.',
    capabilities: ['list_channels', 'read_thread #releases', 'search_messages "deploy failed"', 'post_message'] },
  { slug: 'discord', name: 'Discord', status: 'soon', category: 'communication', auth: 'api-key', runtime: 'mcp-stdio', website: 'https://discord.com',
    does: 'Servers and channels',
    summary: 'Read and post in the servers your bot can see; useful for community support and release announcements.',
    capabilities: ['list_channels', 'read_messages #support', 'send_message #releases', 'search'] },
  { slug: 'microsoft-teams', name: 'Microsoft Teams', status: 'soon', category: 'communication', icon: false, auth: 'oauth', runtime: 'mcp-stdio', website: 'https://www.microsoft.com/microsoft-teams',
    does: 'Teams, channels and chats',
    summary: 'Read teams, channels and chats, and post messages, with your Microsoft account.',
    capabilities: ['list_teams', 'read_channel', 'send_message', 'search_messages'] },
  { slug: 'brave-search', name: 'Brave Search', status: 'soon', category: 'web', icon: 'brave', auth: 'api-key', runtime: 'mcp-stdio', website: 'https://brave.com/search/api/',
    does: 'Web search with an API key',
    summary: 'Web and news search from an independent index. Bring a Brave Search API key; results come back as titles, snippets and links the agent can then fetch.',
    capabilities: ['web_search "tokio graceful shutdown"', 'news_search', 'local_search'] },
  { slug: 'exa', name: 'Exa', status: 'soon', category: 'web', icon: false, auth: 'api-key', runtime: 'mcp-remote', website: 'https://exa.ai',
    does: 'Neural search for research',
    summary: 'Search that understands meaning rather than keywords, with page contents included, for research-heavy chats.',
    capabilities: ['search "papers on speculative decoding"', 'find_similar', 'get_contents'] },
  { slug: 'firecrawl', name: 'Firecrawl', status: 'soon', category: 'web', icon: false, auth: 'api-key', runtime: 'mcp-remote', website: 'https://firecrawl.dev',
    does: 'Crawl sites into clean text',
    summary: 'Turn a site or a page into clean Markdown the agent can read, including JavaScript-rendered pages.',
    capabilities: ['scrape url', 'crawl docs.example.com', 'map site', 'extract'] },
  { slug: 'tavily', name: 'Tavily', status: 'soon', category: 'web', icon: false, auth: 'api-key', runtime: 'mcp-remote', website: 'https://tavily.com',
    does: 'Search built for agents',
    summary: 'A search API made for agents: concise, ranked results with extracted content, tuned for answering questions.',
    capabilities: ['search "…"', 'extract url', 'qna_search'] },
];

export const AUTH_LABEL: Record<Auth, string> = {
  none: 'No sign-in needed',
  'api-key': 'API key you provide',
  oauth: 'Sign in with your account (OAuth)',
  'connection-string': 'Connection string you provide',
};
export const RUNTIME_LABEL: Record<Runtime, string> = {
  native: 'Built into Gantry',
  'mcp-remote': 'Remote MCP server, reached over HTTPS',
  'mcp-stdio': 'MCP server run locally on your machine',
};

export const STATUS_LABEL: Record<Status, string> = { available: 'Available now', soon: 'Coming soon' };

export const available = connectors.filter((c) => c.status === 'available');
export const soon = connectors.filter((c) => c.status === 'soon');

export function bySlug(slug: string): Connector | undefined {
  return connectors.find((c) => c.slug === slug);
}
