//! The guard (docs/plan/04 §6): in Auto mode a small fast model decides each non-read call, so
//! that a long task runs to the end without the user having to answer for every step of it.
//!
//! Three things keep it honest.
//!
//! **The rules decide first and for free.** The guardrail floor, the mode table and the loop
//! detector run in front of the judge (`permissions::decide`), so the judge is only asked about
//! the calls where the answer is genuinely a judgement. That also means a model can never talk
//! its way past `rm -rf /`: the floor refused before anything was asked.
//!
//! **It fails closed.** A timeout, a network error or an answer we cannot parse is not an
//! allow and not a deny; it is a question for the user (04 §1). One interruption in a rare
//! failure beats a silent allow, and beats a block the user did not understand.
//!
//! **Everything below the policy is data.** The task, the action and the history are text the
//! model produced or the user wrote, and the policy says so in as many words. The judge is the
//! one place in Gantry where untrusted content is examined *as* untrusted content, which is
//! exactly where an instruction hidden in a file would try to be obeyed.

use std::{sync::Arc, time::Duration, time::Instant};

use futures_util::StreamExt;
use gantry_core::{
    JudgeDecision, JudgeFlag, JudgeSource, JudgeVerdict, Message, Mode, ProviderKind,
    ReasoningEffort, RiskTier,
};
use gantry_providers::{ChatRequest, Provider, StreamEvent};
use serde_json::{Value, json};

/// The cached policy prefix (04 §6). It is the whole system prompt, so the provider's prompt
/// cache sees the same prefix on every decision of every chat.
pub const POLICY: &str = include_str!("../../../assets/prompts/judge.md");

/// How long the judge may take before the user is asked instead (04 §6).
///
/// Thirty seconds, raised from eight on 2026-09-11 after the second live run timed out on a
/// single question. The number is a bet about which failure costs more. A timeout does not save
/// anybody time: it hands the call to the user, who then reads the command and answers it — so
/// giving up early does not shorten the wait, it only replaces a wait that ends in an answer
/// with a wait that ends in homework. A cheap fast route that is busy usually answers in a
/// second or two and sometimes takes twenty, and eight seconds was inside the range it wanted
/// rather than outside it.
pub const TIMEOUT: Duration = Duration::from_secs(30);
/// How many times one decision is attempted, inside [`TIMEOUT`] (04 §6).
pub const ATTEMPTS: u32 = 2;
/// A `destructive` call allowed below this confidence becomes a prompt instead (04 §6).
pub const DESTRUCTIVE_FLOOR: f32 = 0.7;
/// The same tool with the same arguments failing this many times in one turn is a loop.
pub const LOOP_FAILURES: u32 = 3;
/// How many decisions may be in flight at once (04 §6).
///
/// A model can ask for eight commands in one breath, and eight simultaneous requests to the
/// cheap fast route of a provider is how you find its rate limiter. Three keeps a batch's
/// latency close to one decision's while staying well inside what any provider minds — and a
/// throttled guard is not a slow guard, it is a guard that times out and hands every call to
/// the user, which is the opposite of what Auto mode is for.
pub const MAX_IN_FLIGHT: usize = 3;

const FIRST_MESSAGE_CHARS: usize = 600;
const LAST_MESSAGE_CHARS: usize = 600;
const INTENT_CHARS: usize = 400;
const ARGS_CHARS: usize = 1_500;
const PREVIEW_LINES: usize = 40;
const RECENT_CALLS: usize = 10;
const RECENT_ARGS_CHARS: usize = 120;
/// What one decision may cost in output. The answer is one small object.
const MAX_OUTPUT_TOKENS: u32 = 400;

/// What the task is, in the user's and the assistant's own words (04 §6, "task frame").
#[derive(Debug, Clone)]
pub struct Frame {
    pub project: Option<String>,
    pub first_user: String,
    pub last_user: String,
    /// The assistant's most recent text before this call: why it says it is doing this.
    pub intent: Option<String>,
    pub roots: Vec<String>,
    pub mode: Mode,
}

