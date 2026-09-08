//! Running a command: no window, streamed output, capped capture, a deadline, and a kill that
//! takes the whole tree with it (`docs/connectors/shell.md` §3–§5).

use std::{
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use gantry_connectors::{OutputStream, ToolEventSink};
use gantry_core::CallId;
use tokio::{io::AsyncReadExt, process::Command};
use tokio_util::sync::CancellationToken;

use crate::env::ShellEnv;

/// Captured per stream before capture stops (shell.md §4).
pub const CAPTURE_CAP: usize = 2 * 1024 * 1024;
/// Returned to the model per stream; the middle of a build log is what it needs least.
pub const MODEL_CAP: usize = 16 * 1024;
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;
pub const MAX_TIMEOUT_MS: u64 = 600_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    Exited(i32),
    /// Ran past its deadline and was killed with its children.
    TimedOut,
    /// The turn was cancelled, or `kill_command` was called.
    Killed,
}

#[derive(Debug, Clone)]
pub struct Output {
    pub ended: Ended,
    pub stdout: Captured,
    pub stderr: Captured,
    pub duration: Duration,
}

/// One stream's output, with what was dropped counted rather than silently lost (D6).
#[derive(Debug, Clone, Default)]
pub struct Captured {
    pub text: String,
    /// Bytes the command produced past [`CAPTURE_CAP`].
    pub dropped: usize,
}

impl Captured {
    /// What the model is given: the head and the tail, with the elided middle counted.
    #[must_use]
    pub fn for_model(&self) -> String {
        let clean = strip_ansi(&self.text);
        let mut out = elide(&clean, MODEL_CAP);
        if self.dropped > 0 {
            out.push_str(&format!(
                "\n[{} further bytes were produced and not captured]",
                self.dropped
            ));
        }
        out
    }

    #[must_use]
    pub fn truncated(&self) -> bool {
        self.dropped > 0 || self.text.len() > MODEL_CAP
    }
}

/// What to run, and where.
pub struct Job<'a> {
    pub env: &'a ShellEnv,
    pub command: &'a str,
    pub cwd: &'a std::path::Path,
    pub extra_env: &'a [(String, String)],
    pub timeout: Duration,
}

/// Run one command line to completion, or to its deadline, or until it is cancelled.
pub async fn run(
    job: Job<'_>,
    call_id: &CallId,
    sink: Arc<dyn ToolEventSink>,
    cancel: CancellationToken,
    kill: CancellationToken,
) -> std::io::Result<Output> {
    let Job {
        env,
        command,
        cwd,
        extra_env,
        timeout,
    } = job;
    let started = Instant::now();
    let mut cmd = Command::new(&env.program);
    cmd.args(&env.args)
        .arg(command)
        .current_dir(cwd)
        .env_clear()
        .envs(env.vars.iter())
        .envs(extra_env.iter().cloned())
        // Standard input is closed rather than inherited (D3): a command that asks a question
        // gets end-of-file and exits, instead of hanging until the deadline.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_process(&mut cmd);

    let mut child = cmd.spawn()?;
    let pid = child.id();
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();

    let out_task = pump(
        stdout.take(),
        OutputStream::Stdout,
        call_id.clone(),
        Arc::clone(&sink),
    );
    let err_task = pump(
        stderr.take(),
        OutputStream::Stderr,
        call_id.clone(),
        Arc::clone(&sink),
    );

    let ended = tokio::select! {
        status = child.wait() => match status {
            Ok(status) => Ended::Exited(status.code().unwrap_or(-1)),
            Err(err) => return Err(err),
        },
        () = tokio::time::sleep(timeout) => {
            terminate(&mut child, pid).await;
            Ended::TimedOut
        }
        () = cancel.cancelled() => {
            terminate(&mut child, pid).await;
            Ended::Killed
        }
        () = kill.cancelled() => {
            terminate(&mut child, pid).await;
            Ended::Killed
        }
    };

    Ok(Output {
        ended,
        stdout: out_task.await.unwrap_or_default(),
        stderr: err_task.await.unwrap_or_default(),
        duration: started.elapsed(),
    })
}

/// Read one stream to its end, forwarding every chunk to the feed and capturing up to the cap.
fn pump<R>(
    reader: Option<R>,
    stream: OutputStream,
    call_id: CallId,
    sink: Arc<dyn ToolEventSink>,
) -> tokio::task::JoinHandle<Captured>
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut captured = Captured::default();
        let Some(mut reader) = reader else {
            return captured;
        };
        let mut buf = [0_u8; 8192];
        // Bytes at the end of a read that are the start of a character whose remaining bytes are
        // in the next read. Converting each read on its own would turn every such character into
        // two replacement marks — in the captured text and in what the feed shows.
        let mut partial: Vec<u8> = Vec::new();
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut bytes = std::mem::take(&mut partial);
                    bytes.extend_from_slice(&buf[..n]);
                    let good = complete_prefix(&bytes);
                    partial = bytes[good..].to_vec();
                    let chunk = &bytes[..good];
                    let n = chunk.len();
                    if n == 0 {
                        continue;
                    }
                    // The feed sees everything as it arrives, including the part past the cap:
                    // the live window is bounded by the interface, not by the capture.
                    sink.output(&call_id, stream, chunk);
                    let room = CAPTURE_CAP.saturating_sub(captured.text.len());
                    if room == 0 {
                        captured.dropped += n;
                    } else if n <= room {
                        captured.text.push_str(&String::from_utf8_lossy(chunk));
                    } else {
                        captured
                            .text
                            .push_str(&String::from_utf8_lossy(&chunk[..room]));
                        captured.dropped += n - room;
                    }
                }
            }
        }
        captured
    })
}

