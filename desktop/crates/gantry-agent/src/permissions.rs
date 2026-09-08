//! The permission engine (docs/plan/04 §3): mode and tier decide, a standing grant can answer
//! a prompt the user already answered once, and the user is asked for everything else. The
//! guardrail floor (M7), scope checks (M6) and the judge (M8) slot in as they land; until the
//! judge exists, Guarded Auto asks the user for anything it would have sent to the judge,
//! which is the "fail closed" rule of 04 §1.

use gantry_core::{ChatGrant, CommandClass, DecisionSource, Mode, RiskTier, ToolDef, classify};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow(DecisionSource),
    Ask,
    Deny {
        source: DecisionSource,
        reason: String,
    },
}

/// What one call is: the tool, who owns it, and the arguments a grant may be scoped to.
pub struct Call<'a> {
    pub def: &'a ToolDef,
    pub instance_id: &'a str,
    pub args: &'a serde_json::Value,
}

/// The 04 §3 table plus the chat's standing grants (§8).
///
/// A grant may only turn **Ask** into an allow. It never lifts a denial, so Plan mode still
/// refuses what it refuses, and it never overrides `always_confirm`, so the tools that ask in
/// every mode keep asking.
#[must_use]
pub fn decide(mode: Mode, guard: bool, call: &Call<'_>, grants: &[ChatGrant]) -> Decision {
    let policy = match classified(call) {
        // A command the classifier proved read-only is decided as a read: that single fact is
        // what lets Plan mode allow `git status` while refusing everything else, and what stops
        // Auto-edit asking about `ls` (`docs/connectors/shell.md` §8).
        Some(CommandClass::ReadOnly) => {
            let mut as_read = call.def.clone();
            as_read.tier = RiskTier::Read;
            mode_policy(mode, guard, &as_read)
        }
        Some(CommandClass::Effectful(reason)) if mode == Mode::Plan => Decision::Deny {
            source: DecisionSource::PlanMode,
            reason: format!(
                "Plan mode only runs commands that are provably read-only, and {reason}. \
                 Propose the command instead, or switch to Auto-edit to run it."
            ),
        },
        _ => mode_policy(mode, guard, call.def),
    };
    if policy == Decision::Ask
        && !call.def.always_confirm
        && grants
            .iter()
            .any(|g| g.covers(call.instance_id, &call.def.name, call.def.tier, call.args))
    {
        return Decision::Allow(DecisionSource::UserChatGrant);
    }
    policy
}

/// The verdict on a call that carries a command line, and nothing for every other tool.
fn classified(call: &Call<'_>) -> Option<CommandClass> {
    if call.def.plan_mode != gantry_core::PlanModePolicy::Classify {
        return None;
    }
    let command = call.args.get("command")?.as_str()?;
    Some(classify(command))
}

