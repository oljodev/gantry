//! The permission engine (docs/plan/04 §3): mode and tier decide; the user is asked when the
//! table says so. M3 ships the mode policy. Grants (M7), the guardrail floor (M7), scope
//! checks (M6) and the judge (M8) slot in front of the mode step as they land; until the judge
//! exists, Guarded Auto asks the user for anything it would have sent to the judge, which is
//! the "fail closed" rule of 04 §1.

use gantry_core::{DecisionSource, Mode, RiskTier, ToolDef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow(DecisionSource),
    Ask,
    Deny {
        source: DecisionSource,
        reason: String,
    },
}

/// The 04 §3 table for one call.
#[must_use]
pub fn decide(mode: Mode, guard: bool, def: &ToolDef) -> Decision {
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
}
