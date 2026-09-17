//! The tool a model calls to start a sub agent (docs/plan/18 §4).
//!
//! One tool rather than one per type: the library can reach a dozen entries, and a dozen tools
//! crowd out the connectors in every request of every turn. Choosing a type is choosing an enum
//! value, which models do well, and the enum and its notes are rebuilt from the library each
//! turn because `ToolSet::assemble` asks every connector for its tools on every turn.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock, Weak},
};

use async_trait::async_trait;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::{
    AgentType, InstanceId, Mode, RiskTier, Settings, ToolDef, TurnId, settings::SubAgentSettings,
};
use gantry_store::{Store, repos};
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::{
    subagents::{Overrides, SubAgentRequest, spec::menu},
    turn_manager::TurnManager,
};

/// The catalog id, the namespace, and the folder under `desktop/connectors/`.
pub const ID: &str = "subagents";

/// The tools the page lists before anything has run. The real list is built per turn from the
/// library; this is the shape of it with nothing configured.
#[must_use]
pub fn definitions() -> Vec<ToolDef> {
    vec![run_def(&[], &SubAgentSettings::default())]
}

fn run_def(library: &[AgentType], settings: &SubAgentSettings) -> ToolDef {
    let enabled: Vec<&AgentType> = library.iter().filter(|a| a.enabled).collect();
    let mut properties = serde_json::Map::new();
    properties.insert(
        "agent".to_owned(),
        json!({
            "type": "string",
            "description": if enabled.is_empty() {
                "No sub agents are switched on.".to_owned()
            } else {
                enabled
                    .iter()
                    .map(|a| format!("{} — {}", a.id, a.description))
                    .collect::<Vec<_>>()
                    .join("\n")
            },
            "enum": enabled.iter().map(|a| a.id.clone()).collect::<Vec<_>>(),
        }),
    );
    properties.insert(
        "task".to_owned(),
        json!({
            "type": "string",
            "description": "What this sub agent is to do, written for somebody who cannot see \
                            this conversation: the question or the job, what it needs to know, \
                            and what you want back. It is the only thing it is told besides its \
                            own instructions.",
        }),
    );
    // An argument no enabled type would accept is not offered at all: a model that sends one
    // and is refused has spent a round learning what the schema could have told it (03 §5).
    let opens = |field: gantry_core::OpenField| enabled.iter().any(|a| a.opens(field));
    if opens(gantry_core::OpenField::Instructions) {
        properties.insert(
            "instructions".to_owned(),
            json!({
                "type": "string",
                "description": "How this sub agent should work, for the types that let you say. \
                                Standing rules rather than the task itself.",
            }),
        );
    }
    if opens(gantry_core::OpenField::Connectors) {
        properties.insert(
            "connectors".to_owned(),
            json!({
                "type": "array",
                "items": { "type": "string" },
                "description": "The tool namespaces it may use. Omit to give it the same ones \
                                this chat has.",
            }),
        );
    }
    if opens(gantry_core::OpenField::Write) {
        properties.insert(
            "write".to_owned(),
            json!({
                "type": "boolean",
                "description": "Whether it may change files, or only read them.",
            }),
        );
    }
    if opens(gantry_core::OpenField::Model) && !settings.model_rules.is_empty() {
        properties.insert(
            "model".to_owned(),
            json!({
                "type": "string",
                "description": format!(
                    "Which model it runs, `provider/model`. Omit for the one you are running \
                     on. The user has set these out for you: {}",
                    menu(settings)
                ),
                "enum": settings
                    .model_rules
                    .iter()
                    .map(|r| format!("{}/{}", r.model.provider.as_str(), r.model.model))
                    .collect::<Vec<_>>(),
            }),
        );
    }

    let mut def = ToolDef::new(
        "run",
        "Hand a piece of work to a sub agent and wait for its report. Use one for work that \
         would fill this conversation with material the user does not need to read — reading a \
         lot of pages, trying something several ways — or that can be done beside what you are \
         doing. You wait here until it is done; to run several at once, call this several times \
         in the same turn. A sub agent cannot see this conversation, cannot ask you anything, \
         and hands back one report of text.",
        json!({
            "type": "object",
            "properties": Value::Object(properties),
            "required": ["agent", "task"],
        }),
        // App tier: it starts a conversation inside Gantry. What the sub agent then does is
        // asked about on its own terms — a card in the user's chat, or the guard (18 §6) —
        // which is the permission that matters; a card here would only ask about delegating.
        RiskTier::App,
    );
    def.parallel_safe = true;
    def
}

