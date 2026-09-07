import { defineCollection } from 'astro:content';
import { z } from 'astro/zod';
import { glob } from 'astro/loaders';
import { docsLoader } from '@astrojs/starlight/loaders';
import { docsSchema } from '@astrojs/starlight/schema';

/** Posts: src/content/blog/<yyyy-mm-dd>-<slug>.md(x). Files starting with "_" are ignored; `draft: true` hides a post. */
const blog = defineCollection({
  loader: glob({ pattern: '**/[^_]*.{md,mdx}', base: './src/content/blog', generateId: ({ entry }) => entry.replace(/\.(md|mdx)$/, '').replace(/^\d{4}-\d{2}-\d{2}-/, '') }),
  schema: z.object({
    title: z.string(),
    description: z.string().max(220),
    pubDate: z.coerce.date(),
    updatedDate: z.coerce.date().optional(),
    tags: z.array(z.string()).default([]),
    draft: z.boolean().default(false),
  }),
});

/** Releases: src/content/changelog/<yyyy-mm-dd>-<version>.md. The id is the version with dots as dashes. */
const changelog = defineCollection({
  loader: glob({ pattern: '**/[^_]*.{md,mdx}', base: './src/content/changelog', generateId: ({ entry }) => entry.replace(/\.(md|mdx)$/, '').replace(/^\d{4}-\d{2}-\d{2}-/, '').replace(/\./g, '-') }),
  schema: z.object({
    version: z.string(),
    date: z.coerce.date(),
    title: z.string().optional(),
    highlights: z.array(z.string()).default([]),
    draft: z.boolean().default(false),
  }),
});

export const collections = { blog, changelog, docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }) };
