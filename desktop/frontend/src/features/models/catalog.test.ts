import { describe, expect, it } from 'vitest';

import type { ModelCapabilities, ModelInfo } from '@/bindings';
import {
  contextLabel,
  creatorOf,
  creatorsOf,
  isFree,
  kindOf,
  matches,
  meets,
  nameOf,
  priceLabel,
  sortModels,
  toCatalog,
  type CatalogModel,
  type Filters,
} from './catalog';

/** The catalog's own defaults, so a test says only what it is about. */
function caps(over: Partial<ModelCapabilities> = {}): ModelCapabilities {
  return {
    input: ['text'],
    output: ['text'],
    tools: false,
    parallel_tools: false,
    streams_tool_args: false,
    vision: false,
    pdf_input: false,
    reasoning: { kind: 'none' },
    server_web_search: false,
    structured_output: false,
    prompt_caching: 'none',
    ...over,
  };
}

function info(over: Partial<ModelInfo> & { id: string }): ModelInfo {
  return {
    display_name: over.id,
    context_window: null,
    max_output: null,
    pricing: null,
    ...over,
    capabilities: over.capabilities ?? caps(),
  };
}

function model(over: Partial<ModelInfo> & { id: string }): CatalogModel {
  return toCatalog([
    {
      id: 'openrouter',
      label: 'OpenRouter',
      hasKey: true,
      available: true,
      loading: false,
      models: [info(over)],
    },
  ])[0]!;
}

const ALL: Filters = { query: '', kinds: [], creators: [], needs: [], freeOnly: false };

describe('creator and name', () => {
  it('takes both from the name when it carries the creator', () => {
    const m = info({ id: 'deepseek/deepseek-v4-flash', display_name: 'DeepSeek: V4 Flash 0423' });
    expect(creatorOf(m)).toBe('DeepSeek');
    expect(nameOf(m)).toBe('V4 Flash 0423');
  });

  it('falls back to the id, spelled the way the maker spells it', () => {
    expect(creatorOf(info({ id: 'x-ai/grok-4' }))).toBe('xAI');
    expect(creatorOf(info({ id: 'qwen/qwen3-72b' }))).toBe('Qwen');
  });

  it('counts creators, most models first', () => {
    const models = [
      model({ id: 'a/one', display_name: 'A: One' }),
      model({ id: 'a/two', display_name: 'A: Two' }),
      model({ id: 'b/one', display_name: 'B: One' }),
    ];
    expect(creatorsOf(models)).toEqual([
      { name: 'A', count: 2 },
      { name: 'B', count: 1 },
    ]);
  });
});

describe('kind', () => {
  it('is the thing the model produces, and a picture wins over the text beside it', () => {
    expect(kindOf(caps({ output: ['text'] }))).toBe('text');
    expect(kindOf(caps({ output: ['text', 'image'] }))).toBe('image');
    expect(kindOf(caps({ output: ['audio'] }))).toBe('audio');
  });

  it('is text for a model the catalog never described', () => {
    expect(kindOf(undefined)).toBe('text');
    expect(kindOf(caps({ output: [] }))).toBe('text');
  });
});

describe('needs', () => {
  it('reads both the flag and the modality', () => {
    expect(meets(caps({ vision: true }), 'vision')).toBe(true);
    expect(meets(caps({ input: ['text', 'image'] }), 'vision')).toBe(true);
    expect(meets(caps({ input: ['text'] }), 'vision')).toBe(false);
    expect(meets(caps({ reasoning: { kind: 'none' } }), 'reasoning')).toBe(false);
    expect(meets(caps({ reasoning: { kind: 'effort' } }), 'reasoning')).toBe(true);
  });
});