/// How many sub agents one turn has started, so the two limits mean something (18 §9).
#[derive(Default)]
struct Tally {
    started: HashMap<TurnId, u32>,
    /// Insertion order, so an app left running for a week does not accumulate turns.
    order: Vec<TurnId>,
}

const TALLIED_TURNS: usize = 32;

impl Tally {
    fn take(&mut self, turn: TurnId, limit: u32) -> Result<(), String> {
        if !self.started.contains_key(&turn) {
            self.order.push(turn);
            if self.order.len() > TALLIED_TURNS {
                // Never the turn being counted: it was just pushed on the end.
                let oldest = self.order.remove(0);
                self.started.remove(&oldest);
            }
            self.started.insert(turn, 0);
        }
        let count = self.started.entry(turn).or_insert(0);
        if *count >= limit {
            return Err(format!(
                "this turn has already started {limit} sub agents, which is the limit in \
                 Settings → Sub agents. Finish with the ones you have, or do the rest yourself."
            ));
        }
        *count += 1;
        Ok(())
    }
}

/// The connector. It is registered like any other, and hidden from the catalogue like nothing
/// else (18 §1).
pub struct SubAgents {
    descriptor: ConnectorDescriptor,
    /// Weak, because the manager owns the registry this connector is in, and two `Arc`s in a
    /// ring would keep both alive for the life of the process.
    turns: Arc<RwLock<Weak<TurnManager>>>,
    store: Arc<Store>,
    settings: Arc<RwLock<Settings>>,
    tally: Mutex<Tally>,
    /// One permit per sub agent allowed to run at once. Waiting rather than refusing, because
    /// a concurrency limit is about the machine and the model has already decided the work.
    slots: Semaphore,
}

impl SubAgents {
    #[must_use]
    pub fn new(
        namespace: String,
        instance_id: InstanceId,
        turns: Arc<RwLock<Weak<TurnManager>>>,
        store: Arc<Store>,
        settings: Arc<RwLock<Settings>>,
    ) -> Self {
        let concurrent = settings
            .read()
            .map(|s| s.subagents.max_concurrent)
            .unwrap_or(3)
            .max(1) as usize;
        Self {
            descriptor: ConnectorDescriptor {
                // The namespace is fixed rather than taken from the instance row: the tool name
                // the model reads is `subagents__run` on every machine, and a second instance
                // of a connector that is part of the app is not a thing that can happen.
                id: namespace,
                name: "Sub agents".to_owned(),
                instance_id: Some(instance_id),
                first_party: true,
            },
            turns,
            store,
            settings,
            tally: Mutex::new(Tally::default()),
            slots: Semaphore::new(concurrent),
        }
    }

    fn preferences(&self) -> SubAgentSettings {
        self.settings
            .read()
            .map(|s| s.subagents.clone())
            .unwrap_or_default()
    }

    fn library(&self) -> Vec<AgentType> {
        self.store.read(repos::agents::list).unwrap_or_else(|err| {
            log::warn!("could not read the sub-agent library: {err}");
            Vec::new()
        })
    }
}

#[async_trait]
impl Connector for SubAgents {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        Ok(vec![run_def(&self.library(), &self.preferences())])
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        _sink: Arc<dyn ToolEventSink>,
        cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        if req.tool != "run" {
            return Err(ConnectorError::UnknownTool(req.tool));
        }
        let prefs = self.preferences();
        let request = match parse(&req) {
            Ok(r) => r,
            Err(message) => return Ok(ToolOutcome::error(message)),
        };
        if let Err(message) = self
            .tally
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take(req.scope.turn_id, prefs.max_per_turn.max(1))
        {
            return Ok(ToolOutcome::error(message));
        }
        let Some(turns) = self.turns.read().ok().and_then(|w| w.upgrade()) else {
            return Ok(ToolOutcome::error(
                "sub agents are not available in this session",
            ));
        };

