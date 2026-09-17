//! The clock startup is measured against (docs/dev/performance.md).
//!
//! Nothing in the app knew how long anything took, which is how a five-second login-shell
//! capture can sit on the critical path for months without anybody noticing: it is only ever
//! slow on somebody else's machine. So every phase of `startup::init` is timed, the whole line
//! is logged, and the same numbers are readable from Settings → Advanced.
//!
//! The two clocks are joined by [`timing`]: the frontend's marks are milliseconds since its own
//! `timeOrigin`, which begins when the webview is created — long after the process did — and
//! `since_start_ms` says how far apart the two origins are, read at the moment of the call.

use std::{
    sync::{LazyLock, OnceLock},
    time::Instant,
};

use gantry_core::{StartupPhase, StartupTiming};

/// The earliest instant the process can name. Read on the first line of `run()`, so it covers
/// everything but the dynamic linker — which on Linux means everything but WebKitGTK loading.
static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Filled once, at the end of `setup`.
static STARTUP: OnceLock<StartupTiming> = OnceLock::new();

/// Starts the clock. Call first, before anything else in `run()`.
pub fn start_clock() {
    LazyLock::force(&PROCESS_START);
}

/// Milliseconds since the process started, saturating (a u32 of milliseconds is 49 days).
fn since_start() -> u32 {
    PROCESS_START
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u32::MAX)
}

/// A running stopwatch over the phases of startup.
pub struct Phases {
    last: Instant,
    phases: Vec<StartupPhase>,
}

impl Phases {
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
            phases: Vec::new(),
        }
    }

    /// Closes the phase that has been running and names it.
    pub fn step(&mut self, name: &str) {
        let now = Instant::now();
        let ms = now
            .duration_since(self.last)
            .as_millis()
            .try_into()
            .unwrap_or(u32::MAX);
        self.last = now;
        self.phases.push(StartupPhase {
            name: name.to_owned(),
            ms,
        });
    }

    /// Records the run and logs it as one line. Called whether or not startup succeeded: a
    /// startup that failed halfway is exactly the one whose timings are worth reading.
    pub fn finish(self) {
        let total = since_start();
        let line = self
            .phases
            .iter()
            .map(|p| format!("{} {} ms", p.name, p.ms))
            .collect::<Vec<_>>()
            .join(", ");
        log::info!("startup: {total} ms to the open window ({line})");
        let _ = STARTUP.set(StartupTiming {
            total_ms: total,
            phases: self.phases,
            since_start_ms: total,
        });
    }
}

/// What startup spent, read now. Empty phases until `setup` has finished.
pub fn timing() -> StartupTiming {
    let mut timing = STARTUP.get().cloned().unwrap_or(StartupTiming {
        total_ms: 0,
        phases: Vec::new(),
        since_start_ms: 0,
    });
    timing.since_start_ms = since_start();
    timing
}
