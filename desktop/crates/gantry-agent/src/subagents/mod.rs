//! Sub agents: a model handing part of a job to another model (docs/plan/18).
//!
//! Two halves live here. [`run`] is the engine — it creates the hidden chat, starts its turn,
//! waits for it and reports back — and it is a method on [`TurnManager`] because everything a
//! turn needs (providers, settings, the guard, the interaction registry, the notifier) is
//! already assembled there, and a second assembly of the same things is a second thing to keep
//! in step. [`connector`] is the tool the model actually calls.
//!
//! The sub agent *is* a turn: same runner, same event stream, same persistence, same guard.
//! What makes it a sub agent is three fields on its chat row and one on its turn input.

pub mod connector;
pub mod library;
mod spec;

use std::sync::Arc;

use gantry_core::{ChatId, Message, Surface, TurnId, TurnStatus, Usage, agent::INHERIT, now_ms};
use gantry_store::repos;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::{
    chats::{NewChat, SubAgentOrigin},
    events::EventSink,
    system_prompt::{CORE_VERSION, SystemPromptBuilder},
    turn_manager::{ActiveTurn, StartOptions, TurnManager},
};

pub use connector::{ID, SubAgents, definitions};
pub use library::{builtins, original, seed};
pub use spec::{Overrides, SubAgentRequest, resolve};

/// What the parent's tool call gets back.
#[derive(Debug, Clone, PartialEq)]
pub struct SubAgentReport {
    /// The transcript, for the tree that opens it (18 §7).
    pub chat_id: ChatId,
    pub agent: String,
    /// The sub agent's last message: its whole output.
    pub text: String,
    pub status: TurnStatus,
    pub usage: Option<Usage>,
    pub ms: i64,
}

/// What `start_message` needs to run a turn as a sub agent rather than as a person's.
pub(crate) struct SubAgentStart {
    pub origin: SubAgentOrigin,
    pub skills_on: bool,
    /// The turn that is waiting, so a permission card can be announced on its stream.
    pub parent: Arc<ActiveTurn>,
    pub done: oneshot::Sender<()>,
}

/// Stops the sub agent however this future ends.
///
/// The runner cancels a tool call by dropping its future (`runner::execute`), so the code that
/// started the sub agent gets no chance to do anything about it. Without this, a stopped turn
/// would leave a model running somewhere, spending money on an answer nobody will read.
struct StopOnDrop {
    manager: Arc<TurnManager>,
    turn: TurnId,
    finished: bool,
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        if !self.finished {
            self.manager.cancel(self.turn);
        }
    }
}

/// A sink for a conversation nobody is watching.
///
/// The sub agent's events still reach the database — `start_message` adds the persisting sink
/// itself — so the transcript is complete and readable afterwards. What there is no subscriber
/// for is the live channel: the parent's chat shows a waiting line and nothing else (18 A6).
struct Unwatched;

impl EventSink for Unwatched {
    fn emit(&self, _batch: gantry_core::AgentEventBatch) {}
}

impl TurnManager {
    /// Runs one sub agent to completion and reports (18 §4).
    ///
    /// Blocking is the whole shape of v1 (18 A2): the tool call does not return until this
    /// does, and several sub agents at once come from the parent emitting several calls in one
    /// round, which the runner already executes concurrently.
    ///
    /// Errors here are the model's to read and recover from — a type that does not exist, a
    /// field it may not set, a limit it has reached — so they are strings rather than
    /// `GantryError`: nothing in the interface shows them.
    pub async fn run_sub_agent(
        self: &Arc<Self>,
        req: SubAgentRequest,
        cancel: CancellationToken,
    ) -> Result<SubAgentReport, String> {
        let settings = self.settings();
        let store = self.chats().store().clone();

        let parent_chat = req.parent_chat;
        let parent = store
            .read(move |c| repos::chats::get(c, parent_chat))
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "the parent chat is gone".to_owned())?;

        // Depth, checked here as well as by leaving the namespace out of a sub agent's tool
        // list (18 A9): the list is the thing that stops it, this is the thing that says so.
        if parent.parent_turn_id.is_some() {
            return Err("a sub agent cannot start sub agents".to_owned());
        }
        if parent.incognito && !settings.subagents.in_incognito {
            return Err(
                "sub agents are switched off in incognito chats, because their transcripts \
                 would be rows in the database. Settings → Sub agents changes that."
                    .to_owned(),
            );
        }