describe('matches', () => {
  const flash = model({
    id: 'deepseek/deepseek-v4-flash',
    display_name: 'DeepSeek: V4 Flash 0423',
    capabilities: caps({ tools: true }),
  });

  it('finds a model from words in any order', () => {
    expect(matches(flash, { ...ALL, query: 'flash deep' })).toBe(true);
    expect(matches(flash, { ...ALL, query: 'deepseek v4' })).toBe(true);
    expect(matches(flash, { ...ALL, query: 'claude' })).toBe(false);
  });

  it('filters by kind, creator and capability', () => {
    expect(matches(flash, { ...ALL, kinds: ['image'] })).toBe(false);
    expect(matches(flash, { ...ALL, kinds: ['text'] })).toBe(true);
    expect(matches(flash, { ...ALL, creators: ['DeepSeek'] })).toBe(true);
    expect(matches(flash, { ...ALL, creators: ['Google'] })).toBe(false);
    expect(matches(flash, { ...ALL, needs: ['tools'] })).toBe(true);
    expect(matches(flash, { ...ALL, needs: ['vision'] })).toBe(false);
  });

  it('treats a priced model as not free, and an unpriced one as unknown rather than free', () => {
    const priced = model({
      id: 'a/b',
      pricing: { input_per_mtok: 1, output_per_mtok: 2, cache_read_per_mtok: null },
    });
    const zero = model({
      id: 'c/d',
      pricing: { input_per_mtok: 0, output_per_mtok: 0, cache_read_per_mtok: null },
    });
    expect(isFree(priced.info)).toBe(false);
    expect(isFree(zero.info)).toBe(true);
    expect(isFree(flash.info)).toBe(false);
    expect(matches(priced, { ...ALL, freeOnly: true })).toBe(false);
    expect(matches(zero, { ...ALL, freeOnly: true })).toBe(true);
  });
});

describe('sorting', () => {
  const cheap = model({
    id: 'a/cheap',
    display_name: 'A: Cheap',
    context_window: 8000,
    pricing: { input_per_mtok: 0.1, output_per_mtok: 0.2, cache_read_per_mtok: null },
  });
  const roomy = model({
    id: 'b/roomy',
    display_name: 'B: Roomy',
    context_window: 1_000_000,
    pricing: { input_per_mtok: 3, output_per_mtok: 15, cache_read_per_mtok: null },
  });
  const unpriced = model({ id: 'c/unpriced', display_name: 'C: Unpriced', context_window: 32000 });

  it('puts the cheapest first and the unpriced last', () => {
    expect(sortModels([roomy, unpriced, cheap], 'price').map((m) => m.id)).toEqual([
      'a/cheap',
      'b/roomy',
      'c/unpriced',
    ]);
  });

  it('puts the largest context first', () => {
    expect(sortModels([cheap, roomy, unpriced], 'context').map((m) => m.id)).toEqual([
      'b/roomy',
      'c/unpriced',
      'a/cheap',
    ]);
  });

  it('sorts by creator then name', () => {
    expect(sortModels([roomy, cheap], 'name').map((m) => m.id)).toEqual(['a/cheap', 'b/roomy']);
  });
});

describe('labels', () => {
  it('shortens the context window', () => {
    expect(contextLabel(1_048_576)).toBe('1.0M');
    expect(contextLabel(131_072)).toBe('131K');
    expect(contextLabel(null)).toBe('');
  });

  it('prices a text model per million tokens and an image model per picture', () => {
    const text = model({
      id: 'a/b',
      pricing: { input_per_mtok: 0.088606, output_per_mtok: 0.177212, cache_read_per_mtok: null },
    });
    expect(priceLabel(text)).toBe('$0.089 / $0.177');
    const image = model({
      id: 'g/image',
      capabilities: caps({ output: ['text', 'image'] }),
      pricing: {
        input_per_mtok: 0.3,
        output_per_mtok: 2.5,
        cache_read_per_mtok: null,
        image_output_usd: 0.03,
      },
    });
    expect(priceLabel(image)).toBe('$0.030 / image');
    expect(priceLabel(model({ id: 'x/y' }))).toBe('—');
  });
});
