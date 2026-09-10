//! First-party connector: Shell (`docs/connectors/shell.md`).
//!
//! Two tools: run a command, and stop one that is running. It is the most capable and the most
//! dangerous thing in Gantry — a command runs as the user, and once it is running nothing here
//! constrains what it does — so the honest parts of that are built in rather than described:
//! the working directory is an attached folder while the chat has one, the classifier says
//! whether the command line was *proved* read-only, output is capped with the loss counted, the
//! deadline is real, and a kill takes the whole process tree.

#![forbid(unsafe_code)]

mod env;
mod run;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use gantry_connectors::{
    Connector, ConnectorDescriptor, ConnectorError, ToolCallRequest, ToolEventSink, ToolOutcome,
};
use gantry_core::{ChatId, CommandClass, InstanceId, PlanModePolicy, RiskTier, ToolDef};
use gantry_workspace::{Workspace, WorkspaceError};
use tokio_util::sync::CancellationToken;

pub use env::ShellEnv;
pub use run::{DEFAULT_TIMEOUT_MS, Ended, MAX_TIMEOUT_MS};

/// The connector manifest, embedded at build time (`docs/plan/03-connector-system.md` §3).
pub const MANIFEST: &str = include_str!("../manifest.json");

/// The connector id; equals the folder name and the tool namespace prefix.
pub const ID: &str = "shell";

pub struct Shell {
    descriptor: ConnectorDescriptor,
    workspace: Arc<Workspace>,
    env: Arc<ShellEnv>,
    /// Commands in flight, so `kill_command` has something to reach.
    running: Mutex<HashMap<String, CancellationToken>>,
}

impl Shell {
    #[must_use]
    pub fn new(
        namespace: String,
        instance_id: InstanceId,
        workspace: Arc<Workspace>,
        env: Arc<ShellEnv>,
    ) -> Self {
        Self {
            descriptor: ConnectorDescriptor {
                id: namespace,
                name: "Shell".to_owned(),
                instance_id: Some(instance_id),
                first_party: true,
            },
            workspace,
            env,
            running: Mutex::new(HashMap::new()),
        }
    }

    /// Where the command runs: the `cwd` argument if it resolves inside an attached folder, and
    /// the chat's first folder otherwise (shell.md §2, open question 3 — defaulting, because a
    /// model that must name the folder every time names it wrongly).
    ///
    /// A chat with no folder attached runs in the home folder instead of refusing (shell.md D8,
    /// corrected 2026-09-10). "What hardware is in this machine" is a real question, and it has
    /// nothing to do with a project; sending the user off to attach a folder before `lscpu` may
    /// run is a rule with no purpose behind it.
    fn working_directory(&self, chat: ChatId, cwd: Option<&str>) -> Result<PathBuf, String> {
        let roots = match self.workspace.roots(chat) {
            Ok(roots) => roots,
            Err(WorkspaceError::NoRoots) => return self.home_directory(cwd),
            Err(err) => {
                return Err(format!(
                    "{err}, so there is nowhere to run a command. Attach a folder with the + \
                     button in the composer."
                ));
            }
        };
        match cwd {
            Some(path) => {
                let scoped = roots.resolve(path).map_err(|err| {
                    format!(
                        "{}\n\nThe working directory has to be inside a folder attached to this \
                         chat. Add it with the + button in the composer, or run the command in a \
                         folder that is already attached.",
                        err
                    )
                })?;
                Ok(scoped.path)
            }
            None => roots
                .primary()
                .map(std::borrow::ToOwned::to_owned)
                .ok_or_else(|| {
                    "this chat has no folder attached, so there is nowhere to run a command. \
                     Add one with the + button in the composer."
                        .to_owned()
                }),
        }
    }

    /// No folder is attached: the home folder, and an explicit `cwd` may name any directory that
    /// exists. Nothing is weakened by that — with nothing attached there is no boundary to hold,
    /// and a command line can `cd` wherever it likes in any case (shell.md §7).
    fn home_directory(&self, cwd: Option<&str>) -> Result<PathBuf, String> {
        let home = self.env.home().ok_or_else(|| {
            "this chat has no folder attached and your home folder could not be found, so there \
             is nowhere to run a command. Attach a folder with the + button in the composer."
                .to_owned()
        })?;
        let Some(path) = cwd else { return Ok(home) };
        // An absolute path replaces the home folder; a relative one is read from it, which is
        // what the same words would mean in a terminal.
        let candidate = home.join(path);
        candidate
            .canonicalize()
            .ok()
            .filter(|resolved| resolved.is_dir())
            .ok_or_else(|| {
                format!(
                    "{} is not a directory that exists. This chat has no folder attached, so the \
                     command would otherwise run in {}.",
                    candidate.display(),
                    home.display()
                )
            })
    }

