//! The permission engine (docs/plan/04 §3): mode and tier decide, a standing grant can answer
//! a prompt the user already answered once, and the user is asked for everything else. The
//! guardrail floor (M7), scope checks (M6) and the judge (M8) slot in as they land; until the
//! judge exists, Guarded Auto asks the user for anything it would have sent to the judge,
//! which is the "fail closed" rule of 04 §1.

use gantry_core::{ChatGrant, DecisionSource, Mode, RiskTier, ToolDef};

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
    let policy = mode_policy(mode, guard, call.def);
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
}