/// The one call being decided.
#[derive(Debug, Clone)]
pub struct Action {
    pub connector: String,
    pub tool: String,
    pub description: String,
    pub tier: RiskTier,
    pub args: Value,
    /// What the call would do, computed before it runs: the diff for an edit (04 §6). `None`
    /// when the connector cannot say, which is most of them.
    pub preview: Option<String>,
}

/// One earlier call of this turn, as the judge sees it.
#[derive(Debug, Clone)]
pub struct Recent {
    pub tool: String,
    pub args: String,
    pub outcome: &'static str,
    pub decision: String,
}

/// The whole judge input, rendered once so it can be tested as text.
#[must_use]
pub fn render(frame: &Frame, action: &Action, recent: &[Recent], denials: u32) -> String {
    let mut out = String::new();
    out.push_str("<task>\n");
    if let Some(project) = &frame.project {
        out.push_str(&format!("Project: {project}\n"));
    }
    out.push_str(&format!(
        "What the user first asked: {}\n",
        excerpt(&frame.first_user, FIRST_MESSAGE_CHARS)
    ));
    if frame.last_user != frame.first_user {
        out.push_str(&format!(
            "What the user last said: {}\n",
            excerpt(&frame.last_user, LAST_MESSAGE_CHARS)
        ));
    }
    match &frame.intent {
        Some(intent) if !intent.trim().is_empty() => out.push_str(&format!(
            "What the assistant said it is doing: {}\n",
            excerpt(intent, INTENT_CHARS)
        )),
        _ => out.push_str("The assistant said nothing before this call.\n"),
    }
    out.push_str("</task>\n\n<workspace>\n");
    if frame.roots.is_empty() {
        out.push_str("Roots: none — this chat has no folder, so nothing local is in scope.\n");
    } else {
        out.push_str(&format!("Roots: {}\n", frame.roots.join(", ")));
    }
    out.push_str(&format!("Permission mode: {}\n", mode_name(frame.mode)));
    out.push_str("</workspace>\n\n<action>\n");
    out.push_str(&format!(
        "Connector: {}\nTool: {} ({})\nWhat the tool does: {}\n",
        action.connector,
        action.tool,
        tier_name(action.tier),
        excerpt(&action.description, 200)
    ));
    out.push_str(&render_args(&action.args));
    if let Some(preview) = &action.preview {
        out.push_str("Dry run of the change:\n");
        out.push_str(&first_lines(preview, PREVIEW_LINES));
        out.push('\n');
    }
    out.push_str("</action>\n\n<recent>\n");
    if recent.is_empty() {
        out.push_str("This is the first tool call of the turn.\n");
    } else {
        for (i, r) in recent.iter().rev().take(RECENT_CALLS).rev().enumerate() {
            out.push_str(&format!(
                "{}. {} {} — {}, {}\n",
                i + 1,
                r.tool,
                excerpt(&r.args, RECENT_ARGS_CHARS),
                r.outcome,
                r.decision
            ));
        }
    }
    out.push_str(&format!("Denied so far this turn: {denials}\n"));
    out.push_str("</recent>\n\nDecide about the action above. JSON only.");
    out
}

/// The shell's own shape first, because a command and its working directory are the whole
/// question and JSON around them only makes them harder to read; everything else as JSON.
fn render_args(args: &Value) -> String {
    if let Some(command) = args.get("command").and_then(Value::as_str) {
        let mut out = format!("Command: {}\n", excerpt(command, ARGS_CHARS));
        if let Some(cwd) = args.get("cwd").and_then(Value::as_str) {
            out.push_str(&format!("Working directory: {cwd}\n"));
        }
        if let Some(env) = args.get("env").and_then(Value::as_object)
            && !env.is_empty()
        {
            out.push_str(&format!(
                "Environment it sets: {}\n",
                env.keys().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        return out;
    }
    let rendered = match args {
        Value::Null => "none".to_owned(),
        other => other.to_string(),
    };
    format!("Arguments: {}\n", excerpt(&rendered, ARGS_CHARS))
}

/// The answer's shape, for a provider that can be told it (04 §6). The same schema is described
/// in the policy prompt, because most providers cannot.
#[must_use]
pub fn schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["decision", "confidence", "reason", "flags"],
        "properties": {
            "decision": { "type": "string", "enum": ["allow", "deny"] },
            "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
            "reason": { "type": "string", "maxLength": gantry_core::judge::MAX_REASON_CHARS },
            "flags": {
                "type": "array",
                "items": {
                    "type": "string",
                    "enum": ["irreversible", "outside_task", "secret_exposure", "loop",
                             "suspicious_input"]
                }
            }
        }
    })
}

