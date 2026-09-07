//! Risk tiers for discovered tools (docs/plan/03 §6). A server's annotations are hints, not
//! promises, so the mapping is deliberately pessimistic: what a server does not claim, it does
//! not get. `tool_overrides` in a manifest and the user's own overrides refine the result.

use gantry_core::RiskTier;
use rmcp::model::ToolAnnotations;

/// `destructiveHint` earns `destructive`; `readOnlyHint` earns `read`; anything else is judged
/// by where the tool runs — a remote server writes to somebody else's system, a local process
/// can do whatever the user can.
///
/// Destructive is tested first on purpose. The specification says the hint is meaningful only
/// when `readOnlyHint` is false, so a server setting both is confused, and the cost of reading
/// a confused server the cautious way is one extra confirmation.
#[must_use]
pub fn tier_for(annotations: Option<&ToolAnnotations>, remote: bool) -> RiskTier {
    let default = if remote {
        RiskTier::WriteExternal
    } else {
        RiskTier::Execute
    };
    let Some(a) = annotations else { return default };
    if a.destructive_hint == Some(true) {
        return RiskTier::Destructive;
    }
    if a.read_only_hint == Some(true) {
        return RiskTier::Read;
    }
    if a.open_world_hint == Some(true) {
        return RiskTier::WriteExternal;
    }
    default
}

#[cfg(test)]
mod tests {
    use super::*;

    fn annotations(read_only: Option<bool>, destructive: Option<bool>) -> ToolAnnotations {
        let mut a = ToolAnnotations::default();
        a.read_only_hint = read_only;
        a.destructive_hint = destructive;
        a
    }

    #[test]
    fn an_unannotated_tool_is_judged_by_where_it_runs() {
        assert_eq!(tier_for(None, true), RiskTier::WriteExternal);
        assert_eq!(tier_for(None, false), RiskTier::Execute);
    }

    #[test]
    fn a_read_only_tool_reads() {
        let a = annotations(Some(true), None);
        assert_eq!(tier_for(Some(&a), true), RiskTier::Read);
    }

    #[test]
    fn a_server_claiming_both_is_read_as_destructive() {
        let both = annotations(Some(true), Some(true));
        assert_eq!(tier_for(Some(&both), true), RiskTier::Destructive);
    }

    #[test]
    fn destructive_is_taken_at_its_word() {
        let a = annotations(Some(false), Some(true));
        assert_eq!(tier_for(Some(&a), true), RiskTier::Destructive);
    }
}