/// The 04 §3 table for one call, before grants.
#[must_use]
pub fn mode_policy(mode: Mode, guard: bool, def: &ToolDef) -> Decision {
    use RiskTier::*;
    if def.tier == App {
        return Decision::Allow(DecisionSource::Mode);
    }
    let policy = match (mode, def.tier) {
        (Mode::Manual, _) => Decision::Ask,
        (Mode::AutoEdit, Read | Write) => Decision::Allow(DecisionSource::Mode),
        (Mode::AutoEdit, _) => Decision::Ask,
        (Mode::Plan, Read) => Decision::Ask,
        // A classifiable tool reaches this arm only when no command was given to classify —
        // the verdict itself is applied in `decide`, above.
        (Mode::Plan, Execute) if def.plan_mode == gantry_core::PlanModePolicy::Classify => {
            Decision::Ask
        }
        (Mode::Plan, _) => Decision::Deny {
            source: DecisionSource::PlanMode,
            reason: "Plan mode cannot change anything; propose the change instead.".into(),
        },
        (Mode::Auto, Read) => Decision::Allow(DecisionSource::Mode),
        (Mode::Auto, _) if !guard => Decision::Allow(DecisionSource::Mode),
        // The judge arrives with M8; until then a guarded call is the user's to decide.
        (Mode::Auto, _) => Decision::Ask,
    };
    if def.always_confirm && matches!(policy, Decision::Allow(_)) {
        return Decision::Ask;
    }
    policy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(tier: RiskTier) -> ToolDef {
        ToolDef::new("t", "d", serde_json::json!({}), tier)
    }

    fn decide(mode: Mode, guard: bool, def: &ToolDef) -> Decision {
        mode_policy(mode, guard, def)
    }

    #[test]
    fn the_mode_table_holds() {
        use RiskTier::*;
        assert_eq!(decide(Mode::Manual, true, &def(Read)), Decision::Ask);
        assert_eq!(
            decide(Mode::Manual, true, &def(App)),
            Decision::Allow(DecisionSource::Mode)
        );
        assert_eq!(
            decide(Mode::AutoEdit, true, &def(Write)),
            Decision::Allow(DecisionSource::Mode)
        );
        assert_eq!(
            decide(Mode::AutoEdit, true, &def(WriteExternal)),
            Decision::Ask
        );
        assert_eq!(decide(Mode::AutoEdit, true, &def(Execute)), Decision::Ask);
        assert_eq!(decide(Mode::Plan, true, &def(Read)), Decision::Ask);
        assert!(matches!(
            decide(Mode::Plan, true, &def(Write)),
            Decision::Deny {
                source: DecisionSource::PlanMode,
                ..
            }
        ));
        assert_eq!(
            decide(Mode::Auto, false, &def(Destructive)),
            Decision::Allow(DecisionSource::Mode)
        );
        assert_eq!(decide(Mode::Auto, true, &def(Write)), Decision::Ask);
        assert_eq!(
            decide(Mode::Auto, true, &def(Read)),
            Decision::Allow(DecisionSource::Mode)
        );
        let mut confirm = def(Read);
        confirm.always_confirm = true;
        assert_eq!(decide(Mode::Auto, false, &confirm), Decision::Ask);
    }

    fn grant(scope: gantry_core::GrantScope, tool: &str) -> ChatGrant {
        scope.grant(
            gantry_core::ChatId::new(),
            "fs",
            "Files",
            tool,
            gantry_core::GrantSource::UserPrompt,
        )
    }

    fn call<'a>(def: &'a ToolDef, args: &'a serde_json::Value) -> Call<'a> {
        Call {
            def,
            instance_id: "fs",
            args,
        }
    }

    #[test]
    fn a_grant_answers_a_prompt_the_user_already_answered() {
        use gantry_core::GrantScope;
        let args = serde_json::json!({});
        let read = def(RiskTier::Read);
        let grants = vec![grant(GrantScope::Tool, "t")];
        assert_eq!(
            super::decide(Mode::Manual, true, &call(&read, &args), &grants),
            Decision::Allow(DecisionSource::UserChatGrant)
        );
        assert_eq!(
            super::decide(Mode::Manual, true, &call(&read, &args), &[]),
            Decision::Ask
        );
    }

    #[test]
    fn a_grant_never_lifts_a_denial_or_an_always_confirm() {
        use gantry_core::GrantScope;
        let args = serde_json::json!({});
        let write = def(RiskTier::Write);
        let grants = vec![grant(GrantScope::Tool, "t")];
        assert!(matches!(
            super::decide(Mode::Plan, true, &call(&write, &args), &grants),
            Decision::Deny { .. }
        ));
        let mut confirm = def(RiskTier::Read);
        confirm.always_confirm = true;
        assert_eq!(
            super::decide(Mode::Manual, true, &call(&confirm, &args), &grants),
            Decision::Ask
        );
    }

    #[test]
    fn all_reads_covers_a_second_read_tool_but_not_a_write() {
        use gantry_core::GrantScope;
        let args = serde_json::json!({});
        let grants = vec![grant(GrantScope::AllReads, "other")];
        let read = def(RiskTier::Read);
        let write = def(RiskTier::Write);
        assert_eq!(
            super::decide(Mode::Manual, true, &call(&read, &args), &grants),
            Decision::Allow(DecisionSource::UserChatGrant)
        );
        assert_eq!(
            super::decide(Mode::Manual, true, &call(&write, &args), &grants),
            Decision::Ask
        );
    }

    /// The matrix of `docs/connectors/shell.md` §8, through the engine the runner calls.
    #[test]
    fn a_command_is_decided_by_what_the_classifier_could_prove() {
        use gantry_core::PlanModePolicy;
        let mut run = ToolDef::new("run_command", "d", serde_json::json!({}), RiskTier::Execute);
        run.plan_mode = PlanModePolicy::Classify;
        let call = |args: &'static str| serde_json::json!({ "command": args });

        let reads = call("git status");
        let writes = call("rm -rf build");
        let decide_with = |mode: Mode, guard: bool, args: &serde_json::Value| {
            super::decide(
                mode,
                guard,
                &Call {
                    def: &run,
                    instance_id: "shell",
                    args,
                },
                &[],
            )
        };

        // Read-only: allowed where a read is allowed, asked where a read is asked.
        assert_eq!(decide_with(Mode::Manual, true, &reads), Decision::Ask);
        assert_eq!(
            decide_with(Mode::AutoEdit, true, &reads),
            Decision::Allow(DecisionSource::Mode)
        );
        assert_eq!(decide_with(Mode::Plan, true, &reads), Decision::Ask);
        assert_eq!(
            decide_with(Mode::Auto, true, &reads),
            Decision::Allow(DecisionSource::Mode)
        );

        // Everything else: asked in Manual and Auto-edit, refused outright in Plan.
        assert_eq!(decide_with(Mode::Manual, true, &writes), Decision::Ask);
        assert_eq!(decide_with(Mode::AutoEdit, true, &writes), Decision::Ask);
        let Decision::Deny { source, reason } = decide_with(Mode::Plan, true, &writes) else {
            panic!("Plan mode must refuse a command it cannot prove read-only");
        };
        assert_eq!(source, DecisionSource::PlanMode);
        assert!(
            reason.contains("`rm` is not on the read-only list"),
            "{reason}"
        );
        assert_eq!(
            decide_with(Mode::Auto, false, &writes),
            Decision::Allow(DecisionSource::Mode),
            "unguarded Auto runs it"
        );
        assert_eq!(
            decide_with(Mode::Auto, true, &writes),
            Decision::Ask,
            "guarded Auto asks until the judge exists"
        );
    }

    #[test]
    fn a_tool_without_a_command_is_untouched_by_the_classifier() {
        let def = ToolDef::new("write_file", "d", serde_json::json!({}), RiskTier::Write);
        assert_eq!(
            super::decide(
                Mode::AutoEdit,
                true,
                &Call {
                    def: &def,
                    instance_id: "filesystem",
                    args: &serde_json::json!({ "path": "/tmp/x" })
                },
                &[]
            ),
            Decision::Allow(DecisionSource::Mode)
        );
    }
}