/// The wire shape that asks for [`schema`], as `provider_options` so that no client has to
/// learn about the judge (02 §2). `Null` where we cannot ask for it safely:
///
/// - **Gemini** puts its response schema inside `generation_config`, which the client already
///   fills in. Overwriting that key would silently drop the token limit with it, so Gemini is
///   asked in the prompt like every provider that has no schema support at all.
/// - Anthropic's `output_config` is written by the client only when reasoning is on, and the
///   judge turns reasoning off, so there is nothing to collide with.
#[must_use]
pub fn structured_output(kind: ProviderKind) -> Value {
    let schema = schema();
    match kind {
        ProviderKind::OpenAiChat => json!({
            "response_format": {
                "type": "json_schema",
                "json_schema": { "name": "guard_decision", "strict": true, "schema": schema }
            }
        }),
        ProviderKind::OpenAiResponses => json!({
            "text": {
                "format": {
                    "type": "json_schema", "name": "guard_decision",
                    "strict": true, "schema": schema
                }
            }
        }),
        ProviderKind::Anthropic => json!({
            "output_config": { "format": { "type": "json_schema", "schema": schema } }
        }),
        ProviderKind::Gemini => Value::Null,
    }
}

/// Why a decision could not be reached. Every one of them ends in a question for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JudgeError {
    /// No provider, or no model to ask.
    Unavailable,
    Timeout,
    Provider(String),
    /// The model answered, but not with a verdict.
    Unreadable(String),
}

impl std::fmt::Display for JudgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JudgeError::Unavailable => f.write_str("the guard has no model to ask"),
            JudgeError::Timeout => write!(f, "the guard did not answer in {}s", TIMEOUT.as_secs()),
            JudgeError::Provider(e) => write!(f, "the guard could not be reached: {e}"),
            JudgeError::Unreadable(e) => write!(f, "the guard's answer was unreadable: {e}"),
        }
    }
}

/// One decision. Costs a few hundred tokens, almost all of them a cached prefix.
pub async fn decide(
    provider: Arc<dyn Provider>,
    model: String,
    input: String,
) -> Result<JudgeVerdict, JudgeError> {
    let started = Instant::now();
    let structured = provider
        .model_info(&model)
        .is_some_and(|m| m.capabilities.structured_output);
    let mut req = ChatRequest::new(model.clone(), POLICY, vec![Message::user_text(input)]);
    req.max_output_tokens = MAX_OUTPUT_TOKENS;
    req.reasoning = ReasoningEffort::Off;
    // At eight seconds a retry was not worth its backoff: it spent the budget to arrive at the
    // same answer, late. At thirty, one retry costs about a second and rescues the commonest
    // way the guard fails — a rate limiter on the cheap fast route — while the timeout above
    // still stops the whole thing from outstaying its welcome.
    req.retries = ATTEMPTS;
    if structured {
        req.provider_options = structured_output(provider.kind());
    }
    let text = match tokio::time::timeout(TIMEOUT, collect(provider, req)).await {
        Err(_) => return Err(JudgeError::Timeout),
        Ok(Err(err)) => return Err(JudgeError::Provider(err.to_string())),
        Ok(Ok(text)) => text,
    };
    let mut verdict = parse(&text).map_err(JudgeError::Unreadable)?;
    verdict.model = model;
    verdict.latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    Ok(verdict)
}

async fn collect(
    provider: Arc<dyn Provider>,
    req: ChatRequest,
) -> Result<String, gantry_providers::ProviderError> {
    let mut stream = provider.stream(req).await?;
    let mut text = String::new();
    while let Some(ev) = stream.next().await {
        match ev? {
            StreamEvent::TextDelta { text: t, .. } => text.push_str(&t),
            StreamEvent::MessageEnd { .. } => break,
            _ => {}
        }
    }
    Ok(text)
}

