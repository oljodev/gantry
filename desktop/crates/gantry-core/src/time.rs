//! Wall-clock milliseconds since the Unix epoch, the one timestamp representation used in
//! events, DTOs and the database.

use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch; never fails (a clock before 1970 reads as 0).
#[must_use]
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
