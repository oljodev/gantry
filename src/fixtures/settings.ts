import type { Provider } from '@/fixtures/types';

export interface ProviderState {
  id: Provider;
  name: string;
  key: { present: boolean; hint?: string; invalid?: boolean };
  baseUrl?: string;
  defaultModel?: string;
  models: { id: string; label: string; context: string; price?: string }[];
}

export const providers: ProviderState[] = [
  {
    id: 'anthropic',
    name: 'Anthropic',
    key: { present: true, hint: 'abcd' },
    defaultModel: 'claude-opus-5',
    models: [
      { id: 'claude-opus-5', label: 'Claude Opus 5', context: '1M', price: '$15 / $75' },
      { id: 'claude-sonnet-5', label: 'Claude Sonnet 5', context: '1M', price: '$3 / $15' },
      { id: 'claude-haiku-4-5', label: 'Claude Haiku 4.5', context: '200k', price: '$1 / $5' },
    ],
  },
  { id: 'openai', name: 'OpenAI', key: { present: false }, models: [] },
  { id: 'gemini', name: 'Google Gemini', key: { present: false }, models: [] },
  { id: 'xai', name: 'xAI', key: { present: false }, baseUrl: 'https://api.x.ai/v1', models: [] },
  {
    id: 'openrouter',
    name: 'OpenRouter',
    key: { present: true, hint: '9f2e', invalid: true },
    baseUrl: 'https://openrouter.ai/api/v1',
    models: [],
  },
];

export const PROVIDER_LABEL: Record<Provider, string> = {
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  gemini: 'Google Gemini',
  xai: 'xAI',
  openrouter: 'OpenRouter',
};