/// The answer, read strictly: the decision and the reason must both be there, because a
/// verdict with either one missing is not a verdict. Everything else has a sane default.
///
/// The object is found rather than assumed: a model told to reply with JSON only will still
/// sometimes fence it or introduce it, and refusing a good decision over a code fence would
/// turn a correct answer into a prompt the user did not need.
pub fn parse(text: &str) -> Result<JudgeVerdict, String> {
    let raw = find_object(text).ok_or_else(|| short(text))?;
    let value: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let decision = match value.get("decision").and_then(Value::as_str) {
        Some(d) if d.eq_ignore_ascii_case("allow") => JudgeDecision::Allow,
        Some(d) if d.eq_ignore_ascii_case("deny") || d.eq_ignore_ascii_case("block") => {
            JudgeDecision::Deny
        }
        _ => return Err(format!("no decision in {}", short(raw))),
    };
    let reason = value
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .ok_or_else(|| format!("no reason in {}", short(raw)))?;
    let confidence = value
        .get("confidence")
        .and_then(Value::as_f64)
        .map_or(0.5, |c| c.clamp(0.0, 1.0) as f32);
    let flags = value
        .get("flags")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter_map(flag)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(JudgeVerdict {
        decision,
        confidence,
        reason: gantry_core::judge::cut(reason),
        flags,
        source: JudgeSource::Model,
        model: String::new(),
        latency_ms: 0,
        overridden: false,
        wrong: None,
    })
}

fn flag(name: &str) -> Option<JudgeFlag> {
    match name.trim().to_ascii_lowercase().as_str() {
        "irreversible" => Some(JudgeFlag::Irreversible),
        "outside_task" | "outside task" => Some(JudgeFlag::OutsideTask),
        "secret_exposure" | "secret" => Some(JudgeFlag::SecretExposure),
        "loop" => Some(JudgeFlag::Loop),
        "suspicious_input" | "suspicious" => Some(JudgeFlag::SuspiciousInput),
        _ => None,
    }
}

/// The first balanced `{…}` outside a string, so a brace inside the reason cannot end it early.
fn find_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, &c) in bytes.iter().enumerate().skip(start) {
        if in_string {
            match c {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return text.get(start..=i);
                }
            }
            _ => {}
        }
    }
    None
}

fn short(text: &str) -> String {
    excerpt(text.trim(), 120)
}

fn excerpt(text: &str, max: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max {
        return one_line;
    }
    let mut out: String = one_line.chars().take(max).collect();
    out.push('…');
    out
}

fn first_lines(text: &str, max: usize) -> String {
    let mut out: String = text.lines().take(max).collect::<Vec<_>>().join("\n");
    if text.lines().count() > max {
        out.push_str("\n… (the rest of the diff is not shown)");
    }
    out
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Manual => "manual",
        Mode::AutoEdit => "auto-edit",
        Mode::Plan => "plan",
        Mode::Auto => "auto",
    }
}

fn tier_name(tier: RiskTier) -> &'static str {
    match tier {
        RiskTier::Read => "read: observes, changes nothing",
        RiskTier::Write => "write: changes a local file Gantry can revert",
        RiskTier::WriteExternal => "write_external: changes something Gantry cannot revert",
        RiskTier::Execute => "execute: runs code",
        RiskTier::Destructive => "destructive: deletes or forces, irreversibly",
        RiskTier::App => "app: touches only Gantry's own state",
    }
}

/// The loop detector of 04 §6, rule 4: the same tool with the same arguments, failing over and
/// over, is the one thing a judge should not be asked about — the answer is always the same and
/// the model is not learning anything by being told again.
#[derive(Debug, Default)]
pub struct LoopTracker {
    failures: std::collections::HashMap<(String, String), u32>,
}

impl LoopTracker {
    /// Records that a call failed. Arguments are compared as their canonical JSON text, so
    /// whitespace and key order do not make two identical calls look different.
    pub fn failed(&mut self, tool: &str, args: &Value) {
        *self
            .failures
            .entry((tool.to_owned(), args.to_string()))
            .or_default() += 1;
    }

