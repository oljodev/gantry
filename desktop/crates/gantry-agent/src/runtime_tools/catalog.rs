//! The connector tools the app owns (docs/plan/03 §9, 04 §9): finding a connector, asking for
//! one the chat has not attached, and offering to install one that is not there at all.
//!
//! They are runtime tools rather than a connector because they are Gantry's own behaviour: no
//! external system, no auth, no process. None of them installs or attaches anything on its
//! own — attaching is a card the user answers, and installing runs the ordinary install flow
//! in the UI. `App` tier, so the permission engine never prompts for the *call*; the decision
//! the user makes is the card itself.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex, RwLock},
};

use gantry_connectors::{
    ConnectorRegistry, ToolCallRequest, ToolEventSink, ToolOutcome, catalog::Catalog,
};
use gantry_core::{
    AccessDecision, AccessRequest, AgentEventKind, ChatId, ConnectorInstanceDto,
    ConnectorSuggestion, DecisionSource, GrantScope, GrantSource, InstanceId, Interaction,
    InteractionPayload, InteractionResolution, RiskTier, Settings, SuggestionOutcome, ToolDef,
    TurnId,
};
use gantry_store::{Store, repos};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::interactions::Interactions;

pub const SEARCH: &str = "search_connectors";
pub const REQUEST_ACCESS: &str = "request_access";
pub const SUGGEST: &str = "suggest_connector";

pub const NAMES: [&str; 3] = [SEARCH, REQUEST_ACCESS, SUGGEST];

/// At most this many suggestions per turn (03 §9), so a model that cannot do the task does not
/// answer with a shopping list.
const MAX_SUGGESTIONS_PER_TURN: u32 = 2;
/// How many entries one search returns unless the model asks for fewer.
const DEFAULT_LIMIT: usize = 8;

#[derive(Default)]
struct Limits {
    /// Suggestions already made in a turn.
    suggested: HashMap<TurnId, u32>,
    /// Catalog entries the user said no to, per chat: never offered twice.
    declined: HashMap<ChatId, HashSet<String>>,
}

/// What the three tools need: what is installed, what the catalog holds, and a way to ask.
pub struct ConnectorAccess {
    store: Arc<Store>,
    registry: Arc<ConnectorRegistry>,
    catalog: Catalog,
    interactions: Arc<Interactions>,
    settings: Arc<RwLock<Settings>>,
    limits: Mutex<Limits>,
}

