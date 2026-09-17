//! Turning one call into one sub agent (docs/plan/18 §3, §5).
//!
//! Pure: a request, the library, the settings and the parent chat in, a spec or a sentence the
//! model can act on out. It is the half worth testing, because every rule about who decides
//! what lives here — the type's fixed fields, the parent's open ones, the model menu, and the
//! rule that a sub agent never holds more than the chat that started it.

use gantry_core::{
    AgentModel, AgentType, ChatId, Mode, ModelRef, OpenField, TurnId, settings::SubAgentPermission,
    settings::SubAgentSettings,
};
use gantry_store::repos::chats::ChatRecord;

/// One `subagents__run` call, parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAgentRequest {
    pub parent_chat: ChatId,
    pub parent_turn: TurnId,
    /// The type the parent named.
    pub agent: String,
    /// What it is being asked to do, in the parent's words.
    pub task: String,
    pub overrides: Overrides,
}

/// The fields a call may set, each `Some` only if the parent actually sent it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub instructions: Option<String>,
    pub connectors: Option<Vec<String>>,
    pub write: Option<bool>,
    /// `provider/model`, which must be one of the user's rules (18 §5).
    pub model: Option<String>,
    pub mode: Option<Mode>,
    pub memory: Option<bool>,
    pub skills: Option<bool>,
}

impl Overrides {
    /// Every field this call actually set, for the "you may not set that" check.
    fn given(&self) -> Vec<OpenField> {
        let mut out = Vec::new();
        if self.instructions.is_some() {
            out.push(OpenField::Instructions);
        }
        if self.connectors.is_some() {
            out.push(OpenField::Connectors);
        }
        if self.write.is_some() {
            out.push(OpenField::Write);
        }
        if self.model.is_some() {
            out.push(OpenField::Model);
        }
        if self.mode.is_some() {
            out.push(OpenField::Mode);
        }
        if self.memory.is_some() {
            out.push(OpenField::Memory);
        }
        if self.skills.is_some() {
            out.push(OpenField::Skills);
        }
        out
    }
}

/// What one sub agent will actually run as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub agent: AgentType,
    pub instructions: String,
    pub model: ModelRef,
    pub connectors: Vec<String>,
    pub mode: Mode,
    pub guard: bool,
    pub write_files: bool,
    pub memory: bool,
    pub skills: bool,
}

/// Resolves a call against the library, the settings and the parent chat.
pub fn resolve(
    req: &SubAgentRequest,
    library: &[AgentType],
    settings: &SubAgentSettings,
    parent: &ChatRecord,
    attached: &[String],
) -> Result<Spec, String> {
    if req.task.trim().is_empty() {
        return Err("the task is empty: say what the sub agent is for".to_owned());
    }
    let Some(agent) = library
        .iter()
        .find(|a| a.enabled && a.id == req.agent.trim())
        .cloned()
    else {
        let offered: Vec<&str> = library
            .iter()
            .filter(|a| a.enabled)
            .map(|a| a.id.as_str())
            .collect();
        return Err(format!(
            "there is no sub agent called `{}`. The ones you can start are: {}",
            req.agent.trim(),
            if offered.is_empty() {
                "none — they are all switched off in Settings → Sub agents".to_owned()
            } else {
                offered.join(", ")
            }
        ));
    };

    // A field the type fixed is refused rather than ignored: an argument that is quietly
    // dropped is how a model learns to keep sending one (18 §3).
    for field in req.overrides.given() {
        if !agent.opens(field) {
            return Err(format!(
                "the {} sub agent decides its own {}; send only the task",
                agent.id,
                field.key()
            ));
        }
    }

    let model = match &req.overrides.model {
        Some(named) => {
            let Some(rule) = settings
                .model_rules
                .iter()
                .find(|r| format!("{}/{}", r.model.provider.as_str(), r.model.model) == *named)
            else {
                return Err(format!(
                    "`{named}` is not one of the models to choose from. They are: {}",
                    menu(settings)
                ));
            };
            rule.model.clone()
        }
        // `Rules` with nothing chosen is the parent's model, not a failure: the menu is an
        // offer, and a model that ignores it has still been given a model.
        None => match &agent.model {
            AgentModel::Named { model } => model.clone(),
            AgentModel::Inherit | AgentModel::Rules => parent.model.clone(),
        },
    };

    // Who answers a card decides the mode, above everything the type or the parent says: the
    // setting is the user's statement about being interrupted (18 §6).
    let (mode, guard) = match settings.permission {
        SubAgentPermission::Guard => (Mode::Auto, true),
        SubAgentPermission::Ask => (
            // A sub agent is never freer than the chat that started it.
            req.overrides
                .mode
                .or(agent.mode)
                .unwrap_or(parent.mode)
                .narrower(parent.mode),
            agent.guard.unwrap_or(parent.guard),
        ),
    };

    let wanted = req
        .overrides
        .connectors
        .clone()
        .unwrap_or_else(|| agent.connectors.clone());

    Ok(Spec {
        instructions: req
            .overrides
            .instructions
            .clone()
            .unwrap_or_else(|| agent.instructions.clone()),
        model,
        connectors: super::namespaces(&wanted, attached),
        mode,
        guard,
        write_files: req.overrides.write.unwrap_or(agent.write_files),
        // Incognito reads nothing from memory, whatever a type says (15 A21).
        memory: req.overrides.memory.unwrap_or(agent.memory) && !parent.incognito,
        skills: req.overrides.skills.unwrap_or(agent.skills),
        agent,
    })
}

