import type { AdvancedSettings, ChatSettings, Settings } from '@/bindings';

/**
 * Every settings field is optional on the wire (`#[serde(default)]`); these mirror the Rust
 * defaults in `gantry-core/src/settings.rs` so the pages always hold a full section.
 */
export function chatDefaults(s: Settings | undefined): Required<ChatSettings> {
  const c = s?.chat ?? {};
  return {
    default_mode: c.default_mode ?? 'auto_edit',
    default_guard: c.default_guard ?? true,
    default_model: c.default_model ?? null,
    default_effort: c.default_effort ?? 'medium',
    custom_instructions: c.custom_instructions ?? '',
    suggest_connectors: c.suggest_connectors ?? true,
  };
}

export function advancedDefaults(s: Settings | undefined): Required<AdvancedSettings> {
  const a = s?.advanced ?? {};
  return {
    max_output_tokens: a.max_output_tokens ?? 8192,
    developer_mode: a.developer_mode ?? false,
  };
}
