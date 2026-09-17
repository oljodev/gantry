import type { MediaOptions, ModelCapabilities, ModelInfo, ModelRef } from '@/bindings';
import type { CatalogProvider } from '@/lib/ipc/hooks/providers';

/**
 * The catalog as the model dialog reads it (docs/plan/15 §7): one flat list of models, each
 * carrying the facts a person chooses on — who made it, what it produces, what it takes in, how
 * much room it has and what it costs — so the dialog only filters and sorts.
 */

/**
 * What a model produces. The kinds Gantry means to support; speech-to-text is not one.
 *
 * `speech` and `audio` are two different things and both were asked for: `speech` is a
 * text-to-speech model that reads a passage aloud, `audio` a model that answers in sound or
 * writes music as part of a conversation. The provider draws the same line.
 */
export type ModelKind = 'text' | 'image' | 'speech' | 'audio' | 'video';

export const KIND_LABEL: Record<ModelKind, string> = {
  text: 'Text',
  image: 'Image',
  speech: 'Speech',
  audio: 'Music & audio',
  video: 'Video',
};

export interface CatalogModel {
  /** `provider/model`, unique across providers and stable enough to store. */
  key: string;
  ref: ModelRef;
  /** The model's own id, as the provider spells it. */
  id: string;
  /** The name without its creator, which is shown beside it. */
  name: string;
  creator: string;
  providerId: string;
  providerLabel: string;
  kind: ModelKind;
  info: ModelInfo;
}

export function modelKey(ref: ModelRef): string {
  return `${ref.provider}/${ref.model}`;
}

/**
 * What a model makes. A model that answers with text *and* pictures is an image model here:
 * being able to produce a picture is the thing a person is looking for when they filter.
 */
export function kindOf(caps: ModelCapabilities | undefined): ModelKind {
  const out = caps?.output ?? [];
  if (out.includes('video')) return 'video';
  if (out.includes('speech')) return 'speech';
  if (out.includes('image')) return 'image';
  if (out.includes('audio')) return 'audio';
  return 'text';
}

/** Names the app spells differently from the id (`x-ai` is xAI, not X Ai). */
const CREATORS: Record<string, string> = {
  'x-ai': 'xAI',
  openai: 'OpenAI',
  meta: 'Meta',
  'meta-llama': 'Meta',
  mistralai: 'Mistral',
  deepseek: 'DeepSeek',
  ai21: 'AI21',
  'z-ai': 'Z.AI',
  nvidia: 'NVIDIA',
  thudm: 'THUDM',
  nousresearch: 'Nous Research',
  cognitivecomputations: 'Cognitive Computations',
};

/**
 * Who made the model. OpenRouter writes it twice — as the id's first segment and as the part of
 * the name before the colon — and the name is the one with the capitals a person expects, so it
 * wins where it exists.
 */
export function creatorOf(info: ModelInfo): string {
  const colon = info.display_name.indexOf(': ');
  if (colon > 0) return info.display_name.slice(0, colon);
  const slug = info.id.includes('/') ? info.id.slice(0, info.id.indexOf('/')) : '';
  if (!slug) return 'Other';
  return CREATORS[slug] ?? slug.charAt(0).toUpperCase() + slug.slice(1);
}

/** The model's name with the creator taken off the front, which the row shows separately. */
export function nameOf(info: ModelInfo): string {
  const colon = info.display_name.indexOf(': ');
  return colon > 0 ? info.display_name.slice(colon + 2) : info.display_name;
}

export function toCatalog(providers: CatalogProvider[]): CatalogModel[] {
  return providers.flatMap((p) =>
    p.models.map((info) => ({
      key: `${p.id}/${info.id}`,
      ref: { provider: p.id, model: info.id },
      id: info.id,
      name: nameOf(info),
      creator: creatorOf(info),
      providerId: p.id,
      providerLabel: p.label,
      kind: kindOf(info.capabilities),
      info,
    })),
  );
}

/** A thing a model has to be able to do, in the words the dialog uses. */
export type Need = 'vision' | 'files' | 'tools' | 'reasoning' | 'caching';

export const NEED_LABEL: Record<Need, string> = {
  vision: 'Reads images',
  files: 'Reads files',
  tools: 'Calls tools',
  reasoning: 'Thinks first',
  caching: 'Caches prompts',
};

export function meets(caps: ModelCapabilities | undefined, need: Need): boolean {
  switch (need) {
    case 'vision':
      return caps?.vision === true || (caps?.input ?? []).includes('image');
    case 'files':
      return caps?.pdf_input === true || (caps?.input ?? []).includes('file');
    case 'tools':
      return caps?.tools === true;
    case 'reasoning':
      return (caps?.reasoning?.kind ?? 'none') !== 'none';
    case 'caching':
      return (caps?.prompt_caching ?? 'none') !== 'none';
  }
}

/** How far back a model may have been released, in days; `null` is any age. */
export type MaxAge = 30 | 90 | 180 | 365 | null;

