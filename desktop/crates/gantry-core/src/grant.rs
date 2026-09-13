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
    ///
    /// **The prefix must end where a name ends.** A plain `starts_with` would let a grant over
    /// `/home/olav/dev` reach `/home/olav/development`, and one over `cargo test` reach
    /// `cargo testify` — two different places, granted by a coincidence of spelling. So the
    /// value either equals the prefix or continues it with the separator that scope uses: `/`
    /// for a path, a space for a command.
    #[must_use]
    pub fn holds(&self, args: &serde_json::Value) -> bool {
        let (field, prefix, separator) = match self {
            ArgScope::PathPrefix { prefix } => ("path", prefix.trim_end_matches('/'), '/'),
            ArgScope::CommandPrefix { prefix } => ("command", prefix.trim_end(), ' '),
        };
        if prefix.is_empty() {
            return false;
        }
        args.get(field)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|value| {
                let Some(rest) = value.strip_prefix(prefix) else {
                    return false;
                };
                rest.is_empty() || rest.starts_with(separator)
            })
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
///
/// The argument scopes carry the prefix the card will grant rather than leaving the user to
/// type one: the whole point is one click on an offer they can read, and a text field asking a
/// person to write a path correctly under a prompt is a worse permission model than asking
/// every time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrantScope {
    /// This tool, from this connector, for the rest of this chat.
    Tool,
    /// Every read from this connector for the rest of this chat.
    AllReads,
    /// This tool, for the rest of this chat, only where the path is under this directory.
    PathPrefix { prefix: String },
    /// This tool, for the rest of this chat, only for commands beginning with these words.
    CommandPrefix { prefix: String },
}

impl GrantScope {
    /// What a card may offer for one call. Only reads can be granted wholesale; the argument
    /// scopes are offered whenever the call carries an argument to scope to, at any tier —
    /// "every edit under `src/`" is the narrow answer to a prompt that would otherwise be
    /// answered with "every edit".
    ///
    /// Narrowest first, so the dropdown reads from tightest to widest and the eye stops at the
    /// first one that fits. "Allow once" is prepended by the view and stays the default.
    ///
    /// **A card only offers what the engine would honour.** The permission floor (04 §5) refuses
    /// to let a grant answer a guardrail, and `always_confirm` asks every time by definition —
    /// so on those prompts a standing scope would be a promise nobody keeps: the user picks
    /// "for this chat", the grant is written, and the identical call asks again on the next
    /// turn. The one exception is 04 §5's own: a *path* guardrail may be reached by an explicit
    /// grant, and explicit means scoped to the argument, so those cards keep the folder scope
    /// and lose the rest.
    #[must_use]
    pub fn for_call(
        tier: RiskTier,
        args: &serde_json::Value,
        guardrail: Option<crate::guardrail::GuardrailKind>,
        always_confirm: bool,
    ) -> Vec<GrantScope> {
        if tier == RiskTier::App || always_confirm {
            return Vec::new();
        }
        match guardrail {
            None => {}
            Some(crate::guardrail::GuardrailKind::Path) => {
                // Only the argument scopes, and only when there is an argument to name.
                return path_scope(args)
                    .map(|prefix| vec![GrantScope::PathPrefix { prefix }])
                    .unwrap_or_default();
            }
            Some(_) => return Vec::new(),
        }
        let mut scopes = Vec::new();
        if let Some(prefix) = path_scope(args) {
            scopes.push(GrantScope::PathPrefix { prefix });
        }
        if let Some(prefix) = command_scope(args) {
            scopes.push(GrantScope::CommandPrefix { prefix });
        }
        scopes.push(GrantScope::Tool);
        if tier == RiskTier::Read {
            scopes.push(GrantScope::AllReads);
        }
        scopes
    }

