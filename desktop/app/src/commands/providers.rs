use gantry_core::{ErrorDto, GantryError, ProviderErrorKind, ProviderId};
use gantry_providers::{KeyInfo, ModelInfo, catalog, registry::kind_of};
use gantry_secrets::{CredentialKind, OwnerKind};
use gantry_store::repos;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{AppState, events::ProvidersChanged};

/// What the UI may know about a key (docs/plan/11 §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct KeyStatus {
    pub present: bool,
    /// The last four characters, when present.
    pub hint: Option<String>,
    /// The last test failed with an authentication error.
    pub invalid: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ProviderRow {
    pub id: ProviderId,
    /// The client implementation: `openai_chat`, `anthropic`, `openai_responses`, `gemini`.
    pub kind: String,
    pub label: String,
    pub base_url: Option<String>,
    pub enabled: bool,
    pub key: KeyStatus,
    pub default_model: Option<String>,
    /// Whether this build has a client for the provider's kind.
    pub available: bool,
    /// A user-added OpenAI-compatible endpoint (`custom:<ulid>`); the only kind that can be
    /// removed.
    pub custom: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ProviderTest {
    pub ok: bool,
    pub info: Option<KeyInfo>,
    pub error: Option<ErrorDto>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ProviderUpdate {
    pub default_model: Option<String>,
    pub base_url: Option<String>,
}

/// Ids of user-added endpoints (06 §3).
pub const CUSTOM_PREFIX: &str = "custom:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CustomEndpoint {
    pub label: String,
    pub base_url: String,
}

fn key_status(state: &AppState, id: &str) -> Result<KeyStatus, GantryError> {
    let refs = state.secrets.list_for_owner(OwnerKind::Provider, id)?;
    let api_key = refs.iter().find(|r| r.kind == "api_key");
    let invalid = state
        .invalid_keys
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(id);
    Ok(KeyStatus {
        present: api_key.is_some(),
        hint: api_key.map(|r| r.hint.clone()),
        invalid,
    })
}

#[tauri::command]
#[specta::specta]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<ProviderRow>, ErrorDto> {
    let rows = state
        .store
        .read(repos::providers::list)
        .map_err(GantryError::from)?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(ProviderRow {
            key: key_status(&state, &r.id)?,
            custom: r.id.starts_with(CUSTOM_PREFIX),
            available: kind_of(&r.kind).is_some(),
            id: ProviderId::new(r.id),
            kind: r.kind,
            label: r.label,
            base_url: r.base_url,
            enabled: r.enabled,
            default_model: r.default_model,
        });
    }
    Ok(out)
}

/// Stores the key encrypted and forgets the plaintext. Write-only: nothing returns it.
#[tauri::command]
#[specta::specta]
pub async fn set_provider_key(
    app: AppHandle,
    state: State<'_, AppState>,
    provider_id: ProviderId,
    key: String,
) -> Result<KeyStatus, ErrorDto> {
    let key = key.trim().to_owned();
    if key.is_empty() {
        return Err(GantryError::invalid("the key is empty").into());
    }
    let id = provider_id.as_str().to_owned();
    let exists = state
        .store
        .read(|c| repos::providers::get(c, &id))
        .map_err(GantryError::from)?
        .is_some();
    if !exists {
        return Err(GantryError::not_found(format!("provider {id}")).into());
    }
    let secret = state
        .secrets
        .set(OwnerKind::Provider, &id, CredentialKind::ApiKey, None, &key)
        .await
        .map_err(GantryError::from)?;
    drop(key);
    let (pid, cid) = (id.clone(), secret.id.clone());
    state
        .store
        .write(move |c| repos::providers::set_credential(c, &pid, Some(&cid)))
        .await
        .map_err(GantryError::from)?;
    state
        .invalid_keys
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    state.providers.rebuild()?;
    let _ = ProvidersChanged.emit(&app);
    Ok(key_status(&state, &id)?)
}

#[tauri::command]
#[specta::specta]
pub async fn clear_provider_key(
    app: AppHandle,
    state: State<'_, AppState>,
    provider_id: ProviderId,
) -> Result<KeyStatus, ErrorDto> {
    let id = provider_id.as_str().to_owned();
    for r in state
        .secrets
        .list_for_owner(OwnerKind::Provider, &id)
        .map_err(GantryError::from)?
    {
        state
            .secrets
            .delete(&r.id)
            .await
            .map_err(GantryError::from)?;
    }
    let pid = id.clone();
    state
        .store
        .write(move |c| repos::providers::set_credential(c, &pid, None))
        .await
        .map_err(GantryError::from)?;
    state
        .invalid_keys
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    state.providers.rebuild()?;
    let _ = ProvidersChanged.emit(&app);
    Ok(key_status(&state, &id)?)
}