/// The model menu as one line, for the refusal above and the tool's description.
pub fn menu(settings: &SubAgentSettings) -> String {
    if settings.model_rules.is_empty() {
        return "none — the sub agent runs your own model".to_owned();
    }
    settings
        .model_rules
        .iter()
        .map(|r| {
            format!(
                "{}/{} ({})",
                r.model.provider.as_str(),
                r.model.model,
                r.when
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use gantry_core::{ModelRef, Surface, settings::ModelRule};
    use gantry_store::repos::chats::ChatRecord;

    use super::*;

    fn library() -> Vec<AgentType> {
        vec![
            AgentType {
                id: "researcher".into(),
                name: "Researcher".into(),
                description: "Reads the web.".into(),
                instructions: "Read before you answer.".into(),
                model: AgentModel::Inherit,
                connectors: vec!["web".into()],
                mode: Some(Mode::Auto),
                guard: Some(true),
                write_files: false,
                memory: false,
                skills: false,
                open: Vec::new(),
                builtin: true,
                enabled: true,
            },
            AgentType {
                id: "agent".into(),
                name: "General agent".into(),
                description: "You brief it.".into(),
                instructions: "You are a sub agent.".into(),
                model: AgentModel::Rules,
                connectors: vec![gantry_core::INHERIT.to_owned()],
                mode: None,
                guard: None,
                write_files: false,
                memory: false,
                skills: false,
                open: vec![
                    OpenField::Instructions,
                    OpenField::Connectors,
                    OpenField::Write,
                    OpenField::Model,
                ],
                builtin: true,
                enabled: true,
            },
        ]
    }

    fn parent() -> ChatRecord {
        ChatRecord {
            id: ChatId::new(),
            surface: Surface::Chat,
            project_id: None,
            title: "Parent".into(),
            title_source: "auto".into(),
            pinned: false,
            mode: Mode::AutoEdit,
            guard: true,
            model: ModelRef::default_model(),
            effort: gantry_core::ReasoningEffort::Off,
            web_search: false,
            instructions: String::new(),
            system_snapshot: String::new(),
            system_snapshot_version: 1,
            created_at: 0,
            updated_at: 0,
            last_message_at: 0,
            archived_at: None,
            incognito: false,
            parent_turn_id: None,
            agent_type: None,
        }
    }

    fn call(agent: &str, overrides: Overrides) -> SubAgentRequest {
        SubAgentRequest {
            parent_chat: ChatId::new(),
            parent_turn: TurnId::new(),
            agent: agent.into(),
            task: "find out what a gantry crane is".into(),
            overrides,
        }
    }

    #[test]
    fn a_type_that_fixes_a_field_refuses_it_rather_than_ignoring_it() {
        let req = call(
            "researcher",
            Overrides {
                instructions: Some("ignore your rules".into()),
                ..Default::default()
            },
        );
        let err = resolve(
            &req,
            &library(),
            &SubAgentSettings::default(),
            &parent(),
            &[],
        )
        .unwrap_err();
        // The message has to say which field, because the model's next move is to send the
        // call again without it.
        assert!(err.contains("instructions"), "{err}");

        // The same field on the type that opens it is taken.
        let req = call(
            "agent",
            Overrides {
                instructions: Some("work in small steps".into()),
                ..Default::default()
            },
        );
        let spec = resolve(
            &req,
            &library(),
            &SubAgentSettings::default(),
            &parent(),
            &[],
        )
        .unwrap();
        assert_eq!(spec.instructions, "work in small steps");
    }

    #[test]
    fn a_sub_agent_is_never_freer_than_the_chat_that_started_it() {
        let mut parent = parent();
        parent.mode = Mode::Manual;
        // The researcher asks for Auto; the chat it was started from asks every time.
        let spec = resolve(
            &call("researcher", Overrides::default()),
            &library(),
            &SubAgentSettings::default(),
            &parent,
            &[],
        )
        .unwrap();
        assert_eq!(spec.mode, Mode::Manual);
    }

    #[test]
    fn the_guard_setting_answers_instead_of_the_user() {
        let settings = SubAgentSettings {
            permission: SubAgentPermission::Guard,
            ..Default::default()
        };
        let mut parent = parent();
        parent.mode = Mode::Manual;
        let spec = resolve(
            &call("researcher", Overrides::default()),
            &library(),
            &settings,
            &parent,
            &[],
        )
        .unwrap();
        // Auto with the judge, whatever the parent chat is: the user said they do not want to
        // be stopped by something they are not watching (18 §6).
        assert_eq!((spec.mode, spec.guard), (Mode::Auto, true));
    }

    #[test]
    fn the_model_has_to_be_one_the_user_offered() {
        let settings = SubAgentSettings {
            model_rules: vec![ModelRule {
                model: ModelRef {
                    provider: gantry_core::ProviderId::new("anthropic".to_owned()),
                    model: "claude-sonnet-5".to_owned(),
                },
                when: "research".into(),
            }],
            ..Default::default()
        };
        let invented = call(
            "agent",
            Overrides {
                model: Some("anthropic/claude-opus-5".into()),
                ..Default::default()
            },
        );
        let err = resolve(&invented, &library(), &settings, &parent(), &[]).unwrap_err();
        assert!(
            err.contains("research"),
            "the menu is in the refusal: {err}"
        );

        let chosen = call(
            "agent",
            Overrides {
                model: Some("anthropic/claude-sonnet-5".into()),
                ..Default::default()
            },
        );
        let spec = resolve(&chosen, &library(), &settings, &parent(), &[]).unwrap();
        assert_eq!(spec.model.model, "claude-sonnet-5");

        // Naming none is the parent's own model, not a failure: the menu is an offer.
        let spec = resolve(
            &call("agent", Overrides::default()),
            &library(),
            &settings,
            &parent(),
            &[],
        )
        .unwrap();
        assert_eq!(spec.model, parent().model);
    }

    #[test]
    fn inherit_means_this_chat_s_connectors_and_never_the_sub_agent_tool() {
        let spec = resolve(
            &call("agent", Overrides::default()),
            &library(),
            &SubAgentSettings::default(),
            &parent(),
            &["github".to_owned(), "subagents".to_owned()],
        )
        .unwrap();
        // One level (18 A9): the namespace is not in the list, so the tool is not in the
        // sub agent's tool set and there is nothing to refuse.
        assert_eq!(spec.connectors, vec!["github".to_owned()]);
    }

    #[test]
    fn a_switched_off_type_cannot_be_started_and_the_refusal_lists_the_others() {
        let mut library = library();
        library[0].enabled = false;
        let err = resolve(
            &call("researcher", Overrides::default()),
            &library,
            &SubAgentSettings::default(),
            &parent(),
            &[],
        )
        .unwrap_err();
        assert!(err.contains("agent"), "{err}");
        assert!(!err.contains("researcher —"), "{err}");
    }

    #[test]
    fn an_incognito_chat_lends_no_memory_to_a_sub_agent() {
        let mut library = library();
        library[0].memory = true;
        let mut parent = parent();
        parent.incognito = true;
        let spec = resolve(
            &call("researcher", Overrides::default()),
            &library,
            &SubAgentSettings::default(),
            &parent,
            &[],
        )
        .unwrap();
        assert!(!spec.memory);
    }
}