export const AGES: { days: MaxAge; label: string }[] = [
  { days: 30, label: 'Last month' },
  { days: 90, label: 'Last 3 months' },
  { days: 180, label: 'Last 6 months' },
  { days: 365, label: 'Last year' },
  { days: null, label: 'Any age' },
];

export interface Filters {
  query: string;
  /** Empty means every kind; likewise for creators and providers. */
  kinds: ModelKind[];
  creators: string[];
  needs: Need[];
  freeOnly: boolean;
  maxAgeDays: MaxAge;
}

/**
 * Whether a model is new enough. A model whose provider never dated it cannot answer the
 * question, so it drops out of an age filter rather than being assumed recent — the filter is
 * asked precisely when the old ones are in the way.
 */
export function withinAge(info: ModelInfo, days: MaxAge, now = Date.now()): boolean {
  if (days === null) return true;
  if (!info.created_at) return false;
  return now - info.created_at * 1000 <= days * 24 * 60 * 60 * 1000;
}

/** `3 mo`, `2 y`, or nothing when the provider never dated the model. */
export function ageLabel(info: ModelInfo, now = Date.now()): string {
  if (!info.created_at) return '';
  const days = Math.floor((now - info.created_at * 1000) / (24 * 60 * 60 * 1000));
  if (days < 1) return 'today';
  if (days < 31) return `${days} d`;
  if (days < 365) return `${Math.round(days / 30)} mo`;
  const years = days / 365;
  return `${years < 10 ? years.toFixed(1) : Math.round(years)} y`;
}

export const NO_FILTERS: Filters = {
  query: '',
  kinds: [],
  creators: [],
  needs: [],
  freeOnly: false,
  maxAgeDays: null,
};

export function isFiltered(f: Filters): boolean {
  return (
    f.query.trim().length > 0 ||
    f.kinds.length > 0 ||
    f.creators.length > 0 ||
    f.needs.length > 0 ||
    f.freeOnly ||
    f.maxAgeDays !== null
  );
}

/**
 * Priced, and priced at nothing. A model whose price nobody stated is unknown, not free — and
 * zero counts only when it is the price of the thing the model makes. Every video model reports
 * nothing but `0` token prices and still bills by the second, so "Free" there would be a lie
 * told by arithmetic.
 */
export function isFree(m: CatalogModel): boolean {
  const p = m.info.pricing;
  if (!p) return false;
  if (m.kind === 'video') {
    const rates = rateList(p.video_per_second_usd);
    return rates.length > 0 && rates.every((r) => r === 0);
  }
  if (m.kind === 'image' && typeof p.image_output_per_mtok === 'number') {
    return p.image_output_per_mtok === 0;
  }
  if (m.kind === 'audio' && typeof p.audio_output_per_mtok === 'number') {
    return p.audio_output_per_mtok === 0;
  }
  if (p.input_per_mtok === null || p.output_per_mtok === null) return false;
  return p.input_per_mtok === 0 && p.output_per_mtok === 0;
}

export function matches(m: CatalogModel, f: Filters): boolean {
  const q = f.query.trim().toLowerCase();
  if (q) {
    const haystack = `${m.id} ${m.info.display_name} ${m.creator} ${m.providerLabel}`.toLowerCase();
    // Every word has to appear somewhere, so "deep flash" finds DeepSeek V4 Flash.
    if (!q.split(/\s+/).every((word) => haystack.includes(word))) return false;
  }
  if (f.kinds.length > 0 && !f.kinds.includes(m.kind)) return false;
  if (f.creators.length > 0 && !f.creators.includes(m.creator)) return false;
  if (f.needs.some((need) => !meets(m.info.capabilities, need))) return false;
  if (f.freeOnly && !isFree(m)) return false;
  if (!withinAge(m.info, f.maxAgeDays)) return false;
  return true;
}

export type Sort = 'newest' | 'name' | 'price' | 'context';

export const SORT_LABEL: Record<Sort, string> = {
  newest: 'Newest first',
  name: 'Name',
  price: 'Cheapest first',
  context: 'Largest context',
};

/** What one million input tokens plus one million output tokens costs; unpriced sorts last. */
export function priceOf(info: ModelInfo): number {
  const p = info.pricing;
  if (!p || (p.input_per_mtok === null && p.output_per_mtok === null)) {
    return Number.POSITIVE_INFINITY;
  }
  return (p.input_per_mtok ?? 0) + (p.output_per_mtok ?? 0);
}

export function sortModels(models: CatalogModel[], sort: Sort): CatalogModel[] {
  const byName = (a: CatalogModel, b: CatalogModel) =>
    a.creator.localeCompare(b.creator) || a.name.localeCompare(b.name);
  return [...models].sort((a, b) => {
    // Newest first, and an undated model sorts to the end rather than to 1970.
    if (sort === 'newest') {
      return (b.info.created_at ?? 0) - (a.info.created_at ?? 0) || byName(a, b);
    }
    if (sort === 'price') return priceOf(a.info) - priceOf(b.info) || byName(a, b);
    if (sort === 'context')
      return (b.info.context_window ?? 0) - (a.info.context_window ?? 0) || byName(a, b);
    return byName(a, b);
  });
}