impl ConnectorAccess {
    #[must_use]
    pub fn new(
        store: Arc<Store>,
        registry: Arc<ConnectorRegistry>,
        interactions: Arc<Interactions>,
        settings: Arc<RwLock<Settings>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            registry,
            catalog: Catalog::embedded(),
            interactions,
            settings,
            limits: Mutex::new(Limits::default()),
        })
    }

    fn suggestions_on(&self) -> bool {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .chat
            .suggest_connectors
    }

    fn instances(&self) -> Vec<ConnectorInstanceDto> {
        match self.store.read(repos::connectors::list) {
            Ok(list) => list,
            Err(err) => {
                log::warn!("could not read the installed connectors: {err}");
                Vec::new()
            }
        }
    }

    fn attached(&self, chat_id: ChatId) -> Vec<String> {
        self.store
            .read(move |c| repos::connectors::attached_namespaces(c, chat_id))
            .unwrap_or_else(|err| {
                log::warn!("could not read the chat's connectors: {err}");
                Vec::new()
            })
    }

    /// The definitions this turn offers. `request_access` appears only when something is
    /// installed to ask for, and `suggest_connector` only while the setting allows it (03 §9).
    #[must_use]
    pub fn definitions(&self) -> Vec<ToolDef> {
        let mut defs = vec![search_def()];
        if self.instances().iter().any(|i| i.enabled) {
            defs.push(request_access_def());
        }
        if self.suggestions_on() {
            defs.push(suggest_def());
        }
        defs
    }

    pub async fn call(
        &self,
        req: &ToolCallRequest,
        sink: &Arc<dyn ToolEventSink>,
        cancel: &CancellationToken,
    ) -> ToolOutcome {
        match req.tool.as_str() {
            SEARCH => self.search(req),
            REQUEST_ACCESS => self.request_access(req, sink, cancel).await,
            SUGGEST => self.suggest(req, sink, cancel).await,
            other => ToolOutcome::error(format!("unknown tool {other}")),
        }
    }

    /// Everything Gantry can reach, whether or not it is installed (03 §9).
    fn search(&self, req: &ToolCallRequest) -> ToolOutcome {
        let query = req
            .args
            .get("query")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        let limit = req
            .args
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map_or(DEFAULT_LIMIT, |n| (n as usize).clamp(1, 25));
        let installed = self.instances();
        let attached = self.attached(req.scope.chat_id);

        let mut results = Vec::new();
        // Installed instances first: they are one card, or nothing at all, away from working.
        for i in installed
            .iter()
            .filter(|i| matches(&query, &i.name, &i.namespace, &i.tools_text()))
        {
            results.push(json!({
                "connector": i.namespace,
                "name": i.name,
                "description": describe_instance(i),
                "installed": true,
                "attached": attached.iter().any(|n| n == &i.namespace),
                "ready": i.enabled && i.auth_state == gantry_core::AuthState::Authorized,
                "tools": i.tools.iter().map(|t| t.name.clone()).collect::<Vec<_>>(),
            }));
            if results.len() >= limit {
                break;
            }
        }
        // Then the catalog, minus what is already installed above.
        let installed_ids: HashSet<&str> = installed
            .iter()
            .filter_map(|i| i.catalog_id.as_deref())
            .collect();
        let entries = if query.is_empty() {
            self.catalog
                .all()
                .iter()
                .filter(|m| !m.catalog.hidden)
                .cloned()
                .collect()
        } else {
            self.catalog.search(&query, limit)
        };
        for m in entries {
            if results.len() >= limit {
                break;
            }
            if installed_ids.contains(m.id.as_str()) {
                continue;
            }
            let entry = m.entry(Vec::new());
            results.push(json!({
                "id": entry.id,
                "name": entry.name,
                "description": entry.description,
                "category": entry.category,
                "installed": false,
                "attached": false,
                "requires": {
                    "auth": entry.auth.as_str(),
                    "runtime": entry.requires.iter().map(|r| format!("{} {}", r.name, r.version))
                        .collect::<Vec<_>>(),
                },
            }));
        }

        ToolOutcome::json(json!({
            "results": results,
            "next": "Use gantry__request_access for an installed connector this chat has not \
                     attached, and gantry__suggest_connector to offer to install one that is not \
                     installed. Neither happens without the user's answer.",
        }))
    }

    /// Asks the user to attach an installed connector to this chat (04 §9).
    async fn request_access(
        &self,
        req: &ToolCallRequest,
        sink: &Arc<dyn ToolEventSink>,
        cancel: &CancellationToken,
    ) -> ToolOutcome {
        let Some(name) = req
            .args
            .get("connector")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return ToolOutcome::error(
                "`connector` is required: the namespace of an installed connector, as gantry__search_connectors reports it.",
            );
        };
        let reason = req
            .args
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        if reason.is_empty() {
            return ToolOutcome::error(
                "`reason` is required: one sentence the user will read, saying what you need it for.",
            );
        }
        let tools: Vec<String> = req
            .args
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();

        let installed = self.instances();
        let Some(instance) = installed
            .iter()
            .find(|i| i.namespace == name || i.name.eq_ignore_ascii_case(name))
        else {
            return ToolOutcome::error(format!(
                "{name} is not installed. Search with gantry__search_connectors, then offer it \
                 with gantry__suggest_connector."
            ));
        };
        if !instance.enabled {
            return ToolOutcome::error(format!(
                "{} is switched off in Settings → Connectors; only the user can turn it back on.",
                instance.name
            ));
        }
        if self
            .attached(req.scope.chat_id)
            .iter()
            .any(|n| n == &instance.namespace)
        {
            return ToolOutcome::json(json!({
                "attached": true,
                "connector": instance.namespace,
                "note": "Already attached to this chat; its tools are in your tool list.",
            }));
        }

        // 04 §9: in Auto the answer is already in — from the mode when the guard is off, from
        // the guard before this call was allowed to run when it is on — and a card would be
        // asking a question that has been answered. Attaching still grants nothing by itself:
        // every call the new tools make comes back through the same engine this one came
        // through, guardrails, mode and guard alike.
        if req.scope.attach_decided {
            return self
                .attach_and_report(
                    req.scope.chat_id,
                    instance,
                    "Attached without asking, because the chat is in Auto mode. The tools are \
                     available from your next call, and permission for each of them still \
                     follows the mode.",
                )
                .await;
        }

        let payload = InteractionPayload::AccessRequest {
            request: AccessRequest {
                instance_id: instance.id,
                connector: instance.namespace.clone(),
                connector_name: instance.name.clone(),
                tools: tools.clone(),
                tool_count: u32::try_from(instance.tools.len()).unwrap_or(u32::MAX),
                reason,
            },
        };
        let resolution = self
            .ask(req.scope.chat_id, req.scope.turn_id, payload, sink, cancel)
            .await;
        match resolution {
            InteractionResolution::AccessRequest {
                decision: AccessDecision::Attach { allow_tools },
                ..
            } => {
                if allow_tools {
                    self.grant(req.scope.chat_id, instance, &tools);
                }
                self.attach_and_report(
                    req.scope.chat_id,
                    instance,
                    "The tools are available from your next call. Permission still follows the \
                     chat's mode.",
                )
                .await
            }
            InteractionResolution::AccessRequest { message, .. } => ToolOutcome::json(json!({
                "attached": false,
                "message": message.unwrap_or_else(|| {
                    "The user did not attach this connector. Continue without it and say what \
                     you cannot do."
                        .to_owned()
                }),
            })),
            _ => ToolOutcome::json(json!({
                "attached": false,
                "message": "The request was cancelled.",
            })),
        }
    }

    /// Attaches the instance to the chat and reports it to the model, whichever of the two
    /// decisions got here. The grant, when there is one, is made by the caller: only the user
    /// pre-approves tools, so only the card's branch has one to make.
    async fn attach_and_report(
        &self,
        chat_id: ChatId,
        instance: &ConnectorInstanceDto,
        note: &str,
    ) -> ToolOutcome {
        if let Err(err) = self.attach(chat_id, instance.id) {
            return ToolOutcome::error(format!("could not attach {}: {err}", instance.name));
        }
        let names = self.tool_names(instance).await;
        ToolOutcome::json(json!({
            "attached": true,
            "connector": instance.namespace,
            "tools": names,
            "note": note,
        }))
    }

    /// Offers to install something that is not installed at all (03 §9).
    async fn suggest(
        &self,
        req: &ToolCallRequest,
        sink: &Arc<dyn ToolEventSink>,
        cancel: &CancellationToken,
    ) -> ToolOutcome {
        if !self.suggestions_on() {
            return ToolOutcome::error(
                "The user turned connector suggestions off in Settings → General.",
            );
        }
        let Some(id) = req
            .args
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return ToolOutcome::error(
                "`id` is required: a catalog id from gantry__search_connectors.",
            );
        };
        let reason = req
            .args
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_owned();
        if reason.is_empty() {
            return ToolOutcome::error(
                "`reason` is required: one sentence the user will read, saying why this connector.",
            );
        }
        let Some(manifest) = self.catalog.get(id) else {
            return ToolOutcome::error(format!(
                "{id} is not in the catalog. Search first with gantry__search_connectors."
            ));
        };
        if self
            .instances()
            .iter()
            .any(|i| i.catalog_id.as_deref() == Some(id))
        {
            return ToolOutcome::error(format!(
                "{} is already installed; use gantry__request_access instead.",
                manifest.name
            ));
        }
        {
            let mut limits = self.limits.lock().unwrap_or_else(|e| e.into_inner());
            if limits
                .declined
                .get(&req.scope.chat_id)
                .is_some_and(|d| d.contains(id))
            {
                return ToolOutcome::json(json!({
                    "outcome": "declined",
                    "message": "The user already said no to this connector in this chat. Do not \
                                offer it again.",
                }));
            }
            let count = limits.suggested.entry(req.scope.turn_id).or_default();
            if *count >= MAX_SUGGESTIONS_PER_TURN {
                return ToolOutcome::json(json!({
                    "outcome": "declined",
                    "message": "Enough suggestions for one reply. Answer with what you have.",
                }));
            }
            *count += 1;
        }

        let entry = manifest.entry(Vec::new());
        let payload = InteractionPayload::ConnectorSuggestion {
            suggestion: ConnectorSuggestion {
                catalog_id: entry.id.clone(),
                name: entry.name.clone(),
                description: entry.description.clone(),
                category: entry.category.clone(),
                auth: entry.auth,
                requires: entry.requires.clone(),
                reason,
            },
        };
        let resolution = self
            .ask(req.scope.chat_id, req.scope.turn_id, payload, sink, cancel)
            .await;
        match resolution {
            InteractionResolution::ConnectorSuggestion {
                outcome: SuggestionOutcome::Installed { instance_id },
            } => {
                if let Err(err) = self.attach(req.scope.chat_id, instance_id) {
                    return ToolOutcome::error(format!("could not attach {}: {err}", entry.name));
                }
                let instance = self.instances().into_iter().find(|i| i.id == instance_id);
                let ready = instance
                    .as_ref()
                    .is_some_and(|i| i.auth_state == gantry_core::AuthState::Authorized);
                let names = match &instance {
                    Some(i) => self.tool_names(i).await,
                    None => Vec::new(),
                };
                ToolOutcome::json(json!({
                    "outcome": if ready { "installed_and_attached" } else { "needs_setup" },
                    "connector": instance.as_ref().map(|i| i.namespace.clone()),
                    "tools": names,
                }))
            }
            _ => {
                self.limits
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .declined
                    .entry(req.scope.chat_id)
                    .or_default()
                    .insert(entry.id.clone());
                ToolOutcome::json(json!({
                    "outcome": "declined",
                    "message": "The user did not install it. Say plainly what you cannot do \
                                without it.",
                }))
            }
        }
    }

    /// Puts a card in front of the user and waits for it, exactly as the permission path does.
    async fn ask(
        &self,
        chat_id: ChatId,
        turn_id: TurnId,
        payload: InteractionPayload,
        sink: &Arc<dyn ToolEventSink>,
        cancel: &CancellationToken,
    ) -> InteractionResolution {
        let interaction = Interaction::pending(chat_id, turn_id, payload);
        let id = interaction.id;
        let rx = self.interactions.request(interaction.clone());
        sink.event(AgentEventKind::DecisionRequested {
            interaction: Box::new(interaction),
        });
        let resolution = tokio::select! {
            () = cancel.cancelled() => InteractionResolution::Cancelled,
            r = rx => r.unwrap_or(InteractionResolution::Cancelled),
        };
        sink.event(AgentEventKind::DecisionResolved {
            interaction_id: id,
            resolution: resolution.clone(),
            source: DecisionSource::UserOnce,
        });
        resolution
    }

    fn attach(
        &self,
        chat_id: ChatId,
        instance_id: InstanceId,
    ) -> Result<(), gantry_store::StoreError> {
        self.store.write_blocking(move |c| {
            repos::connectors::attach(c, chat_id, instance_id, "access_request")
        })
    }

    /// "Attach and allow these tools": one grant per tool the model named (04 §9).
    fn grant(&self, chat_id: ChatId, instance: &ConnectorInstanceDto, tools: &[String]) {
        for tool in tools {
            let grant = GrantScope::Tool.grant(
                chat_id,
                &instance.namespace,
                &instance.name,
                tool,
                GrantSource::AccessRequest,
            );
            if let Err(err) = self
                .store
                .write_blocking(move |c| repos::grants::insert(c, &grant))
            {
                log::warn!("could not store the grant: {err}");
            }
        }
    }

    /// The live tool names if the connector is registered, the cached ones otherwise.
    async fn tool_names(&self, instance: &ConnectorInstanceDto) -> Vec<String> {
        if let Some(connector) = self.registry.get(&instance.namespace)
            && let Ok(defs) = connector.tools().await
        {
            return defs.into_iter().map(|d| d.name).collect();
        }
        instance.tools.iter().map(|t| t.name.clone()).collect()
    }
}