    /// The verdict when this call has already failed [`LOOP_FAILURES`] times, and nothing
    /// otherwise.
    #[must_use]
    pub fn verdict(&self, tool: &str, args: &Value) -> Option<JudgeVerdict> {
        let count = *self.failures.get(&(tool.to_owned(), args.to_string()))?;
        if count < LOOP_FAILURES {
            return None;
        }
        Some(JudgeVerdict::from_rule(
            JudgeDecision::Deny,
            JudgeSource::Loop,
            format!(
                "This exact call has already failed {count} times in this turn. Repeating it \
                 will not make it work; change the approach or ask the user."
            ),
            vec![JudgeFlag::Loop],
        ))
    }
}

/// **Allow anyway** (04 §6): the user's answer to a block, remembered until that exact call is
/// made again and then forgotten.
///
/// It is deliberately not stored. An override is a decision about one action in one moment, and
/// a decision like that should not survive a restart the user did not connect it to — coming
/// back tomorrow to find yesterday's override still standing is exactly the surprise the guard
/// exists to prevent. If the model does not make the call again, the override simply expires.
#[derive(Default)]
pub struct Overrides {
    inner: std::sync::Mutex<Vec<(gantry_core::ChatId, String, String)>>,
}

impl Overrides {
    /// Remembers that the user allowed this call after the guard blocked it.
    pub fn add(&self, chat_id: gantry_core::ChatId, tool: &str, args: &Value) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = (chat_id, tool.to_owned(), args.to_string());
        if !inner.contains(&entry) {
            inner.push(entry);
        }
    }

    /// Whether this call was overridden, consuming the override if it was. It must be the same
    /// call the user looked at: the same tool with the same arguments, in the same chat.
    pub fn take(&self, chat_id: gantry_core::ChatId, tool: &str, args: &Value) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let args = args.to_string();
        let Some(i) = inner
            .iter()
            .position(|(c, t, a)| *c == chat_id && t == tool && *a == args)
        else {
            return false;
        };
        inner.remove(i);
        true
    }
}

/// The last rule of 04 §6: a `destructive` call the judge allowed without being sure is not an
/// allow. It becomes the user's question, which is what "when confidence is below 0.7, do not
/// allow" means with the fail-closed rule of §1 applied to it.
#[must_use]
pub fn needs_the_user(verdict: &JudgeVerdict, tier: RiskTier) -> bool {
    verdict.allows() && tier == RiskTier::Destructive && verdict.confidence < DESTRUCTIVE_FLOOR
}

#[cfg(test)]
mod tests {
    /// The retries have to fit inside the budget with room to spare, or the extra attempt is a
    /// timeout with extra steps: it waits out its own backoff and is cut off mid-answer, on
    /// exactly the slow route that made the retry necessary.
    #[test]
    fn the_budget_leaves_room_for_the_retry_it_asks_for() {
        // `retry::backoff` sleeps 500 ms plus up to 250 ms of jitter before each retry.
        let worst = super::Duration::from_millis(750) * (super::ATTEMPTS - 1);
        assert!(
            super::TIMEOUT.saturating_sub(worst) >= super::Duration::from_secs(20),
            "{} attempts back off for {worst:?}, leaving too little of {:?} to answer in",
            super::ATTEMPTS,
            super::TIMEOUT,
        );
    }

    use super::*;

    fn action(args: Value) -> Action {
        Action {
            connector: "shell".into(),
            tool: "run_command".into(),
            description: "Runs a command in the workspace.".into(),
            tier: RiskTier::Execute,
            args,
            preview: None,
        }
    }

    fn frame() -> Frame {
        Frame {
            project: None,
            first_user: "Clean the build and run the tests".into(),
            last_user: "Clean the build and run the tests".into(),
            intent: Some("I'll remove the build folder first.".into()),
            roots: vec!["/home/olav/dev/gantry".into()],
            mode: Mode::Auto,
        }
    }

