//! The guard's verdict (docs/plan/04 §6).
//!
//! In Guarded Auto a small fast model decides each non-read call instead of the user. This is
//! what it decided, kept with the call in `tool_calls.judge_json` so that "what ran, and who
//! allowed it" stays a single query (04 §11) and Settings → Guard can show the last decisions
//! back to the person whose work they shaped.
//!
//! The type is here rather than in `gantry-agent` because three crates need it: the agent
//! writes it, the store keeps it, and the frontend reads it through `specta`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum JudgeDecision {
    Allow,
    Deny,
}

impl JudgeDecision {
    #[must_use]
    pub fn allows(self) -> bool {
        self == JudgeDecision::Allow
    }
}

/// Why the judge decided as it did, in its own vocabulary. The list is closed: an open one
/// would be a second reason field, and the reason is already free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum JudgeFlag {
    /// Gantry cannot undo it.
    Irreversible,
    /// It is not part of what the user asked for.
    OutsideTask,
    /// It reads, writes or sends a credential.
    SecretExposure,
    /// The same failing action, again.
    Loop,
    /// The arguments look like they came from something the model read rather than from the
    /// task: the untrusted-content rule of 10, seen from the outside.
    SuspiciousInput,
}

impl JudgeFlag {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            JudgeFlag::Irreversible => "irreversible",
            JudgeFlag::OutsideTask => "outside the task",
            JudgeFlag::SecretExposure => "touches a secret",
            JudgeFlag::Loop => "repeating a failing action",
            JudgeFlag::SuspiciousInput => "suspicious input",
        }
    }
}

/// Who reached the verdict. The judge's own name covers the rules that run in front of it,
/// because to the user they are all "the guard": one setting turns the lot on and off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum JudgeSource {
    /// The model decided.
    Model,
    /// The loop detector decided, before the model was asked (04 §6, rule 4).
    Loop,
    /// The model could not be asked, or did not answer in a shape we could read. Fail closed:
    /// the user is asked instead (04 §1).
    Unavailable,
}

/// One decision, as the activity row, the audit and the Guard page read it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct JudgeVerdict {
    pub decision: JudgeDecision,
    /// `0.0` to `1.0`, as the model reported it; `1.0` for a rule that did not need a model.
    pub confidence: f32,
    /// At most [`MAX_REASON_CHARS`], written for the person who reads it on the row.
    pub reason: String,
    pub flags: Vec<JudgeFlag>,
    pub source: JudgeSource,
    /// Which model decided, empty when none was asked.
    pub model: String,
    #[specta(type = specta_typescript::Number)]
    pub latency_ms: u64,
    /// The user pressed **Allow anyway** on the block (04 §6). Kept with the verdict rather
    /// than beside it: an override is a fact about this decision.
    #[serde(default)]
    pub overridden: bool,
    /// The user marked the decision wrong, for later prompt tuning (04 §6). `None` means they
    /// said nothing, which is not the same as saying it was right.
    #[serde(default)]
    pub wrong: Option<bool>,
}

/// What the reason is cut to, in characters. The judge is told the same number.
pub const MAX_REASON_CHARS: usize = 200;

impl JudgeVerdict {
    /// A verdict no model was asked for: a rule in front of the judge decided.
    #[must_use]
    pub fn from_rule(
        decision: JudgeDecision,
        source: JudgeSource,
        reason: impl Into<String>,
        flags: Vec<JudgeFlag>,
    ) -> Self {
        Self {
            decision,
            confidence: 1.0,
            reason: cut(&reason.into()),
            flags,
            source,
            model: String::new(),
            latency_ms: 0,
            overridden: false,
            wrong: None,
        }
    }

    #[must_use]
    pub fn allows(&self) -> bool {
        self.decision.allows()
    }

    /// What the model is told when the guard refuses (04 §6).
    #[must_use]
    pub fn blocked_result(&self) -> serde_json::Value {
        serde_json::json!({
            "error": "blocked_by_guard",
            "reason": self.reason,
            "hint": "Ask the user or choose a safer approach.",
        })
    }
}

/// Cuts a reason to [`MAX_REASON_CHARS`] on a character boundary.
#[must_use]
pub fn cut(reason: &str) -> String {
    let reason = reason.trim();
    if reason.chars().count() <= MAX_REASON_CHARS {
        return reason.to_owned();
    }
    let mut out: String = reason.chars().take(MAX_REASON_CHARS - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_reason_is_cut_on_a_character_boundary() {
        let long = "æ".repeat(400);
        let shortened = cut(&long);
        assert_eq!(shortened.chars().count(), MAX_REASON_CHARS);
        assert!(shortened.ends_with('…'));
        assert_eq!(cut("  short  "), "short");
    }

    #[test]
    fn a_block_tells_the_model_what_happened_and_what_to_do() {
        let v = JudgeVerdict::from_rule(
            JudgeDecision::Deny,
            JudgeSource::Loop,
            "The same command has failed three times.",
            vec![JudgeFlag::Loop],
        );
        assert!(!v.allows());
        let json = v.blocked_result();
        assert_eq!(json["error"], "blocked_by_guard");
        assert_eq!(json["reason"], "The same command has failed three times.");
        assert!(json["hint"].as_str().is_some_and(|h| h.contains("safer")));
    }
}