/// Text a search query is matched against for an installed instance.
trait ToolsText {
    fn tools_text(&self) -> String;
}

impl ToolsText for ConnectorInstanceDto {
    fn tools_text(&self) -> String {
        self.tools
            .iter()
            .map(|t| format!("{} {}", t.name, t.description))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Whether an installed instance answers a query: any word of it against any word of the
/// name, the namespace or the tools, matching on either prefix so "issues" finds `create_issue`.
fn matches(query: &str, name: &str, namespace: &str, tools: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let haystack = format!("{name} {namespace} {tools}").to_lowercase();
    let tokens: Vec<&str> = haystack
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    query.split_whitespace().any(|word| {
        let word = word.to_lowercase();
        tokens.iter().any(|t| {
            t.starts_with(word.as_str()) || (word.len() >= 3 && t.len() >= 3 && word.starts_with(t))
        })
    })
}

fn describe_instance(i: &ConnectorInstanceDto) -> String {
    let count = i.tools.len();
    let state = match i.auth_state {
        gantry_core::AuthState::Authorized => "connected",
        _ => "installed, not signed in",
    };
    format!("{count} tools, {state}")
}

#[must_use]
fn search_def() -> ToolDef {
    let mut def = ToolDef::new(
        SEARCH,
        "Look through the connectors Gantry can use: the ones installed on this machine \
         (whether or not this chat has attached them) and the ones in the catalog that are not \
         installed. Call it whenever the user asks for something none of your current tools can \
         do — GitHub, Cloudflare, a database, a browser — instead of saying you have no access. \
         Leave `query` out to see everything.",
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What the connector should do: `github`, `issues`, `cloudflare workers`."
                },
                "limit": { "type": "integer", "minimum": 1, "maximum": 25 }
            },
            "additionalProperties": false
        }),
        RiskTier::App,
    );
    def.parallel_safe = true;
    def
}