    #[test]
    fn a_command_is_shown_as_a_command_and_not_as_json() {
        let input = render(
            &frame(),
            &action(json!({ "command": "rm -rf build", "cwd": "/home/olav/dev/gantry" })),
            &[],
            0,
        );
        assert!(input.contains("Command: rm -rf build"), "{input}");
        assert!(input.contains("Working directory: /home/olav/dev/gantry"));
        assert!(input.contains("This is the first tool call of the turn."));
        assert!(input.contains("Denied so far this turn: 0"));
        // The first message is not repeated as the last when they are the same one.
        assert_eq!(input.matches("Clean the build").count(), 1, "{input}");
    }

    #[test]
    fn an_environment_is_named_because_it_decides_what_the_command_is() {
        let input = render(
            &frame(),
            &action(json!({ "command": "ls", "env": { "PATH": "/tmp/mine" } })),
            &[],
            0,
        );
        assert!(input.contains("Environment it sets: PATH"), "{input}");
    }

    #[test]
    fn only_the_last_ten_calls_are_sent() {
        let recent: Vec<Recent> = (0..14)
            .map(|i| Recent {
                tool: format!("t{i}"),
                args: String::new(),
                outcome: "ok",
                decision: "allowed by the guard".into(),
            })
            .collect();
        let input = render(&frame(), &action(json!({})), &recent, 2);
        assert!(!input.contains("t3 "), "the oldest calls are dropped");
        assert!(input.contains("t4"), "{input}");
        assert!(input.contains("t13"));
        assert!(input.contains("Denied so far this turn: 2"));
    }