/**
 * A price in the range money is normally written in. `usd` keeps three and four decimals
 * because a token price of $0.089 needs them; a clip that costs $0.10 a second does not, and
 * "$0.100" reads like a rounding error rather than a price.
 */
function usdCents(value: number): string {
  return value < 0.01 ? usd(value) : `$${value.toFixed(2)}`;
}

/** The per-second rates a model states, with the blanks dropped. */
function rateList(rates: Record<string, number | null> | undefined | null): number[] {
  return Object.values(rates ?? {}).filter((r): r is number => typeof r === 'number');
}

/** Dollars per second of video at a resolution, falling back to the model's flat rate. */
export function perSecond(m: CatalogModel, resolution?: string | null): number | null {
  const rates = m.info.pricing?.video_per_second_usd;
  if (!rates) return null;
  const at = resolution ? rates[resolution] : undefined;
  const flat = rates[''];
  const only = rateList(rates);
  const price = at ?? flat ?? (only.length === 1 ? only[0] : undefined);
  return price ?? null;
}

/**
 * What the clip as configured will cost, in the words of the thing being bought. Shown next to
 * the controls that decide it, because seconds × resolution is exactly where the money goes and
 * a person setting 10 seconds of 1080p deserves to see the number before they send.
 */
export function clipEstimate(m: CatalogModel, options: MediaOptions | undefined): string {
  if (m.kind !== 'video') return '';
  const seconds = options?.duration_seconds ?? m.info.capabilities?.durations?.[0];
  const resolution = options?.resolution ?? null;
  const rate = perSecond(m, resolution);
  if (!seconds || rate === null) return '';
  const where = resolution ? ` at ${resolution}` : '';
  return `${seconds} s${where} ≈ ${usdCents(rate * seconds)}`;
}

/** Creators present in a list, with how many models each has, most first. */
export function creatorsOf(models: CatalogModel[]): { name: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const m of models) counts.set(m.creator, (counts.get(m.creator) ?? 0) + 1);
  return [...counts]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name));
}

/** `1M`, `131K`, or nothing when the provider never said. */
export function contextLabel(n: number | null | undefined): string {
  if (!n) return '';
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
  if (n >= 1000) return `${Math.round(n / 1000)}K`;
  return String(n);
}

/** A dollar figure at the precision it deserves: cheap models need the decimals. */
export function usd(value: number): string {
  if (value === 0) return '$0';
  if (value < 0.01) return `$${value.toFixed(4)}`;
  if (value < 1) return `$${value.toFixed(3)}`;
  if (value < 100) return `$${value.toFixed(2)}`;
  return `$${Math.round(value)}`;
}

/**
 * What the row shows about price: token prices for a model that answers in text, the price of a
 * picture for one that draws, and the price of sound for one that talks, because a
 * per-million-token text price says nothing about any of those.
 */
export function priceLabel(m: CatalogModel): string {
  const p = m.info.pricing;
  if (!p) return '—';
  // A clip is priced by the second, at a rate that depends on the resolution, so the row shows
  // the span and the options strip works out what the chosen clip comes to.
  if (m.kind === 'video') {
    const rates = rateList(p.video_per_second_usd).sort((a, b) => a - b);
    if (rates.length === 0) return '—';
    const low = rates[0] as number;
    const high = rates[rates.length - 1] as number;
    return low === high ? `${usdCents(low)} / s` : `${usdCents(low)}–${usdCents(high)} / s`;
  }
  // Per million output tokens, not per picture: the provider prices the picture's own tokens
  // and never says how many a picture comes to. It used to read `$0.0000 / image`, which is
  // what a $30-per-million rate looks like when it is printed as the price of one image.
  if (m.kind === 'image' && p.image_output_per_mtok) {
    return `${usd(p.image_output_per_mtok)} / M drawn`;
  }
  if (m.kind === 'audio' && p.audio_output_per_mtok) {
    return `${usd(p.audio_output_per_mtok)} / M spoken`;
  }
  // A text-to-speech model is billed by the character it reads, not by the token it was sent:
  // the provider reports it in the same field, and the unit matters when the two differ by an
  // order of magnitude.
  if (m.kind === 'speech' && p.input_per_mtok !== null) {
    return `${usd(p.input_per_mtok)} / M chars`;
  }
  if (p.request_usd && !p.input_per_mtok && !p.output_per_mtok) {
    return `${usd(p.request_usd)} / call`;
  }
  if (isFree(m)) return 'Free';
  if (p.input_per_mtok === null && p.output_per_mtok === null) return '—';
  return `${usd(p.input_per_mtok ?? 0)} / ${usd(p.output_per_mtok ?? 0)}`;
}
