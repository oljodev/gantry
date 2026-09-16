//! What a response actually is, when a page fights back (`docs/connectors/web.md` §7.5).
//!
//! Handing a model a cheerful HTTP 200 containing "checking your browser" is the worst outcome
//! this connector has: it will summarise the challenge page as though it were the article, and
//! everything downstream of that is confidently wrong. Until now that is exactly what happened —
//! anything in the 200s went through the reader and out to the model as prose.
//!
//! The ladder, in order, and it is mostly free:
//!
//! 1. A **challenge header**. Definitive, and never retried: a challenge is permanent for a
//!    client that does not run JavaScript, and asking again hardens the host's score against the
//!    user rather than against Gantry.
//! 2. A **refusal status** without that header — forbidden, unauthorized, unavailable for legal
//!    reasons. A paywall or an outright no.
//! 3. A **rate-limit status**, or unavailable with a retry hint. Retryable once, after the hint.
//! 4. **Success with almost no text**, plus a corroborating signal in the body. Only here does
//!    body content count at all.
//! 5. Otherwise, the page.
//!
//! Step 4 is the only one that reads the body, and that is the point. Measured (§7.5), grepping
//! for challenge strings wrongly flagged four sites that had returned entirely usable content,
//! because those strings appear in ordinary pages served through the same edge network. So the
//! header decides, and the body only corroborates an absence of text that is already suspicious.

use std::time::Duration;

/// Response headers a challenge is announced in, lowercased.
///
/// Cloudflare's `cf-mitigated` is the documented one: it carries `challenge` on exactly the
/// responses that are a challenge rather than a page. The others are the same idea from vendors
/// that also set a header rather than only a body. A header not in this list is not a challenge,
/// which is deliberate — this list growing is how a new vendor is handled, and body sniffing is
/// not (§7.5).
const CHALLENGE_HEADERS: &[&str] = &[
    "cf-mitigated",
    "x-datadome",
    "x-sucuri-block",
    "x-akamai-bot-manager",
];

/// How long a retry hint may ask for before it stops being a hint and starts being a refusal.
/// A person is waiting on this tool call.
pub const MAX_RETRY_AFTER: Duration = Duration::from_secs(20);

/// What the response turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Step 5: a page, as far as the headers are concerned.
    Page,
    /// Step 1: a bot challenge. Never retried.
    Challenge { named_by: String },
    /// Step 2: the host said no.
    Refused { status: u16 },
    /// Step 3: too fast, or temporarily gone, with a hint. Retryable once.
    Wait { status: u16, after: Duration },
    /// Step 3 without a usable hint: the status says retryable, nothing says when.
    Busy { status: u16 },
}

impl Verdict {
    /// Whether one retry, after waiting, is the right answer (§7.3: never for a challenge).
    #[must_use]
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Wait { after, .. } => Some(*after),
            _ => None,
        }
    }

    /// Whether this is a response to read rather than to explain.
    #[must_use]
    pub fn is_page(&self) -> bool {
        matches!(self, Self::Page)
    }
}

/// Steps 1 to 3: everything that can be decided from the status line and the headers.
pub fn from_response(status: u16, header: impl Fn(&str) -> Option<String>) -> Verdict {
    for name in CHALLENGE_HEADERS {
        if let Some(value) = header(name)
            && !value.trim().is_empty()
        {
            // Cloudflare sets `cf-mitigated` on more than challenges; only the challenge value
            // means the body is a challenge rather than the page.
            if *name == "cf-mitigated" && !value.to_lowercase().contains("challenge") {
                continue;
            }
            return Verdict::Challenge {
                named_by: (*name).to_owned(),
            };
        }
    }
    match status {
        401 | 403 | 451 => Verdict::Refused { status },
        429 | 503 => match header("retry-after").as_deref().and_then(retry_after) {
            Some(after) => Verdict::Wait { status, after },
            None => Verdict::Busy { status },
        },
        _ => Verdict::Page,
    }
}

/// `Retry-After` is either a number of seconds or an HTTP date. Both are honoured; a date is
/// read as "how far away is that", because that is the only part of it this needs.
///
/// A hint longer than [`MAX_RETRY_AFTER`] is not waited out — it is a refusal wearing a number,
/// and the tool call belongs to somebody sitting in front of the screen.
#[must_use]
pub fn retry_after(value: &str) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        let wanted = Duration::from_secs(seconds);
        return (wanted <= MAX_RETRY_AFTER).then_some(wanted);
    }
    let when = httpdate(value)?;
    let now = std::time::SystemTime::now();
    let wanted = when.duration_since(now).ok()?;
    (wanted <= MAX_RETRY_AFTER).then_some(wanted)
}