    #[test]
    fn a_dry_run_is_cut_to_forty_lines() {
        let mut a = action(json!({ "path": "src/lib.rs" }));
        a.preview = Some(
            (0..60)
                .map(|i| format!("+line {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let input = render(&frame(), &a, &[], 0);
        assert!(input.contains("+line 39"), "{input}");
        assert!(!input.contains("+line 40"));
        assert!(input.contains("the rest of the diff is not shown"));
    }

    #[test]
    fn a_plain_answer_is_read() {
        let v = parse(
            r#"{"decision":"allow","confidence":0.9,"reason":"Builds the project","flags":[]}"#,
        )
        .expect("a verdict");
        assert!(v.allows());
        assert_eq!(v.confidence, 0.9);
        assert_eq!(v.reason, "Builds the project");
        assert_eq!(v.source, JudgeSource::Model);
    }

    /// Told "JSON only", a small model still fences it, introduces it, or puts a brace in the
    /// reason. None of those is a reason to interrupt the user.
    #[test]
    fn a_fenced_or_introduced_answer_is_still_read() {
        let fenced = "Here is my decision:\n```json\n{\"decision\": \"deny\", \"confidence\": 0.8,\n \"reason\": \"Deletes {config} nobody asked about\", \"flags\": [\"irreversible\"]}\n```\nThat's it.";
        let v = parse(fenced).expect("a verdict");
        assert!(!v.allows());
        assert_eq!(v.reason, "Deletes {config} nobody asked about");
        assert_eq!(v.flags, vec![JudgeFlag::Irreversible]);
    }

    #[test]
    fn an_answer_that_is_not_a_verdict_is_an_error_rather_than_a_guess() {
        assert!(parse("I think that's fine, go ahead.").is_err());
        assert!(parse(r#"{"confidence":0.9,"reason":"fine"}"#).is_err());
        assert!(
            parse(r#"{"decision":"allow","flags":[]}"#).is_err(),
            "a verdict with no reason is not a verdict"
        );
        // An unknown flag is dropped; it is not a reason to throw the decision away.
        let v = parse(r#"{"decision":"deny","reason":"no","flags":["vibes","loop"]}"#).unwrap();
        assert_eq!(v.flags, vec![JudgeFlag::Loop]);
        assert_eq!(
            v.confidence, 0.5,
            "an unstated confidence is neither sure nor unsure"
        );
    }

    #[test]
    fn a_long_reason_is_cut_to_the_length_the_policy_asked_for() {
        let long = "x".repeat(500);
        let v = parse(&format!(r#"{{"decision":"deny","reason":"{long}"}}"#)).unwrap();
        assert_eq!(
            v.reason.chars().count(),
            gantry_core::judge::MAX_REASON_CHARS
        );
    }

    #[test]
    fn the_same_failing_call_three_times_is_a_loop() {
        let mut t = LoopTracker::default();
        let args = json!({ "command": "cargo build" });
        // The same arguments written differently are the same call.
        let same = json!({ "command": "cargo build" });
        assert!(t.verdict("shell__run_command", &args).is_none());
        t.failed("shell__run_command", &args);
        t.failed("shell__run_command", &same);
        assert!(
            t.verdict("shell__run_command", &args).is_none(),
            "twice is not a loop"
        );
        t.failed("shell__run_command", &args);
        let v = t.verdict("shell__run_command", &args).expect("a loop");
        assert_eq!(v.source, JudgeSource::Loop);
        assert!(!v.allows());
        assert_eq!(v.flags, vec![JudgeFlag::Loop]);
        // A different call is not the same call.
        assert!(
            t.verdict("shell__run_command", &json!({ "command": "cargo test" }))
                .is_none()
        );
    }

    #[test]
    fn an_override_is_spent_on_the_call_it_was_given_for() {
        let chat = gantry_core::ChatId::new();
        let other = gantry_core::ChatId::new();
        let args = json!({ "command": "git push --force" });
        let o = Overrides::default();
        o.add(chat, "shell__run_command", &args);
        assert!(
            !o.take(other, "shell__run_command", &args),
            "another chat's"
        );
        assert!(!o.take(
            chat,
            "shell__run_command",
            &json!({ "command": "git push" })
        ));
        assert!(!o.take(chat, "fake__write", &args));
        assert!(o.take(chat, "shell__run_command", &args));
        assert!(
            !o.take(chat, "shell__run_command", &args),
            "an override answers one call and is then gone"
        );
    }

    #[test]
    fn an_unsure_allow_on_a_destructive_call_goes_to_the_user() {
        let sure = parse(r#"{"decision":"allow","confidence":0.95,"reason":"asked for"}"#).unwrap();
        let unsure = parse(r#"{"decision":"allow","confidence":0.4,"reason":"probably"}"#).unwrap();
        let denied = parse(r#"{"decision":"deny","confidence":0.4,"reason":"no"}"#).unwrap();
        assert!(needs_the_user(&unsure, RiskTier::Destructive));
        assert!(!needs_the_user(&sure, RiskTier::Destructive));
        assert!(
            !needs_the_user(&unsure, RiskTier::Write),
            "an ordinary write is allowed on the judge's word"
        );
        assert!(
            !needs_the_user(&denied, RiskTier::Destructive),
            "a deny is a deny however sure it was"
        );
    }

    #[test]
    fn the_schema_is_asked_for_where_the_wire_shape_is_ours_to_set() {
        let chat = structured_output(ProviderKind::OpenAiChat);
        assert_eq!(chat["response_format"]["type"], "json_schema");
        assert_eq!(chat["response_format"]["json_schema"]["strict"], true);
        assert_eq!(
            chat["response_format"]["json_schema"]["schema"]["properties"]["decision"]["enum"][1],
            "deny"
        );
        assert_eq!(
            structured_output(ProviderKind::Anthropic)["output_config"]["format"]["type"],
            "json_schema"
        );
        assert_eq!(
            structured_output(ProviderKind::OpenAiResponses)["text"]["format"]["name"],
            "guard_decision"
        );
        assert!(
            structured_output(ProviderKind::Gemini).is_null(),
            "Gemini's schema key is the client's; the policy asks in the prompt instead"
        );
    }

    /// The policy prompt is the cached prefix, so it must actually say what the parser expects.
    #[test]
    fn the_policy_describes_the_answer_the_parser_reads() {
        assert!(POLICY.contains("\"decision\":\"allow\"|\"deny\""));
        for flag in [
            "irreversible",
            "outside_task",
            "secret_exposure",
            "loop",
            "suspicious_input",
        ] {
            assert!(POLICY.contains(flag), "the policy never mentions {flag}");
        }
        assert!(POLICY.contains(&gantry_core::judge::MAX_REASON_CHARS.to_string()));
    }
}
