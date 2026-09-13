//! `gantry__clock`: the current date and time on the user's machine. It observes nothing but
//! the clock, so it is `read` tier on purpose rather than `app`: in Manual mode it asks, like
//! every other read. It was also the first tool that exercised the loop and the permission card
//! (09 M3).
//!
//! Since the prompt carries `<gantry_now>` with today's date, this tool is for the clock and
//! not the calendar, and its description says so: a model that called it to find out what day
//! it was spent a round — and a permission card, in Manual — on something already in front of
//! it. The time is deliberately not in the prompt, because it would invalidate the cached
//! prefix every turn, so the remaining questions are real ones.

use chrono::{Datelike, Local, Utc};
use gantry_connectors::ToolOutcome;
use gantry_core::{RiskTier, ToolDef};
use serde_json::json;

pub const NAME: &str = "clock";

#[must_use]
pub fn definition() -> ToolDef {
    let mut def = ToolDef::new(
        NAME,
        "The current time on the user's computer: local time with its UTC offset, UTC, the \
         weekday, the ISO week and the Unix timestamp. Today's date is already in your prompt \
         under <gantry_now>, so do not call this to find out what day it is — call it when you \
         need the hour, the minute, a timestamp, or the date after the conversation has been \
         open long enough for it to have changed.",
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        }),
        RiskTier::Read,
    );
    def.parallel_safe = true;
    def
}

#[must_use]
pub fn call(_args: &serde_json::Value) -> ToolOutcome {
    let local = Local::now();
    let utc = Utc::now();
    ToolOutcome::json(json!({
        "local": local.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "utc": utc.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "date": local.format("%Y-%m-%d").to_string(),
        "time": local.format("%H:%M").to_string(),
        "weekday": local.weekday().to_string(),
        "utc_offset": local.format("%:z").to_string(),
        "iso_week": local.iso_week().week(),
        "unix_ms": local.timestamp_millis(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_answers_with_a_date_and_a_weekday() {
        let ToolOutcome::Complete {
            structured,
            is_error,
            ..
        } = call(&json!({}));
        assert!(!is_error);
        let v = structured.unwrap();
        assert_eq!(v["date"].as_str().unwrap().len(), 10);
        assert!(v["weekday"].as_str().unwrap().len() >= 3);
        assert!(v["local"].as_str().unwrap().contains('T'));
    }
}