    /// The grant this scope creates for one call.
    #[must_use]
    pub fn grant(
        &self,
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
            // Every scope but `AllReads` is about one tool: an argument scope narrows a tool
            // rather than replacing the question of which tool it is.
            tool_name: match self {
                GrantScope::AllReads => None,
                _ => Some(tool_name.to_owned()),
            },
            tier_ceiling: match self {
                GrantScope::AllReads => Some(RiskTier::Read),
                _ => None,
            },
            arg_scope: match self {
                GrantScope::PathPrefix { prefix } => Some(ArgScope::PathPrefix {
                    prefix: prefix.clone(),
                }),
                GrantScope::CommandPrefix { prefix } => Some(ArgScope::CommandPrefix {
                    prefix: prefix.clone(),
                }),
                _ => None,
            },
            source,
            created_at: crate::time::now_ms(),
            revoked_at: None,
        }
    }
}

/// The directory to offer for a call carrying a `path`: the file's own parent.
///
/// The parent and not the workspace root, which is usually the whole repository — too wide to
/// be one click on a prompt about a single file. A call whose path is already a directory
/// offers that directory.
fn path_scope(args: &serde_json::Value) -> Option<String> {
    let path = args.get("path")?.as_str()?.trim_end_matches('/');
    if path.is_empty() || !path.contains('/') {
        return None;
    }
    let parent = &path[..path.rfind('/')?];
    // "/" itself is every file on the machine, which is not a scope.
    if parent.is_empty() {
        return None;
    }
    Some(parent.to_owned())
}

