//! The tool set of one turn (docs/plan/01 §3 step 2, 02 §3): every registered connector's
//! tools, namespaced for the model, filtered for the mode, with a map back to the connector.

use std::sync::Arc;

use gantry_connectors::{Connector, ConnectorRegistry};
use gantry_core::{Mode, PlanModePolicy, ToolDef, ToolDisplay, ToolDisplayKind};
use gantry_providers::{ToolNameMap, ToolSpec};

#[derive(Clone)]
pub struct ToolEntry {
    pub connector: Arc<dyn Connector>,
    pub def: ToolDef,
    pub model_name: String,
}

impl ToolEntry {
    #[must_use]
    pub fn connector_id(&self) -> &str {
        &self.connector.descriptor().id
    }

    #[must_use]
    pub fn connector_name(&self) -> &str {
        &self.connector.descriptor().name
    }
}

#[derive(Clone, Default)]
pub struct ToolSet {
    entries: Vec<ToolEntry>,
    names: ToolNameMap,
}

impl ToolSet {
    /// Every tool of every registered connector that the mode offers (04 §4: Plan mode hides
    /// tools it would deny rather than letting the model waste rounds on them).
    pub async fn assemble(registry: &ConnectorRegistry, mode: Mode) -> Self {
        let mut set = ToolSet::default();
        for connector in registry.list() {
            let defs = match connector.tools().await {
                Ok(defs) => defs,
                Err(err) => {
                    log::warn!(
                        "connector {} offered no tools: {err}",
                        connector.descriptor().id
                    );
                    continue;
                }
            };
            for def in defs {
                if mode == Mode::Plan && def.plan_mode == PlanModePolicy::Deny {
                    continue;
                }
                let model_name = set.names.insert(&connector.descriptor().id, &def.name);
                set.entries.push(ToolEntry {
                    connector: connector.clone(),
                    def,
                    model_name,
                });
            }
        }
        set
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The model-facing declarations.
    #[must_use]
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.entries
            .iter()
            .map(|e| ToolSpec {
                name: e.model_name.clone(),
                description: e.def.description.clone(),
                input_schema: e.def.input_schema.clone(),
                strict: false,
                deferred: false,
                stream_args: e.def.stream_args,
            })
            .collect()
    }

    #[must_use]
    pub fn resolve(&self, model_name: &str) -> Option<&ToolEntry> {
        self.names.resolve(model_name).and_then(|(c, t)| {
            self.entries
                .iter()
                .find(|e| e.connector_id() == c && e.def.name == t)
        })
    }

    /// `(connector, tool)` from a model-facing name even when the tool is not in the set, so an
    /// unknown call still gets a readable row.
    #[must_use]
    pub fn split_name(model_name: &str) -> (String, String) {
        match model_name.split_once(gantry_providers::tools::SEPARATOR) {
            Some((c, t)) => (c.to_owned(), t.to_owned()),
            None => (String::new(), model_name.to_owned()),
        }
    }
}

/// How the row shows a call before any connector-specific rendering exists: the arguments
/// as `key=value` pairs, cut at 80 characters.
#[must_use]
pub fn display_for(def: Option<&ToolDef>, args: &serde_json::Value) -> ToolDisplay {
    let kind = match def.map(|d| d.tier) {
        Some(gantry_core::RiskTier::Read) => ToolDisplayKind::Read,
        _ => ToolDisplayKind::Connector,
    };
    let summary = match args.as_object() {
        Some(o) if !o.is_empty() => o
            .iter()
            .map(|(k, v)| {
                let value = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                format!("{k}={}", compact(&value, 40))
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    };
    ToolDisplay {
        kind,
        summary: compact(&summary, 80),
    }
}

fn compact(s: &str, max: usize) -> String {
    let one_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let mut cut: String = one_line.chars().take(max).collect();
        cut.push('…');
        cut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_summarise_arguments_on_one_line() {
        let d = display_for(
            None,
            &serde_json::json!({ "path": "src/app.rs", "lines": [1, 2], "note": "a\nb" }),
        );
        assert_eq!(d.kind, ToolDisplayKind::Connector);
        assert_eq!(d.summary, "lines=[1,2] note=a b path=src/app.rs");
        assert_eq!(display_for(None, &serde_json::json!({})).summary, "");
        assert_eq!(
            ToolSet::split_name("gantry__clock"),
            ("gantry".into(), "clock".into())
        );
    }
}
