//! Standing permissions for one chat (docs/plan/04 §8). A grant is the user's answer to a
//! prompt, remembered: it turns a later **Ask** into an allow, and it can do nothing else.
//! It never lifts a denial, never overrides `always_confirm`, and never leaves its chat.

use serde::{Deserialize, Serialize};

use crate::{
    ids::{ChatId, GrantId},
    tool::RiskTier,
};

/// Where a grant came from (04 §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GrantSource {
    /// The user chose a scope on a permission card.
    UserPrompt,
    /// The user answered a mid-conversation access request (04 §9).
    AccessRequest,
    /// Inherited from the project the chat belongs to.
    ProjectDefault,
}

/// A predicate on the call's arguments (04 §8). The path and command forms wait for the tools
/// that produce them; both match by prefix on the named argument when it is a string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArgScope {
    PathPrefix { prefix: String },
    CommandPrefix { prefix: String },
}

impl ArgScope {
    /// Whether the call's arguments satisfy the predicate. A missing or non-string argument
    /// fails closed, because a grant may never widen itself by omission.
    #[must_use]
    pub fn holds(&self, args: &serde_json::Value) -> bool {
        let (field, prefix) = match self {
            ArgScope::PathPrefix { prefix } => ("path", prefix),
            ArgScope::CommandPrefix { prefix } => ("command", prefix),
        };
        args.get(field)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| value.starts_with(prefix.as_str()))
    }
}

/// One standing permission. `tool_name` unset means every tool of the instance; `tier_ceiling`
/// set means every tool at or under that tier, which is how "allow all reads" is expressed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ChatGrant {
    pub id: GrantId,
    pub chat_id: ChatId,
    /// The connector id, or `gantry` for the runtime tools.
    pub instance_id: String,
    pub instance_name: String,
    pub tool_name: Option<String>,
    pub tier_ceiling: Option<RiskTier>,
    pub arg_scope: Option<ArgScope>,
    pub source: GrantSource,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = Option<specta_typescript::Number>)]
    pub revoked_at: Option<i64>,
}

impl ChatGrant {
    /// Whether this grant covers one call. Revoked grants match nothing; `App` is never
    /// granted because it never asks.
    #[must_use]
    pub fn covers(
        &self,
        instance_id: &str,
        tool_name: &str,
        tier: RiskTier,
        args: &serde_json::Value,
    ) -> bool {
        if self.revoked_at.is_some() || tier == RiskTier::App {
            return false;
        }
        if self.instance_id != instance_id {
            return false;
        }
        if let Some(name) = &self.tool_name
            && name != tool_name
        {
            return false;
        }
        if let Some(ceiling) = self.tier_ceiling
            && tier.rank() > ceiling.rank()
        {
            return false;
        }
        self.arg_scope
            .as_ref()
            .is_none_or(|scope| scope.holds(args))
    }

    /// One line for the Permissions panel: what this grant allows.
    #[must_use]
    pub fn summary(&self) -> String {
        let what = match (&self.tool_name, self.tier_ceiling) {
            (Some(tool), _) => format!("`{tool}`"),
            (None, Some(ceiling)) => format!("every {} tool", ceiling.as_str()),
            (None, None) => "every tool".to_string(),
        };
        let where_ = match &self.arg_scope {
            Some(ArgScope::PathPrefix { prefix }) => format!(" under {prefix}"),
            Some(ArgScope::CommandPrefix { prefix }) => format!(" starting with {prefix}"),
            None => String::new(),
        };
        format!("{what} from {}{where_}", self.instance_name)
    }
}

/// The scopes a permission card can offer, in the order they are shown (04 §7, §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GrantScope {
    /// This tool, from this connector, for the rest of this chat.
    Tool,
    /// Every read from this connector for the rest of this chat.
    AllReads,
}

impl GrantScope {
    /// What a card may offer for a call at this tier. Only reads can be granted wholesale.
    #[must_use]
    pub fn for_tier(tier: RiskTier) -> Vec<GrantScope> {
        match tier {
            RiskTier::App => Vec::new(),
            RiskTier::Read => vec![GrantScope::Tool, GrantScope::AllReads],
            _ => vec![GrantScope::Tool],
        }
    }

    /// The grant this scope creates for one call.
    #[must_use]
    pub fn grant(
        self,
        chat_id: ChatId,
        instance_id: &str,
        instance_name: &str,
        tool_name: &str,
        source: GrantSource,
    ) -> ChatGrant {
        ChatGrant {
            id: GrantId::new(),
            chat_id,
            instance_id: instance_id.to_owned(),
            instance_name: instance_name.to_owned(),
            tool_name: match self {
                GrantScope::Tool => Some(tool_name.to_owned()),
                GrantScope::AllReads => None,
            },
            tier_ceiling: match self {
                GrantScope::Tool => None,
                GrantScope::AllReads => Some(RiskTier::Read),
            },
            arg_scope: None,
            source,
            created_at: crate::time::now_ms(),
            revoked_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn grant(scope: GrantScope) -> ChatGrant {
        scope.grant(
            ChatId::new(),
            "filesystem",
            "Filesystem",
            "read_file",
            GrantSource::UserPrompt,
        )
    }

    #[test]
    fn a_tool_grant_covers_only_that_tool() {
        let g = grant(GrantScope::Tool);
        assert!(g.covers("filesystem", "read_file", RiskTier::Read, &json!({})));
        assert!(!g.covers("filesystem", "write_file", RiskTier::Write, &json!({})));
        assert!(!g.covers("shell", "read_file", RiskTier::Read, &json!({})));
    }

    #[test]
    fn all_reads_covers_reads_and_stops_there() {
        let g = grant(GrantScope::AllReads);
        assert!(g.covers("filesystem", "glob", RiskTier::Read, &json!({})));
        assert!(!g.covers("filesystem", "write_file", RiskTier::Write, &json!({})));
        assert!(!g.covers("filesystem", "clock", RiskTier::App, &json!({})));
    }

    #[test]
    fn a_revoked_grant_covers_nothing() {
        let mut g = grant(GrantScope::Tool);
        g.revoked_at = Some(crate::time::now_ms());
        assert!(!g.covers("filesystem", "read_file", RiskTier::Read, &json!({})));
    }

    #[test]
    fn an_argument_scope_fails_closed() {
        let mut g = grant(GrantScope::Tool);
        g.arg_scope = Some(ArgScope::PathPrefix {
            prefix: "/home/olav/dev".into(),
        });
        assert!(g.covers(
            "filesystem",
            "read_file",
            RiskTier::Read,
            &json!({ "path": "/home/olav/dev/gantry/README.md" })
        ));
        assert!(!g.covers(
            "filesystem",
            "read_file",
            RiskTier::Read,
            &json!({ "path": "/etc/passwd" })
        ));
        assert!(!g.covers("filesystem", "read_file", RiskTier::Read, &json!({})));
    }

    #[test]
    fn the_offered_scopes_depend_on_the_tier() {
        assert_eq!(
            GrantScope::for_tier(RiskTier::Read),
            vec![GrantScope::Tool, GrantScope::AllReads]
        );
        assert_eq!(
            GrantScope::for_tier(RiskTier::Destructive),
            vec![GrantScope::Tool]
        );
        assert!(GrantScope::for_tier(RiskTier::App).is_empty());
    }
}