/// The words to offer for a call carrying a `command`: the program and its subcommand.
///
/// Two words rather than one, when the second is a subcommand rather than a flag or a path:
/// `cargo test` is a thing people mean to allow, `cargo` is that plus `cargo publish`. The
/// boundary rule in [`ArgScope::holds`] is what keeps `cargo test` from reaching `cargo
/// testify`.
fn command_scope(args: &serde_json::Value) -> Option<String> {
    let command = args.get("command")?.as_str()?.trim();
    let mut words = command.split_whitespace();
    let program = words.next()?;
    if program.is_empty() {
        return None;
    }
    let subcommand = words
        .next()
        .filter(|w| {
            w.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .filter(|w| !w.starts_with('-'));
    Some(match subcommand {
        Some(sub) => format!("{program} {sub}"),
        None => program.to_owned(),
    })
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
            GrantScope::for_call(RiskTier::Read, &json!({}), None, false),
            vec![GrantScope::Tool, GrantScope::AllReads]
        );
        assert_eq!(
            GrantScope::for_call(RiskTier::Destructive, &json!({}), None, false),
            vec![GrantScope::Tool]
        );
        assert!(GrantScope::for_call(RiskTier::App, &json!({}), None, false).is_empty());
    }

    #[test]
    fn a_call_with_a_path_is_offered_its_folder_first() {
        let scopes = GrantScope::for_call(
            RiskTier::Write,
            &json!({ "path": "/home/olav/dev/gantry/README.md" }),
            None,
            false,
        );
        assert_eq!(
            scopes,
            vec![
                GrantScope::PathPrefix {
                    prefix: "/home/olav/dev/gantry".into()
                },
                GrantScope::Tool,
            ],
            "narrowest first, and a write is never offered all-reads"
        );
    }

    #[test]
    fn a_command_is_offered_its_program_and_subcommand() {
        let scopes = GrantScope::for_call(
            RiskTier::Execute,
            &json!({ "command": "cargo test -p api" }),
            None,
            false,
        );
        assert_eq!(
            scopes,
            vec![
                GrantScope::CommandPrefix {
                    prefix: "cargo test".into()
                },
                GrantScope::Tool,
            ]
        );
        // A flag is not a subcommand, and neither is a path.
        assert_eq!(
            GrantScope::for_call(
                RiskTier::Execute,
                &json!({ "command": "ls -la" }),
                None,
                false
            ),
            vec![
                GrantScope::CommandPrefix {
                    prefix: "ls".into()
                },
                GrantScope::Tool
            ]
        );
        assert_eq!(
            GrantScope::for_call(
                RiskTier::Execute,
                &json!({ "command": "./scripts/build.sh" }),
                None,
                false,
            ),
            vec![
                GrantScope::CommandPrefix {
                    prefix: "./scripts/build.sh".into()
                },
                GrantScope::Tool
            ]
        );
    }

    #[test]
    fn nothing_is_offered_to_scope_to_when_there_is_nothing_to_scope() {
        // A path at the root is every file on the machine, which is not a scope.
        assert_eq!(
            GrantScope::for_call(RiskTier::Read, &json!({ "path": "/etc" }), None, false),
            vec![GrantScope::Tool, GrantScope::AllReads]
        );
        assert_eq!(
            GrantScope::for_call(RiskTier::Read, &json!({ "path": "README.md" }), None, false),
            vec![GrantScope::Tool, GrantScope::AllReads]
        );
    }

    #[test]
    fn a_card_offers_nothing_standing_where_a_grant_would_not_be_honoured() {
        use crate::guardrail::GuardrailKind;
        let args = json!({ "command": "git push --force origin main" });
        // The floor asks in every mode and a grant never answers it (04 §5), so "for this chat"
        // would write a grant that changes nothing and ask again on the next turn.
        assert!(
            GrantScope::for_call(
                RiskTier::Execute,
                &args,
                Some(GuardrailKind::Confirm),
                false
            )
            .is_empty()
        );
        assert!(GrantScope::for_call(RiskTier::Execute, &args, None, true).is_empty());

        // 04 §5's own exception: a sensitive path may be reached by an *explicit* grant, and
        // explicit means one scoped to the argument. So that card keeps the folder and nothing
        // wider.
        assert_eq!(
            GrantScope::for_call(
                RiskTier::Read,
                &json!({ "path": "/home/olav/.ssh/id_rsa" }),
                Some(GuardrailKind::Path),
                false
            ),
            vec![GrantScope::PathPrefix {
                prefix: "/home/olav/.ssh".into()
            }]
        );
    }

    #[test]
    fn a_folder_grant_covers_that_folder_and_that_tool() {
        let g = GrantScope::PathPrefix {
            prefix: "/home/olav/dev/gantry".into(),
        }
        .grant(
            ChatId::new(),
            "code-editor",
            "Code editor",
            "replace",
            GrantSource::UserPrompt,
        );
        let at = |p: &str| json!({ "path": p });
        assert!(g.covers(
            "code-editor",
            "replace",
            RiskTier::Write,
            &at("/home/olav/dev/gantry/src/main.rs")
        ));
        // Its own directory, named exactly.
        assert!(g.covers(
            "code-editor",
            "replace",
            RiskTier::Write,
            &at("/home/olav/dev/gantry")
        ));
        // A sibling whose name merely starts the same way.
        assert!(!g.covers(
            "code-editor",
            "replace",
            RiskTier::Write,
            &at("/home/olav/dev/gantry-old/x.rs")
        ));
        // Another tool of the same connector is a different question.
        assert!(!g.covers(
            "code-editor",
            "insert",
            RiskTier::Write,
            &at("/home/olav/dev/gantry/src/main.rs")
        ));
    }

    #[test]
    fn a_command_grant_stops_at_a_word_boundary() {
        let g = GrantScope::CommandPrefix {
            prefix: "cargo test".into(),
        }
        .grant(
            ChatId::new(),
            "shell",
            "Shell",
            "run_command",
            GrantSource::UserPrompt,
        );
        let run = |c: &str| json!({ "command": c });
        assert!(g.covers(
            "shell",
            "run_command",
            RiskTier::Execute,
            &run("cargo test")
        ));
        assert!(g.covers(
            "shell",
            "run_command",
            RiskTier::Execute,
            &run("cargo test -p api")
        ));
        assert!(!g.covers(
            "shell",
            "run_command",
            RiskTier::Execute,
            &run("cargo testify")
        ));
        assert!(!g.covers(
            "shell",
            "run_command",
            RiskTier::Execute,
            &run("cargo publish")
        ));
    }
}
