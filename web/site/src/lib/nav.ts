/** The site map, used by the navbar menus, the mobile sheet and the footer. One place to add a page. */
export interface NavLink { label: string; href: string; hint?: string; external?: boolean }

export const productMenu: NavLink[] = [
  { label: 'Product tour', href: '/product/', hint: 'Chat, agent and connectors in one window' },
  { label: 'Connectors', href: '/connectors/', hint: 'Built-in tools and MCP servers' },
  { label: 'Pricing', href: '/pricing/', hint: 'Free. Bring your own keys' },
  { label: 'Download', href: '/download/', hint: 'Windows and Linux' },
  { label: 'Changelog', href: '/changelog/', hint: 'What shipped, release by release' },
];

export const resourcesMenu: NavLink[] = [
  { label: 'Docs', href: '/docs/', hint: 'Install, add a key, connect' },
  { label: 'Blog', href: '/blog/', hint: 'Notes from building Gantry' },
  { label: 'Security', href: '/security/', hint: 'Keys, permissions and the guard' },
  { label: 'About', href: '/about/', hint: 'The project and its licence' },
  { label: 'GitHub', href: 'https://github.com/oljodev/gantry', hint: 'Source, issues and discussions', external: true },
];

export const footerColumns: { title: string; links: NavLink[] }[] = [
  { title: 'Product', links: productMenu },
  { title: 'Resources', links: resourcesMenu.filter((l) => !l.external) },
  {
    title: 'Project',
    links: [
      { label: 'GitHub', href: 'https://github.com/oljodev/gantry', external: true },
      { label: 'Issues', href: 'https://github.com/oljodev/gantry/issues', external: true },
      { label: 'Discussions', href: 'https://github.com/oljodev/gantry/discussions', external: true },
      { label: 'License', href: '/license/' },
      { label: 'Privacy', href: '/privacy/' },
    ],
  },
];
