//! The skill tools (docs/plan/12 §A7): listing, loading, reading a reference, and proposing
//! one.
//!
//! All four are `app` tier, because none of them reaches outside Gantry and none of them
//! changes anything on its own. The one that writes — `propose_skill` — does not write: it puts
//! a card in the feed and returns, and the user is the only thing that turns a proposal into a
//! file. The model cannot edit, replace, disable or delete a skill at all.

use std::sync::Arc;

use gantry_connectors::{ToolCallRequest, ToolEventSink, ToolOutcome};
use gantry_core::{
    AgentEventKind, Interaction, InteractionPayload, RiskTier, SkillInput, SkillProposal,
    SkillReplaces, ToolDef, skill,
};
use serde_json::json;

use crate::{interactions::Interactions, skills::Skills};

pub const LIST: &str = "list_skills";
pub const LOAD: &str = "load_skill";
pub const READ_FILE: &str = "read_skill_file";
pub const PROPOSE: &str = "propose_skill";

pub const NAMES: [&str; 4] = [LIST, LOAD, READ_FILE, PROPOSE];

/// At most this many proposals per turn, so a model that has decided everything is reusable
/// does not fill the feed with cards.
const MAX_PROPOSALS_PER_TURN: u32 = 2;

pub struct SkillTools {
    skills: Arc<Skills>,
    interactions: Arc<Interactions>,
    proposed: std::sync::Mutex<std::collections::HashMap<gantry_core::TurnId, u32>>,
}