    async fn run_command(
        &self,
        req: &ToolCallRequest,
        sink: Arc<dyn ToolEventSink>,
        cancel: CancellationToken,
    ) -> ToolOutcome {
        let Some(command) = string(&req.args, "command") else {
            return ToolOutcome::error("`command` is required.");
        };
        let cwd =
            match self.working_directory(req.scope.chat_id, string(&req.args, "cwd").as_deref()) {
                Ok(cwd) => cwd,
                Err(message) => return ToolOutcome::error(message),
            };
        let timeout_ms = req
            .args
            .get("timeout_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);
        let extra_env: Vec<(String, String)> = req
            .args
            .get("env")
            .and_then(serde_json::Value::as_object)
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())))
                    .collect()
            })
            .unwrap_or_default();

        // The same rule the permission engine applies (`permissions::classified`): an
        // environment can decide which program the command's words resolve to, so a call that
        // sets one is never reported as proven read-only.
        let class = if extra_env.is_empty() {
            gantry_core::classify(&command)
        } else {
            CommandClass::Effectful(
                "it sets environment variables, which can change what the command runs".to_owned(),
            )
        };
        let kill = CancellationToken::new();
        self.running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(req.call_id.as_str().to_owned(), kill.clone());

        let result = run::run(
            run::Job {
                env: &self.env,
                command: &command,
                cwd: &cwd,
                extra_env: &extra_env,
                timeout: Duration::from_millis(timeout_ms),
            },
            &req.call_id,
            sink,
            cancel,
            kill,
        )
        .await;

        self.running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(req.call_id.as_str());

        match result {
            Ok(output) => outcome(&command, &cwd, &class, &output, &self.env),
            Err(err) => ToolOutcome::error(start_failure(&command, &err, &self.env)),
        }
    }

    fn kill_command(&self, req: &ToolCallRequest) -> ToolOutcome {
        let Some(call_id) = string(&req.args, "call_id") else {
            return ToolOutcome::error("`call_id` is required.");
        };
        let token = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&call_id)
            .cloned();
        match token {
            Some(token) => {
                token.cancel();
                ToolOutcome::json(serde_json::json!({ "call_id": call_id, "killed": true }))
            }
            None => ToolOutcome::error(format!(
                "No command with call id `{call_id}` is running. It may have already finished."
            )),
        }
    }
}

#[async_trait]
impl Connector for Shell {
    fn descriptor(&self) -> &ConnectorDescriptor {
        &self.descriptor
    }

    async fn tools(&self) -> Result<Vec<ToolDef>, ConnectorError> {
        Ok(definitions())
    }

    async fn call(
        &self,
        req: ToolCallRequest,
        sink: Arc<dyn ToolEventSink>,
        cancel: CancellationToken,
    ) -> Result<ToolOutcome, ConnectorError> {
        match req.tool.as_str() {
            "run_command" => Ok(self.run_command(&req, sink, cancel).await),
            "kill_command" => Ok(self.kill_command(&req)),
            other => Err(ConnectorError::UnknownTool(other.to_owned())),
        }
    }
}

/// The result the model reads. A non-zero exit is a result, not an error (D5): a failing test
/// suite is exactly what was asked for, and calling it a malfunction teaches the model to
/// distrust real output.
fn outcome(
    command: &str,
    cwd: &std::path::Path,
    class: &CommandClass,
    output: &run::Output,
    env: &ShellEnv,
) -> ToolOutcome {
    let exit_code = match output.ended {
        Ended::Exited(code) => Some(code),
        _ => None,
    };
    let mut note = match output.ended {
        Ended::Exited(_) => String::new(),
        Ended::TimedOut => format!(
            "The command ran past its limit of {} s and was stopped, along with everything it \
             started. This tool cannot host long-running processes such as development servers \
             or file watchers; run one in your own terminal instead.",
            output.duration.as_secs()
        ),
        Ended::Killed => "The command was stopped before it finished.".to_owned(),
    };
    if matches!(output.ended, Ended::Exited(_))
        && output.stdout.text.is_empty()
        && output.stderr.text.is_empty()
        && output.duration >= Duration::from_secs(1)
    {
        // Standard input is closed (D3), so a command waiting for a person gets end-of-file and
        // usually exits at once with nothing to say. Saying which is more useful than silence.
        note.push_str(
            "The command produced no output. Standard input is closed, so a command that asks a \
             question ends immediately instead of waiting.",
        );
    }

    let structured = serde_json::json!({
        "command": command,
        "cwd": cwd.display().to_string(),
        "shell": env.label,
        "exit_code": exit_code,
        "stdout": output.stdout.for_model(),
        "stderr": output.stderr.for_model(),
        "duration_ms": u64::try_from(output.duration.as_millis()).unwrap_or(u64::MAX),
        "truncated": output.stdout.truncated() || output.stderr.truncated(),
        "killed": matches!(output.ended, Ended::Killed | Ended::TimedOut),
        "timed_out": output.ended == Ended::TimedOut,
        // Named for what it is: a property the classifier proved about the command *string*.
        "checked_read_only": class.is_read_only(),
        "note": note,
    });
    ToolOutcome::json(structured)
}

