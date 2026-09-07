import { CATEGORIES, connectors, type CategoryId, type Connector } from '@/data/connectors';
import { REPO_URL } from './seo';

export const REQUEST_URL = `${REPO_URL}/issues/new?template=connector-request.yml&title=${encodeURIComponent('Connector request: ')}`;

export function categoryLabel(id: CategoryId): string { return CATEGORIES[id].label; }
export function related(c: Connector, n = 4): Connector[] {
  return connectors.filter((o) => o.category === c.category && o.slug !== c.slug).slice(0, n);
}
export const kindLabel = (c: Connector) => (c.firstParty ? 'Built in' : 'MCP');