#[must_use]
fn request_access_def() -> ToolDef {
    let mut def = ToolDef::new(
        REQUEST_ACCESS,
        "Ask to let this chat use a connector that is installed but not attached. The user \
         answers on a card, unless the chat is in Auto mode, where the decision is made without \
         them. If it is allowed, the connector's tools join your tool list from your next call. \
         Use it instead of telling the user to attach something by hand.",
        json!({
            "type": "object",
            "properties": {
                "connector": {
                    "type": "string",
                    "description": "The connector's namespace, as gantry__search_connectors reports it."
                },
                "tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "The tools you mean to call, if you know them."
                },
                "reason": {
                    "type": "string",
                    "description": "One sentence the user reads: what you need it for."
                }
            },
            "required": ["connector", "reason"],
            "additionalProperties": false
        }),
        RiskTier::App,
    );
    def.widens_access = true;
    def
}

#[must_use]
fn suggest_def() -> ToolDef {
    ToolDef::new(
        SUGGEST,
        "Offer to install a connector from the catalog that is not installed yet. It shows a \
         card with an Install button; nothing is installed unless the user presses it. At most \
         two per reply, and never one the user has already turned down in this chat.",
        json!({
            "type": "object",
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The catalog id from gantry__search_connectors."
                },
                "reason": {
                    "type": "string",
                    "description": "One sentence the user reads: why this connector, for this task."
                }
            },
            "required": ["id", "reason"],
            "additionalProperties": false
        }),
        RiskTier::App,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_matches_a_name_a_namespace_or_a_tool() {
        assert!(matches("", "GitHub", "github", ""));
        assert!(matches(
            "issues",
            "GitHub",
            "github",
            "create_issue open an issue"
        ));
        assert!(matches("GITHUB", "GitHub", "github", ""));
        assert!(!matches("supabase", "GitHub", "github", "create_issue"));
    }

    #[test]
    fn the_three_definitions_are_app_tier_and_named_once() {
        let defs = [search_def(), request_access_def(), suggest_def()];
        for def in &defs {
            assert_eq!(def.tier, RiskTier::App, "{} must never prompt", def.name);
        }
        let names: HashSet<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names.len(), 3);
        assert!(NAMES.iter().all(|n| names.contains(n)));
    }

    /// 04 §9. Attaching is a decision Auto may take; installing never is, in any mode. The
    /// difference is what each one costs if it is wrong: attaching hands the chat tools that
    /// still ask before every call, and installing runs somebody else's code.
    #[test]
    fn only_attaching_is_a_decision_a_mode_can_take() {
        assert!(request_access_def().widens_access);
        assert!(!suggest_def().widens_access);
        assert!(!search_def().widens_access);
    }
}