        let library = store
            .read(repos::agents::list)
            .map_err(|e: gantry_store::StoreError| e.to_string())?;
        let attached = store
            .read(move |c| repos::connectors::attached_namespaces(c, parent_chat))
            .map_err(|e| e.to_string())?;
        let roots = store
            .read(move |c| repos::chats::roots(c, parent_chat))
            .map_err(|e| e.to_string())?;

        let spec = resolve(&req, &library, &settings.subagents, &parent, &attached)?;

        // The prompt is the ordinary one — the core, this machine's context, the user's own
        // instruction layers — with the type's brief as the most specific layer (10 §2).
        let context = self.prompt_context();
        let mode = spec.mode;
        let prompt = SystemPromptBuilder::new(mode, context)
            .global_instructions(&settings.chat.custom_instructions)
            .agent_instructions(&spec.instructions)
            .build();

        let chat = self
            .chats()
            .create(NewChat {
                surface: parent.surface,
                // A sub agent works in the same folders as the session that started it (18 §8).
                // What it may do there is decided by its tools, not by hiding the folder: a
                // read-only type is shown no tool that writes.
                roots: if parent.surface == Surface::Code {
                    roots
                } else {
                    Vec::new()
                },
                model: spec.model.clone(),
                mode,
                guard: spec.guard,
                effort: parent.effort,
                system_snapshot: prompt,
                system_snapshot_version: CORE_VERSION,
                connectors: spec.connectors.clone(),
                incognito: parent.incognito,
                project: parent.project_id,
                grants: Vec::new(),
                parent: Some((req.parent_turn, spec.agent.id.clone())),
            })
            .map_err(|e| e.to_string())?;

        let parent_turn = self
            .running(req.parent_turn)
            .ok_or_else(|| "the parent turn is no longer running".to_owned())?;
        let (done, wait) = oneshot::channel();
        let started = now_ms();
        let turn_id = self
            .start_message(
                chat.id,
                Message::user_text(req.task.clone()),
                Vec::new(),
                Arc::new(Unwatched),
                StartOptions {
                    sub: Some(SubAgentStart {
                        origin: SubAgentOrigin {
                            parent_chat,
                            parent_turn: req.parent_turn,
                            agent: spec.agent.name.clone(),
                            read_only: !spec.write_files,
                        },
                        skills_on: spec.skills,
                        parent: parent_turn,
                        done,
                    }),
                    ..Default::default()
                },
            )
            .map_err(|e| e.to_string())?;

        // The parent's cancel is the sub agent's cancel (18 §9), and the guard is how that
        // survives the way the runner actually cancels a call: it drops the future rather than
        // letting it return, so a `select!` in here is never polled again. Whatever happens to
        // this future — cancelled, dropped, panicked — the sub agent's turn is stopped.
        let mut stop = StopOnDrop {
            manager: Arc::clone(self),
            turn: turn_id,
            finished: false,
        };
        let mut wait = wait;
        tokio::select! {
            biased;
            () = cancel.cancelled() => {
                self.cancel(turn_id);
                // Its own runner still has to unwind: it writes the partial message and marks
                // the turn cancelled, and the report below should describe that rather than a
                // turn caught mid-write.
                let _ = (&mut wait).await;
            }
            _ = &mut wait => {}
        }
        stop.finished = true;

        Ok(self.report(chat.id, spec.agent.name.clone(), started))
    }

    /// What the sub agent ended up saying, read back from its own transcript.
    fn report(&self, chat_id: ChatId, agent: String, started: i64) -> SubAgentReport {
        let detail = self.chats().get(chat_id).ok().flatten();
        let turn = detail.as_ref().and_then(|d| d.turns.last());
        SubAgentReport {
            chat_id,
            agent,
            text: turn.map(|t| t.assistant_text()).unwrap_or_default(),
            status: turn.map_or(TurnStatus::Failed, |t| t.status),
            usage: turn.and_then(|t| t.usage),
            ms: now_ms() - started,
        }
    }
}

/// The namespaces a sub agent may reach, with [`INHERIT`] replaced by the parent's own and the
/// sub-agent namespace itself never in the result (18 A9).
pub(crate) fn namespaces(wanted: &[String], parent: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for want in wanted {
        if want == INHERIT {
            out.extend(parent.iter().cloned());
        } else {
            out.push(want.clone());
        }
    }
    out.retain(|n| n != ID);
    out.sort();
    out.dedup();
    out
}

/// Where a sub agent's turn is, for the tree and for the tests.
#[must_use]
pub fn is_sub_agent(origin: Option<&SubAgentOrigin>) -> Option<(ChatId, TurnId)> {
    origin.map(|o| (o.parent_chat, o.parent_turn))
}