        // Waiting for a slot is still work the parent asked for, so a cancel while queueing
        // has to be answered here rather than after the sub agent has started.
        let permit = tokio::select! {
            biased;
            () = cancel.cancelled() => return Ok(ToolOutcome::cancelled()),
            p = self.slots.acquire() => p,
        };
        let _permit = match permit {
            Ok(p) => p,
            Err(_) => return Ok(ToolOutcome::error("sub agents are shutting down")),
        };

        match turns.run_sub_agent(request, cancel).await {
            Err(message) => Ok(ToolOutcome::error(message)),
            Ok(report) => {
                let text = if report.text.trim().is_empty() {
                    format!(
                        "The {} sub agent ended {} without saying anything.",
                        report.agent,
                        status_word(report.status)
                    )
                } else {
                    report.text.clone()
                };
                // The report, then what it cost. The second part is a `Json` rather than only
                // the `structured` field beside it because `structured` reaches nobody: the
                // runner keeps a call's `content` and drops the rest, so a number that lives
                // only there is a number the row and the turn footer never see.
                let summary = json!({
                    "agent": report.agent,
                    "status": status_word(report.status),
                    "seconds": report.ms / 1000,
                    "transcript": report.chat_id.to_string(),
                    "tokens": report.usage.as_ref().map(|u| u.input + u.output),
                });
                Ok(ToolOutcome::Complete {
                    content: vec![
                        gantry_core::ResultPart::Text { text },
                        gantry_core::ResultPart::Json {
                            json: summary.clone(),
                        },
                    ],
                    structured: Some(summary),
                    is_error: report.status == gantry_core::TurnStatus::Failed,
                    media: Vec::new(),
                })
            }
        }
    }
}

/// What to call a finished turn when telling the model about it.
fn status_word(status: gantry_core::TurnStatus) -> &'static str {
    match status {
        gantry_core::TurnStatus::Completed => "completed",
        gantry_core::TurnStatus::Cancelled => "cancelled",
        gantry_core::TurnStatus::Failed => "failed",
        gantry_core::TurnStatus::Interrupted => "interrupted",
        gantry_core::TurnStatus::Running => "running",
    }
}

/// The call's arguments, with every optional field read only if it is there.
fn parse(req: &ToolCallRequest) -> Result<SubAgentRequest, String> {
    let args = &req.args;
    let agent = args
        .get("agent")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let task = args
        .get("task")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mode = match args.get("mode").and_then(Value::as_str) {
        None => None,
        Some(text) => Some(
            Mode::ALL
                .into_iter()
                .find(|m| m.as_str() == text)
                .ok_or_else(|| format!("`{text}` is not a permission mode"))?,
        ),
    };
    Ok(SubAgentRequest {
        parent_chat: req.scope.chat_id,
        parent_turn: req.scope.turn_id,
        agent,
        task,
        overrides: Overrides {
            instructions: args
                .get("instructions")
                .and_then(Value::as_str)
                .map(str::to_owned),
            connectors: args.get("connectors").and_then(Value::as_array).map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            }),
            write: args.get("write").and_then(Value::as_bool),
            model: args.get("model").and_then(Value::as_str).map(str::to_owned),
            mode,
            memory: args.get("memory").and_then(Value::as_bool),
            skills: args.get("skills").and_then(Value::as_bool),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The per-turn limit refuses rather than queues (18 §9): the model can act on a refusal —
    /// finish with what it has, or do the rest itself — and cannot act on a wait.
    #[test]
    fn a_turn_may_start_only_so_many() {
        let mut tally = Tally::default();
        let turn = TurnId::new();
        assert!(tally.take(turn, 2).is_ok());
        assert!(tally.take(turn, 2).is_ok());
        let err = tally.take(turn, 2).unwrap_err();
        assert!(err.contains("already started 2"), "{err}");

        // The next turn starts from nothing: the limit is per turn, not per chat.
        assert!(tally.take(TurnId::new(), 2).is_ok());
    }

    /// An app left open for a week counts turns it will never see again.
    #[test]
    fn the_count_of_old_turns_is_forgotten() {
        let mut tally = Tally::default();
        let first = TurnId::new();
        assert!(tally.take(first, 1).is_ok());
        for _ in 0..TALLIED_TURNS {
            assert!(tally.take(TurnId::new(), 1).is_ok());
        }
        assert!(tally.started.len() <= TALLIED_TURNS);
        assert!(
            !tally.started.contains_key(&first),
            "the oldest turn is the one dropped"
        );
    }
}