/// No window, and a process group that can be signalled as a unit (D1, D4).
#[cfg(unix)]
fn configure_process(cmd: &mut Command) {
    // Its own process group, so a kill reaches the children the shell started rather than the
    // shell alone. This is what stops a killed test run from holding its port.
    cmd.process_group(0);
}

#[cfg(windows)]
fn configure_process(cmd: &mut Command) {
    // CREATE_NO_WINDOW is the whole of D1 on Windows: without it, a graphical application
    // spawning a console program flashes a black rectangle on every call. CREATE_NEW_PROCESS_GROUP
    // gives the tree an identity to kill. The process is deliberately *not* detached, because
    // detaching severs the pipes that carry its output.
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

/// Kill the whole tree, then reap the child so it does not linger as a zombie.
async fn terminate(child: &mut tokio::process::Child, pid: Option<u32>) {
    kill_tree(pid).await;
    let _ = child.start_kill();
    let _ = child.wait().await;
}

#[cfg(unix)]
async fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    let Ok(pid) = i32::try_from(pid) else { return };
    // The negative pid is the process *group*, which is why the child was put in its own.
    let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid), nix::sys::signal::SIGKILL);
}

#[cfg(windows)]
async fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    // `taskkill /T` walks the tree the way a job object would, without this crate needing
    // unsafe code or a Windows API dependency for one call.
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .status()
        .await;
}

/// How many bytes of `bytes` end on a character boundary. A read can stop in the middle of a
/// multi-byte character; those trailing bytes wait for the rest rather than becoming `U+FFFD`.
/// Bytes that are not the start of a valid character at all are kept in the prefix, so genuinely
/// invalid output still comes through as replacement marks instead of being held for ever.
fn complete_prefix(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Ok(_) => bytes.len(),
        Err(err) => match err.error_len() {
            // Invalid, not incomplete: let it through and be replaced.
            Some(_) => bytes.len(),
            None => err.valid_up_to(),
        },
    }
}

/// Keep the head and the tail, and say how much of the middle was dropped (D6).
#[must_use]
pub fn elide(text: &str, budget: usize) -> String {
    if text.len() <= budget {
        return text.to_owned();
    }
    let head_budget = budget * 3 / 5;
    let tail_budget = budget - head_budget;
    let head_end = floor_boundary(text, head_budget);
    let tail_start = ceil_boundary(text, text.len() - tail_budget);
    let dropped = tail_start - head_end;
    format!(
        "{}\n[… {dropped} bytes elided …]\n{}",
        &text[..head_end],
        &text[tail_start..]
    )
}

fn floor_boundary(text: &str, mut i: usize) -> usize {
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(text: &str, mut i: usize) -> usize {
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Terminal escape sequences are kept in what is stored, so the drawer can render colour, and
/// stripped from what the model reads, where they are noise.
#[must_use]
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: ends at the first byte in @–~.
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: ends at BEL or ST.
            Some(']') => {
                chars.next();
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {
                chars.next();
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elision_keeps_both_ends_and_counts_the_middle() {
        let text = "a".repeat(100) + &"b".repeat(100);
        let out = elide(&text, 50);
        assert!(out.starts_with("aaa"), "the head survives");
        assert!(out.ends_with("bbb"), "the tail survives");
        assert!(out.contains("bytes elided"), "the loss is stated");
        assert!(out.len() < text.len());
        assert_eq!(elide("short", 50), "short", "nothing to elide");
    }

    #[test]
    fn elision_does_not_split_a_character() {
        let text = "é".repeat(200);
        let out = elide(&text, 51);
        assert!(out.contains("elided"));
    }

    #[test]
    fn escape_sequences_are_stripped_for_the_model() {
        assert_eq!(strip_ansi("\u{1b}[31merror\u{1b}[0m: bad"), "error: bad");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}plain"), "plain");
        assert_eq!(strip_ansi("no escapes"), "no escapes");
    }

    #[test]
    fn a_character_split_across_two_reads_survives() {
        // "é" is two bytes. A read that ends between them must not produce a replacement mark.
        let whole = "café".as_bytes();
        let split = whole.len() - 1;
        assert_eq!(
            complete_prefix(&whole[..split]),
            split - 1,
            "the é waits for its second byte"
        );
        assert_eq!(
            complete_prefix(whole),
            whole.len(),
            "a complete string is complete"
        );
        // Bytes that can never start a character are not held back for ever.
        assert_eq!(complete_prefix(&[0xff, 0xfe]), 2);
    }

    #[test]
    fn capture_reports_what_it_dropped() {
        let captured = Captured {
            text: "kept".to_owned(),
            dropped: 12,
        };
        assert!(captured.truncated());
        assert!(captured.for_model().contains("12 further bytes"));
    }
}
