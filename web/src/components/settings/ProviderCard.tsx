import { useState } from 'react'
import { Check, X } from 'lucide-react'
import { createProvider, deleteProvider, testProvider } from '../../api/client'
import type { Provider, ProviderTestResult, ProviderType } from '../../api/types'
import { field } from '../forms'

// eslint-disable-next-line react-refresh/only-export-components
export const PROVIDER_LABELS: Record<ProviderType, string> = {
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  google: 'Google',
  openrouter: 'OpenRouter',
  local: 'Local (OpenAI-compatible)',
}

const MODEL_PLACEHOLDERS: Record<ProviderType, string> = {
  anthropic: 'claude-opus-4-8',
  openai: 'gpt-5',
  google: 'gemini-2.5-pro',
  openrouter: 'qwen/qwen3-coder',
  local: 'qwen3:32b',
}

const needsBaseUrl = (type: ProviderType) => type === 'local'
const showsBaseUrl = (type: ProviderType) => type === 'local' || type === 'openrouter'

/** An existing provider (read-only summary + test/delete) or a draft form. */
export function ProviderCard({
  provider,
  draftType,
  onChanged,
}: {
  provider?: Provider
  draftType?: ProviderType
  onChanged: () => void
}) {
  const type = provider?.provider_type ?? draftType ?? 'anthropic'
  const [name, setName] = useState(provider?.name ?? PROVIDER_LABELS[type])
  const [apiKey, setApiKey] = useState('')
  const [baseUrl, setBaseUrl] = useState(provider?.base_url ?? '')
  const [defaultModel, setDefaultModel] = useState(provider?.default_model ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [test, setTest] = useState<ProviderTestResult | null>(null)

  const save = async () => {
    setBusy(true)
    setError(null)
    try {
      await createProvider({
        name: name.trim(),
        provider_type: type,
        api_key: apiKey.trim() || undefined,
        base_url: baseUrl.trim() || undefined,
        default_model: defaultModel.trim(),
      })
      onChanged()
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(false)
    }
  }

  const remove = async () => {
    if (!provider) return
    setBusy(true)
    try {
      await deleteProvider(provider.id)
      onChanged()
    } catch (err) {
      setError(String(err))
      setBusy(false)
    }
  }

  const probe = async () => {
    if (!provider) return
    setTest(null)
    setBusy(true)
    try {
      setTest(await testProvider(provider.id))
    } catch (err) {
      setError(String(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-900/40 p-4">
      <div className="mb-3 flex items-center gap-2">
        <span className="rounded bg-zinc-800 px-2 py-0.5 font-mono text-xs text-zinc-300">
          {type}
        </span>
        {provider ? (
          <span className="text-sm font-semibold">{provider.name}</span>
        ) : (
          <input
            className={`${field} max-w-48`}
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="display name"
          />
        )}
        <span className="grow" />
        {provider && provider.api_key_last4 && (
          <span className="font-mono text-xs text-zinc-500">key •••• {provider.api_key_last4}</span>
        )}
      </div>

      {provider ? (
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-zinc-500">
          <span>
            model <span className="font-mono text-zinc-300">{provider.default_model}</span>
          </span>
          {provider.base_url && (
            <span>
              base <span className="font-mono text-zinc-300">{provider.base_url}</span>
            </span>
          )}
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          <input
            type="password"
            className={`${field} font-mono`}
            placeholder={type === 'local' ? 'API key (optional for local servers)' : 'API key'}
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            autoComplete="off"
          />
          {showsBaseUrl(type) && (
            <input
              className={`${field} font-mono`}
              placeholder={
                type === 'local'
                  ? 'base URL, e.g. http://localhost:11434/v1'
                  : 'base URL (optional)'
              }
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
            />
          )}
          <input
            className={`${field} font-mono`}
            placeholder={`default model, e.g. ${MODEL_PLACEHOLDERS[type]}`}
            value={defaultModel}
            onChange={(e) => setDefaultModel(e.target.value)}
          />
        </div>
      )}

      <div className="mt-3 flex items-center gap-2 text-sm">
        {!provider && (
          <button
            onClick={() => void save()}
            disabled={
              busy || !name.trim() || !defaultModel.trim() || (needsBaseUrl(type) && !baseUrl.trim())
            }
            className="rounded-md bg-amber-600 px-3 py-1 text-xs font-semibold text-zinc-950 transition hover:bg-amber-500 disabled:opacity-40"
          >
            Save
          </button>
        )}
        {provider && (
          <>
            <button
              onClick={() => void probe()}
              disabled={busy}
              className="rounded-md border border-zinc-700 px-3 py-1 text-xs text-zinc-300 transition hover:bg-zinc-900 disabled:opacity-40"
            >
              {busy ? 'Testing…' : 'Test'}
            </button>
            <button
              onClick={() => void remove()}
              disabled={busy}
              className="rounded-md border border-red-900 px-3 py-1 text-xs text-red-300 transition hover:bg-red-950 disabled:opacity-40"
            >
              Delete
            </button>
          </>
        )}
        {test &&
          (test.ok ? (
            <span className="flex items-center gap-1 text-xs text-emerald-400">
              <Check className="h-3.5 w-3.5 shrink-0" aria-hidden />
              {test.model} responded
            </span>
          ) : (
            <span
              className="flex min-w-0 items-center gap-1 text-xs text-red-400"
              title={test.error ?? ''}
            >
              <X className="h-3.5 w-3.5 shrink-0" aria-hidden />
              <span className="max-w-md truncate">{test.error}</span>
            </span>
          ))}
        {error && (
          <span className="max-w-md truncate text-xs text-red-400" title={error}>
            {error}
          </span>
        )}
      </div>
    </div>
  )
}