/// The command could not be started at all — the one case that is a tool error (D5).
fn start_failure(command: &str, err: &std::io::Error, env: &ShellEnv) -> String {
    let source = if env.from_login_shell {
        format!(
            "the environment captured from your login shell ({})",
            env.login_label
        )
    } else {
        "the app's own environment, because the login shell could not be read".to_owned()
    };
    format!(
        "The command could not be started: {err}\n\nCommand: {command}\nShell: {}\nPrograms are \
         looked up in {source}. A program that works in your terminal but not here is usually \
         one your shell adds to PATH in a file that only interactive shells read.",
        env.label
    )
}

fn string(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(std::borrow::ToOwned::to_owned)
}

/// The two tools (shell.md §2, §12). Neither is `parallel_safe`: two commands from one batch in
/// the same directory is a race the model did not intend and cannot reason about.
#[must_use]
pub fn definitions() -> Vec<ToolDef> {
    let mut run = ToolDef::new(
        "run_command",
        "Run one command line and wait for it to finish. Use it for builds, tests, git and \
         scripts. Prefer the file tools for reading and editing files: \
         their results are structured and their changes can be undone. Standard input is \
         closed, so interactive commands end instead of waiting, and long-running processes \
         such as development servers are not supported.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string",
                             "description": "The command line, exactly as it would be typed." },
                "cwd": { "type": "string",
                         "description": "Directory to run in. Defaults to the chat's first attached folder, or your home folder when the chat has none. While a folder is attached it must be inside one." },
                "timeout_ms": { "type": "integer", "minimum": 1000, "maximum": MAX_TIMEOUT_MS,
                                "default": DEFAULT_TIMEOUT_MS,
                                "description": "How long to wait before the command and its children are stopped." },
                "env": { "type": "object", "additionalProperties": { "type": "string" },
                         "description": "Extra environment variables for this command only." }
            },
            "required": ["command"],
            "additionalProperties": false
        }),
        RiskTier::Execute,
    );
    // Plan mode asks for a command the classifier proves read-only and refuses the rest, which
    // is what makes that mode usable rather than inert.
    run.plan_mode = PlanModePolicy::Classify;
    run.stream_args = true;

    let kill = ToolDef::new(
        "kill_command",
        "Stop a command this chat started, together with everything it started.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "call_id": { "type": "string", "description": "The id of the run_command call to stop." }
            },
            "required": ["call_id"],
            "additionalProperties": false
        }),
        RiskTier::Write,
    );

    vec![run, kill]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_valid_json_with_the_right_id() {
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(manifest["manifest_version"], "1");
        assert_eq!(manifest["id"], ID);
        assert_eq!(manifest["runtime"]["kind"], "native");
        assert_eq!(manifest["runtime"]["crate"], env!("CARGO_PKG_NAME"));
    }

    #[test]
    fn the_card_takes_its_tools_from_the_code() {
        // Like the other native connectors: one source for the tool list, so the card cannot
        // describe a tool the code does not have. `docs/connectors/shell.md` §12 listed them in
        // the manifest instead; this is the sibling connectors' pattern, and it is better.
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(manifest["tools_generated"], true);
        assert!(manifest.get("tools").is_none());
        assert_eq!(definitions().len(), 2);
    }

    #[test]
    fn the_manifest_says_what_the_connector_really_reaches() {
        let manifest: serde_json::Value = serde_json::from_str(MANIFEST).unwrap();
        // A command can reach the network, whatever the command is. Saying `none` here because
        // the connector itself opens no sockets would be true and misleading.
        assert_eq!(manifest["risk"]["network"], "internet");
        assert_eq!(manifest["risk"]["local_system"], "execute");
        assert_eq!(manifest["risk"]["default_tool_tier"], "execute");
    }

    #[test]
    fn neither_tool_runs_beside_another() {
        assert!(definitions().iter().all(|d| !d.parallel_safe));
    }

    #[test]
    fn running_a_command_is_classified_in_plan_mode() {
        let run = definitions().into_iter().next().unwrap();
        assert_eq!(run.tier, RiskTier::Execute);
        assert_eq!(run.plan_mode, PlanModePolicy::Classify);
    }
}