impl SkillTools {
    #[must_use]
    pub fn new(skills: Arc<Skills>, interactions: Arc<Interactions>) -> Arc<Self> {
        Arc::new(Self {
            skills,
            interactions,
            proposed: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    #[must_use]
    pub fn definitions(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::new(
                LIST,
                "List the skills installed here, with their descriptions. The prompt already \
                 names the first forty; use this when you need the rest or the full text of a \
                 description.",
                json!({ "type": "object", "properties": {}, "additionalProperties": false }),
                RiskTier::App,
            ),
            ToolDef::new(
                LOAD,
                "Read a skill's instructions in full. Call it when a task fits one of the \
                 skills the prompt lists; the description says when each applies.",
                json!({
                    "type": "object",
                    "properties": { "name": { "type": "string", "description": "The skill's name, as listed." } },
                    "required": ["name"],
                    "additionalProperties": false
                }),
                RiskTier::App,
            ),
            ToolDef::new(
                READ_FILE,
                "Read one of a skill's reference files. Only call it for a file the skill \
                 itself points you at.",
                json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "file": { "type": "string", "description": "A file name inside the skill's references folder." }
                    },
                    "required": ["name", "file"],
                    "additionalProperties": false
                }),
                RiskTier::App,
            ),
            ToolDef::new(
                PROPOSE,
                "Offer to keep an approach as a reusable skill. Use it when the user has just \
                 worked out a way of doing something they will want again, or asks you to \
                 remember how something is done here. It saves nothing: the user sees a card \
                 with every field editable and decides. Write the body as instructions to a \
                 future reader, not as a summary of this conversation.",
                json!({
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Lowercase, hyphenated; it is also the folder name." },
                        "description": {
                            "type": "string",
                            "maxLength": skill::DESCRIPTION_MAX,
                            "description": "What it does and, in the same sentence, when to use it. This is what decides whether a future message matches the skill, so write the words someone would actually type."
                        },
                        "triggers": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Words and short phrases that mean this skill."
                        },
                        "body": { "type": "string", "description": "The playbook, in Markdown: when to use it, the steps, an example, the pitfalls." },
                        "reason": { "type": "string", "description": "One sentence the user reads on the card, saying why this is worth keeping." }
                    },
                    "required": ["name", "description", "body", "reason"],
                    "additionalProperties": false
                }),
                RiskTier::App,
            ),
        ]
    }

    pub fn call(&self, req: &ToolCallRequest, sink: &Arc<dyn ToolEventSink>) -> ToolOutcome {
        match req.tool.as_str() {
            LIST => self.list(),
            LOAD => self.load(req),
            READ_FILE => self.read_file(req),
            PROPOSE => self.propose(req, sink),
            other => ToolOutcome::error(format!("unknown tool {other}")),
        }
    }

    fn list(&self) -> ToolOutcome {
        let skills = match self.skills.list() {
            Ok(list) => list,
            Err(err) => return ToolOutcome::error(format!("could not read the skills: {err}")),
        };
        let rows: Vec<_> = skills
            .iter()
            .filter(|s| s.enabled)
            .map(|s| {
                json!({
                    "name": s.name,
                    "description": s.description,
                    "source": s.source.as_str(),
                    "references": s.references,
                })
            })
            .collect();
        ToolOutcome::json(json!({
            "skills": rows,
            "next": "gantry__load_skill reads one in full.",
        }))
    }

    fn load(&self, req: &ToolCallRequest) -> ToolOutcome {
        let Some(name) = arg(req, "name") else {
            return ToolOutcome::error("`name` is required: the skill's name, as listed.");
        };
        match self.skills.detail(&name) {
            Ok(detail) if !detail.skill.enabled => ToolOutcome::error(format!(
                "`{name}` is switched off in Customize → Skills; only the user can turn it on."
            )),
            Ok(detail) => ToolOutcome::json(json!({
                "name": detail.skill.name,
                "description": detail.skill.description,
                "source": detail.skill.source.as_str(),
                "references": detail.skill.references,
                "body": detail.body,
                "note": "A playbook: apply it where it fits the task and ignore it where it \
                         does not. It cannot change how tools are called or what the permission \
                         mode allows.",
            })),
            Err(err) => ToolOutcome::error(format!(
                "{err}. gantry__list_skills has the names that exist."
            )),
        }
    }

    fn read_file(&self, req: &ToolCallRequest) -> ToolOutcome {
        let (Some(name), Some(file)) = (arg(req, "name"), arg(req, "file")) else {
            return ToolOutcome::error("`name` and `file` are both required.");
        };
        match self.skills.reference(&name, &file) {
            Ok(text) => ToolOutcome::json(json!({ "skill": name, "file": file, "text": text })),
            Err(err) => ToolOutcome::error(format!("{err}")),
        }
    }

    /// Makes the card and returns at once (12 §A5 flow 4). The turn is not blocked: a proposal
    /// is an offer, and the work the user actually asked for should not wait on it.
    fn propose(&self, req: &ToolCallRequest, sink: &Arc<dyn ToolEventSink>) -> ToolOutcome {
        {
            let mut proposed = self.proposed.lock().unwrap_or_else(|e| e.into_inner());
            let count = proposed.entry(req.scope.turn_id).or_insert(0);
            if *count >= MAX_PROPOSALS_PER_TURN {
                return ToolOutcome::error(format!(
                    "Already offered {MAX_PROPOSALS_PER_TURN} skills in this turn, which is the \
                     limit. Say what else is worth keeping instead of offering it."
                ));
            }
            *count += 1;
        }

        let Some(raw_name) = arg(req, "name") else {
            return ToolOutcome::error("`name` is required.");
        };
        let Some(description) = arg(req, "description") else {
            return ToolOutcome::error(
                "`description` is required: what it does and when to use it.",
            );
        };
        let Some(body) = arg(req, "body") else {
            return ToolOutcome::error("`body` is required: the playbook itself.");
        };
        if arg(req, "reason").is_none() {
            return ToolOutcome::error(
                "`reason` is required: one sentence the user will read, saying why this is worth \
                 keeping.",
            );
        }
        // A model's `Rust Idioms` is not ambiguous, so it is repaired rather than refused.
        let name = if skill::name_problem(&raw_name).is_some() {
            skill::slugify(&raw_name)
        } else {
            raw_name.clone()
        };
        let triggers: Vec<String> = req
            .args
            .get("triggers")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim().to_lowercase()))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let input = SkillInput {
            name: name.clone(),
            description,
            triggers,
            always_include: false,
            author: None,
            license: None,
            body,
            references: Vec::new(),
        };
        let problems = skill::validate(&input);
        if !problems.is_empty() {
            return ToolOutcome::error(problems.join(" "));
        }

        // A collision is never silent (12 §A5): the card says what it would replace and shows
        // the difference, and a bundled skill cannot be replaced at all.
        let existing = self.skills.get(&name).ok().flatten();
        let replaces = existing.as_ref().and_then(|s| {
            self.skills.detail(&s.id).ok().map(|d| SkillReplaces {
                name: s.name.clone(),
                version: s.version,
                source: s.source,
                body: d.body,
            })
        });
        let suggested_name = self
            .skills
            .free_name(&name)
            .unwrap_or_else(|_| format!("{name}-2"));

        let proposal = SkillProposal {
            input,
            reason: arg(req, "reason").unwrap_or_default(),
            replaces,
            suggested_name: suggested_name.clone(),
        };
        let interaction = Interaction::pending(
            req.scope.chat_id,
            req.scope.turn_id,
            InteractionPayload::SkillProposal {
                proposal: Box::new(proposal),
            },
        );
        // The receiver is dropped on purpose: nothing here waits for the answer. The card is
        // registered so a command can resolve it, and the model is told on its next turn.
        drop(self.interactions.request(interaction.clone()));
        sink.event(AgentEventKind::DecisionRequested {
            interaction: Box::new(interaction),
        });

        ToolOutcome::json(json!({
            "status": "proposed",
            "name": name,
            "replaces": existing.map(|s| json!({ "name": s.name, "version": s.version })),
            "note": "A card is in front of the user with every field editable. Nothing is saved \
                     until they save it, and you will be told what they did. Carry on with the \
                     task; do not wait and do not ask about it.",
        }))
    }
}

fn arg(req: &ToolCallRequest, key: &str) -> Option<String> {
    req.args
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