/// An IMF-fixdate — `Sun, 06 Nov 1994 08:49:37 GMT` — as a system time. Only that one form: it
/// is the only one a server is allowed to send, and the two obsolete formats are not worth a
/// date-parsing dependency for a header that is usually an integer anyway.
fn httpdate(value: &str) -> Option<std::time::SystemTime> {
    let mut parts = value.split_whitespace();
    let _weekday = parts.next()?;
    let day: u64 = parts.next()?.parse().ok()?;
    let month = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: u64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: u64 = clock.next()?.parse().ok()?;
    let minute: u64 = clock.next()?.parse().ok()?;
    let second: u64 = clock.next()?.parse().ok()?;
    if year < 1970 {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    Some(std::time::UNIX_EPOCH + Duration::from_secs(seconds))
}

/// Days since 1970-01-01, by the civil-calendar algorithm rather than by a table.
fn days_from_civil(year: u64, month: u64, day: u64) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// Step 4: a success with almost nothing in it, corroborated by the body.
///
/// The threshold is low on purpose. A page with a paragraph in it is a page; this is about the
/// ones that extracted to nothing, where the question is only whether to say "this site needs a
/// browser" or "this page is empty" — and a model told the first will stop rather than assert
/// that the subject does not exist.
pub const EMPTY_ENOUGH: usize = 200;

/// Whether an all-but-empty success is a page that needs JavaScript rather than a short page.
#[must_use]
pub fn needs_a_browser(extracted: &str, html: &str) -> bool {
    if extracted.trim().chars().count() > EMPTY_ENOUGH {
        return false;
    }
    let lower = html.to_lowercase();
    // A `<noscript>` telling the user to turn JavaScript on, an empty mount point, or a
    // framework's client-side payload: three different ways of saying the same thing.
    let noscript = lower.contains("<noscript")
        && (lower.contains("enable javascript")
            || lower.contains("javascript is required")
            || lower.contains("turn on javascript")
            || lower.contains("requires javascript"));
    let empty_root = ["<div id=\"root\"></div>", "<div id=\"app\"></div>"]
        .iter()
        .any(|m| lower.contains(m));
    let payload = lower.contains("__next_data__")
        || lower.contains("window.__nuxt__")
        || lower.contains("__sveltekit_");
    noscript || empty_root || payload
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name: &str| {
            owned
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn a_challenge_header_beats_a_cheerful_status() {
        let v = from_response(200, headers(&[("cf-mitigated", "challenge")]));
        assert_eq!(
            v,
            Verdict::Challenge {
                named_by: "cf-mitigated".into()
            }
        );
        assert!(v.retry_after().is_none(), "a challenge is never retried");
        assert!(!v.is_page());
    }

    #[test]
    fn cloudflare_mitigating_something_else_is_not_a_challenge() {
        // `cf-mitigated` appears on responses that are not challenges; only the value decides.
        assert_eq!(
            from_response(200, headers(&[("cf-mitigated", "rate_limit")])),
            Verdict::Page
        );
    }

    #[test]
    fn the_refusal_statuses_are_refusals_and_the_rest_are_pages() {
        for status in [401, 403, 451] {
            assert_eq!(
                from_response(status, headers(&[])),
                Verdict::Refused { status }
            );
        }
        for status in [200, 301, 404, 500] {
            assert_eq!(from_response(status, headers(&[])), Verdict::Page);
        }
    }

    #[test]
    fn a_rate_limit_carries_its_hint_and_a_bare_one_does_not() {
        assert_eq!(
            from_response(429, headers(&[("retry-after", "5")])),
            Verdict::Wait {
                status: 429,
                after: Duration::from_secs(5)
            }
        );
        assert_eq!(
            from_response(503, headers(&[])),
            Verdict::Busy { status: 503 }
        );
        // A hint nobody would wait out is a refusal wearing a number.
        assert_eq!(
            from_response(429, headers(&[("retry-after", "3600")])),
            Verdict::Busy { status: 429 }
        );
    }

    #[test]
    fn a_retry_after_date_is_read_as_how_long_from_now() {
        let soon = std::time::SystemTime::now() + Duration::from_secs(8);
        let formatted = imf(soon);
        let parsed = retry_after(&formatted).expect("a date in the near future");
        assert!(
            parsed <= Duration::from_secs(9) && parsed >= Duration::from_secs(5),
            "{parsed:?} from {formatted}"
        );
        // A date in the past is not a wait at all.
        assert!(retry_after("Sun, 06 Nov 1994 08:49:37 GMT").is_none());
        assert!(retry_after("not a date").is_none());
    }

    #[test]
    fn a_page_that_needs_a_browser_is_told_apart_from_a_short_one() {
        assert!(needs_a_browser(
            "",
            "<html><body><noscript>Please enable JavaScript to view this site.</noscript></body></html>"
        ));
        assert!(needs_a_browser(
            "",
            "<html><body><div id=\"root\"></div><script src=\"/app.js\"></script></body></html>"
        ));
        assert!(needs_a_browser(
            "Loading…",
            "<html><body><script>window.__NEXT_DATA__ = {}</script></body></html>"
        ));
        // A short page that says nothing about JavaScript is a short page.
        assert!(!needs_a_browser(
            "Not found.",
            "<html><body>Not found.</body></html>"
        ));
        // And a real article is never this, whatever else is in its markup.
        assert!(!needs_a_browser(
            &"words ".repeat(100),
            "<html><body><noscript>enable javascript</noscript></body></html>"
        ));
    }

    /// The one date format a server is allowed to send, built here so the test above does not
    /// need a clock library either.
    fn imf(at: std::time::SystemTime) -> String {
        let secs = at.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let days = secs / 86_400;
        let (h, m, s) = ((secs % 86_400) / 3600, (secs % 3600) / 60, secs % 60);
        // Walk forward from the epoch: a handful of thousand iterations, once, in a test.
        let (mut year, mut left) = (1970u64, days);
        loop {
            let len = if leap(year) { 366 } else { 365 };
            if left < len {
                break;
            }
            left -= len;
            year += 1;
        }
        let lengths = [
            31,
            if leap(year) { 29 } else { 28 },
            31,
            30,
            31,
            30,
            31,
            31,
            30,
            31,
            30,
            31,
        ];
        let mut month = 0;
        while left >= lengths[month] {
            left -= lengths[month];
            month += 1;
        }
        let names = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!(
            "Xxx, {:02} {} {year} {h:02}:{m:02}:{s:02} GMT",
            left + 1,
            names[month]
        )
    }

    fn leap(year: u64) -> bool {
        (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
    }
}