/// Checks the key with the provider's cheapest call and reports the error class on failure.
#[tauri::command]
#[specta::specta]
pub async fn test_provider(
    state: State<'_, AppState>,
    provider_id: ProviderId,
) -> Result<ProviderTest, ErrorDto> {
    let provider = state.providers.require(&provider_id)?;
    match provider.check_key().await {
        Ok(info) => {
            state
                .invalid_keys
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(provider_id.as_str());
            Ok(ProviderTest {
                ok: true,
                info: Some(info),
                error: None,
            })
        }
        Err(err) => {
            if err.kind == ProviderErrorKind::Auth {
                state
                    .invalid_keys
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(provider_id.as_str().to_owned());
            }
            Ok(ProviderTest {
                ok: false,
                info: None,
                error: Some(GantryError::from(err).into()),
            })
        }
    }
}

/// The provider's model list from the cache, or fresh when `refresh` is set or the cache is
/// stale (docs/plan/02 §2).
#[tauri::command]
#[specta::specta]
pub async fn list_models(
    state: State<'_, AppState>,
    provider_id: ProviderId,
    refresh: bool,
) -> Result<Vec<ModelInfo>, ErrorDto> {
    let provider = state.providers.require(&provider_id)?;
    Ok(catalog::list_models(&state.store, provider.as_ref(), refresh).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn update_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    provider_id: ProviderId,
    update: ProviderUpdate,
) -> Result<(), ErrorDto> {
    let id = provider_id.as_str().to_owned();
    state
        .store
        .write(move |c| {
            if let Some(model) = &update.default_model {
                let model = model.trim();
                repos::providers::set_default_model(c, &id, (!model.is_empty()).then_some(model))?;
            }
            if let Some(url) = &update.base_url {
                let url = url.trim();
                repos::providers::set_base_url(c, &id, (!url.is_empty()).then_some(url))?;
            }
            Ok(())
        })
        .await
        .map_err(GantryError::from)?;
    state.providers.rebuild()?;
    let _ = ProvidersChanged.emit(&app);
    Ok(())
}

/// Adds an OpenAI-compatible endpoint (11 §4): a `providers` row of kind `openai_chat` with the
/// `custom` profile. The key, if any, is added afterwards like any other.
#[tauri::command]
#[specta::specta]
pub async fn add_custom_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    endpoint: CustomEndpoint,
) -> Result<ProviderId, ErrorDto> {
    let label = endpoint.label.trim().to_owned();
    let base_url = endpoint.base_url.trim().trim_end_matches('/').to_owned();
    if label.is_empty() {
        return Err(GantryError::invalid("the endpoint needs a name").into());
    }
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return Err(
            GantryError::invalid("the base URL must start with http:// or https://").into(),
        );
    }
    let id = format!("{CUSTOM_PREFIX}{}", gantry_core::EventId::new());
    let row_id = id.clone();
    state
        .store
        .write(move |c| {
            repos::providers::ensure(c, &row_id, "openai_chat", &label, Some(&base_url))
        })
        .await
        .map_err(GantryError::from)?;
    state.providers.rebuild()?;
    let _ = ProvidersChanged.emit(&app);
    Ok(ProviderId::new(id))
}

/// Removes a custom endpoint and its key; the built-in accounts stay.
#[tauri::command]
#[specta::specta]
pub async fn remove_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    provider_id: ProviderId,
) -> Result<(), ErrorDto> {
    let id = provider_id.as_str().to_owned();
    if !id.starts_with(CUSTOM_PREFIX) {
        return Err(GantryError::invalid("only custom endpoints can be removed").into());
    }
    for r in state
        .secrets
        .list_for_owner(OwnerKind::Provider, &id)
        .map_err(GantryError::from)?
    {
        state
            .secrets
            .delete(&r.id)
            .await
            .map_err(GantryError::from)?;
    }
    let row_id = id.clone();
    state
        .store
        .write(move |c| repos::providers::delete(c, &row_id))
        .await
        .map_err(GantryError::from)?;
    state
        .invalid_keys
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    state.providers.rebuild()?;
    let _ = ProvidersChanged.emit(&app);
    Ok(())
}
