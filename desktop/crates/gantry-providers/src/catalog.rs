//! The model catalog: each provider's model list, cached in the `models` table for a day and
//! refreshed on demand (docs/plan/02 §2, 11 §4).

use std::sync::Arc;

use gantry_core::GantryError;
use gantry_store::{Store, repos::models};

use crate::provider::{ModelCapabilities, ModelInfo, Pricing, Provider};

/// How long a cached list is trusted before a background refresh is worth it.
pub const TTL_MS: i64 = 24 * 60 * 60 * 1000;

/// The cached list, refreshed when `refresh` is set, the cache is empty, or it is older than
/// [`TTL_MS`]. Without a key the cache is returned as it is.
pub async fn list_models(
    store: &Arc<Store>,
    provider: &dyn Provider,
    refresh: bool,
) -> Result<Vec<ModelInfo>, GantryError> {
    let pid = provider.id().to_string();
    let cached = store.read(|c| models::list_for(c, &pid))?;
    let fetched_at = cached.iter().map(|m| m.fetched_at).max();
    let stale = fetched_at.is_none_or(|t| gantry_core::now_ms() - t > TTL_MS);
    if !provider.has_key() || (!refresh && !stale) {
        return Ok(cached.into_iter().map(from_record).collect());
    }
    let fresh = match provider.list_models().await {
        Ok(list) => list,
        Err(err) if !cached.is_empty() && !refresh => {
            log::warn!("model list refresh for {pid} failed ({err}); using the cache");
            return Ok(cached.into_iter().map(from_record).collect());
        }
        Err(err) => return Err(err.into()),
    };
    let now = gantry_core::now_ms();
    let records: Vec<models::ModelRecord> = fresh.iter().map(|m| to_record(&pid, m, now)).collect();
    let pid_for_write = pid.clone();
    store
        .write(move |c| models::replace_for(c, &pid_for_write, &records))
        .await?;
    Ok(fresh)
}

fn to_record(provider_id: &str, m: &ModelInfo, fetched_at: i64) -> models::ModelRecord {
    models::ModelRecord {
        provider_id: provider_id.to_owned(),
        model_id: m.id.clone(),
        display_name: m.display_name.clone(),
        capabilities_json: serde_json::to_string(&m.capabilities).unwrap_or_else(|_| "{}".into()),
        context_window: m.context_window,
        max_output: m.max_output,
        pricing_json: m.pricing.and_then(|p| serde_json::to_string(&p).ok()),
        fetched_at,
    }
}

fn from_record(r: models::ModelRecord) -> ModelInfo {
    ModelInfo {
        id: r.model_id,
        display_name: r.display_name,
        context_window: r.context_window,
        max_output: r.max_output,
        pricing: r
            .pricing_json
            .as_deref()
            .and_then(|p| serde_json::from_str::<Pricing>(p).ok()),
        capabilities: serde_json::from_str::<ModelCapabilities>(&r.capabilities_json)
            .unwrap_or_default(),
    }
}
