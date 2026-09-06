import rss from '@astrojs/rss';
import { getCollection } from 'astro:content';
import type { APIContext } from 'astro';

export async function GET(context: APIContext) {
  const entries = (await getCollection('changelog', ({ data }) => !data.draft)).sort((a, b) => b.data.date.valueOf() - a.data.date.valueOf());
  return rss({
    title: 'Gantry releases',
    description: 'What shipped in each Gantry release.',
    site: context.site!,
    trailingSlash: true,
    items: entries.map((e) => ({ title: e.data.title ? `${e.data.title} (v${e.data.version})` : `Gantry v${e.data.version}`, description: e.data.highlights.join('. '), pubDate: e.data.date, link: `/changelog/${e.id}/` })),
  });
}
