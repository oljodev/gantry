//! The curated catalog (docs/plan/03 §11). Every entry ships inside the release; nothing is
//! fetched at runtime, so a compromised host cannot push a hostile connector definition to
//! anybody, and a user who is offline sees exactly the same list.

use std::sync::Arc;

use gantry_core::{CatalogEntryDto, InstanceId};

use crate::manifest::{Manifest, ManifestError};

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    entries: Vec<Arc<Manifest>>,
}

impl Catalog {
    /// Parses every embedded manifest. A manifest that fails validation is logged and left out
    /// rather than taking the app down: the rest of the catalog still works.
    #[must_use]
    pub fn embedded() -> Self {
        let mut entries = Vec::new();
        for (id, json) in EMBEDDED {
            match Manifest::parse(json) {
                Ok(manifest) => entries.push(Arc::new(manifest)),
                Err(err) => log::error!("the catalog entry {id} is unusable: {err}"),
            }
        }
        entries.sort_by(|a, b| {
            b.catalog
                .sort_weight
                .partial_cmp(&a.catalog.sort_weight)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Self { entries }
    }

    /// For tests and for the schema check: parse a set of manifests directly.
    pub fn from_json(sources: &[&str]) -> Result<Self, ManifestError> {
        let mut entries = Vec::new();
        for json in sources {
            entries.push(Arc::new(Manifest::parse(json)?));
        }
        Ok(Self { entries })
    }

    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<Manifest>> {
        self.entries.iter().find(|m| m.id == id).cloned()
    }

    #[must_use]
    pub fn all(&self) -> &[Arc<Manifest>] {
        &self.entries
    }

    /// The browse list, each entry told which instances it has produced.
    #[must_use]
    pub fn list(&self, installed: &[(String, InstanceId)]) -> Vec<CatalogEntryDto> {
        self.entries
            .iter()
            .map(|m| {
                let ids = installed
                    .iter()
                    .filter(|(catalog_id, _)| catalog_id == &m.id)
                    .map(|(_, id)| *id)
                    .collect();
                m.entry(ids)
            })
            .collect()
    }

    /// Keyword search over id, name, description and keywords, best first (03 §9).
    ///
    /// Hidden entries are not in it. They cannot be installed or removed, so offering one to a
    /// model looking for a connector is offering something it cannot act on (03 §11).
    #[must_use]
    pub fn search(&self, query: &str, limit: usize) -> Vec<Arc<Manifest>> {
        let mut scored: Vec<(u32, &Arc<Manifest>)> = self
            .entries
            .iter()
            .filter(|m| !m.catalog.hidden)
            .map(|m| (m.score(query), m))
            .filter(|(score, _)| *score > 0)
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, m)| m.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shipped_manifest_parses() {
        // The build script embeds whatever is under desktop/connectors; this is the assertion
        // that each one is also *valid*, which the build script does not check.
        let catalog = Catalog::embedded();
        assert_eq!(
            catalog.all().len(),
            EMBEDDED.len(),
            "a shipped manifest failed to parse"
        );
    }

    #[test]
    fn searching_finds_an_entry_by_its_name() {
        let catalog = Catalog::embedded();
        if catalog.all().is_empty() {
            return;
        }
        let first = catalog.all()[0].clone();
        let hits = catalog.search(&first.name, 5);
        assert!(
            hits.iter().any(|m| m.id == first.id),
            "{} not found",
            first.id
        );
    }
}
