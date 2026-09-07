//! The set of configured providers, built from the `providers` table and the vault; rebuilt
//! whenever a key or a provider row changes (docs/plan/01 §2).

use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use gantry_core::{GantryError, ProviderId};
use gantry_secrets::SecretVault;
use gantry_store::{Store, repos::providers};

use crate::{
    catalog,
    openai_chat::{CompatProfile, OpenAiChatProvider},
    provider::Provider,
};

pub struct ProviderRegistry {
    store: Arc<Store>,
    vault: Arc<SecretVault>,
    http: reqwest::Client,
    providers: RwLock<HashMap<ProviderId, Arc<dyn Provider>>>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<String> = self.ids().into_iter().map(|p| p.0).collect();
        f.debug_struct("ProviderRegistry")
            .field("providers", &ids)
            .finish()
    }
}

impl ProviderRegistry {
    /// Builds an empty registry; call [`rebuild`](Self::rebuild) to load it.
    #[must_use]
    pub fn new(store: Arc<Store>, vault: Arc<SecretVault>, http: reqwest::Client) -> Self {
        Self {
            store,
            vault,
            http,
            providers: RwLock::new(HashMap::new()),
        }
    }

    /// Reloads every provider row. Rows whose kind has no client yet are skipped with a log line.
    pub fn rebuild(&self) -> Result<(), GantryError> {
        let rows = self.store.read(providers::list)?;
        let mut next: HashMap<ProviderId, Arc<dyn Provider>> = HashMap::new();
        for row in rows.into_iter().filter(|r| r.enabled) {
            let id = ProviderId::new(row.id.clone());
            let key = row
                .credential_id
                .as_deref()
                .and_then(|cid| match self.vault.get(cid) {
                    Ok(k) => Some(k),
                    Err(err) => {
                        log::error!("could not decrypt the key for {}: {err}", row.id);
                        None
                    }
                });
            match row.kind.as_str() {
                "openai_chat" => {
                    let mut profile =
                        CompatProfile::for_provider_id(&row.id).unwrap_or_else(|| {
                            CompatProfile::custom(
                                row.label.clone(),
                                row.base_url.clone().unwrap_or_default(),
                            )
                        });
                    if let Some(url) = row.base_url.as_deref().filter(|u| !u.is_empty()) {
                        profile.base_url = url.to_owned();
                    }
                    let known = catalog::cached(&self.store, &row.id).unwrap_or_default();
                    let provider =
                        OpenAiChatProvider::new(id.clone(), profile, key, self.http.clone(), known);
                    next.insert(id, Arc::new(provider));
                }
                other => log::info!("provider {} of kind {other} has no client yet", row.id),
            }
        }
        *self.providers.write().unwrap_or_else(|e| e.into_inner()) = next;
        Ok(())
    }

    #[must_use]
    pub fn get(&self, id: &ProviderId) -> Option<Arc<dyn Provider>> {
        self.providers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()
    }

    /// The provider, or a `NotFound` error naming it.
    pub fn require(&self, id: &ProviderId) -> Result<Arc<dyn Provider>, GantryError> {
        self.get(id)
            .ok_or_else(|| GantryError::not_found(format!("provider {id}")))
    }

    #[must_use]
    pub fn ids(&self) -> Vec<ProviderId> {
        let mut ids: Vec<ProviderId> = self
            .providers
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        ids.sort();
        ids
    }
}
