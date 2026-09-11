import type { AdvancedSettings, ChatSettings, GuardrailSettings, Settings } from '@/bindings';

/**
 * Every settings field is optional on the wire (`#[serde(default)]`); these mirror the Rust
 * defaults in `gantry-core/src/settings.rs` so the pages always hold a full section.
 */
export function chatDefaults(s: Settings | undefined): Required<ChatSettings> {
  const c = s?.chat ?? {};
  return {
    default_mode: c.default_mode ?? 'auto_edit',
    default_guard: c.default_guard ?? true,
    code_default_mode: c.code_default_mode ?? 'auto_edit',
    code_default_guard: c.code_default_guard ?? true,
    default_model: c.default_model ?? null,
    default_effort: c.default_effort ?? 'medium',
    custom_instructions: c.custom_instructions ?? '',
    suggest_connectors: c.suggest_connectors ?? true,
    open_artifact_panel: c.open_artifact_panel ?? true,
    favourite_models: c.favourite_models ?? [],
    model_options: c.model_options ?? {},
  };
}

export function guardrailDefaults(s: Settings | undefined): Required<GuardrailSettings> {
  const g = s?.guardrails ?? {};
  return {
    enabled: g.enabled ?? true,
    disabled: g.disabled ?? [],
    custom: g.custom ?? [],
  };
}

export function advancedDefaults(s: Settings | undefined): Required<AdvancedSettings> {
  const a = s?.advanced ?? {};
  return {
    max_output_tokens: a.max_output_tokens ?? 8192,
    developer_mode: a.developer_mode ?? false,
    max_tool_rounds: a.max_tool_rounds ?? 50,
  };
}
