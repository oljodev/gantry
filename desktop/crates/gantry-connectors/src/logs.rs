//! What a local server said on its way down (docs/plan/03 §6, §11 step 4).
//!
//! A stdio server talks MCP on stdout, so stdout is unreadable by a person — everything it wants
//! to *tell* anybody goes to stderr, and that is where the reason a spawn failed lives. Without
//! this the install's failure is "the process exited", which names no cause and suggests no fix,
//! while the line that says `Cannot find module 'x'` or `ENOTFOUND` was written and thrown away.
//!
//! Kept in memory, bounded, per instance. Not a file: these lines are diagnostics for the minutes
//! after a failure, not a record worth keeping across restarts, and a log file that grows in a
//! directory nobody sweeps is a second problem in exchange for solving half of the first.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use gantry_core::InstanceId;

/// Lines kept per instance. Enough for a stack trace and the noise around it; a server that
/// writes more than this before dying has said the useful part already.
const MAX_LINES: usize = 400;
/// A single line longer than this is truncated: some servers print an entire bundle on one line.
const MAX_LINE: usize = 4_000;

/// The stderr of every running local server, newest last.
#[derive(Debug, Default, Clone)]
pub struct ConnectorLogs {
    inner: Arc<Mutex<HashMap<InstanceId, VecDeque<String>>>>,
}

impl ConnectorLogs {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, id: InstanceId, line: &str) {
        let line = line.trim_end();
        let line = if line.len() > MAX_LINE {
            format!("{}…", &line[..MAX_LINE])
        } else {
            line.to_owned()
        };
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let lines = guard.entry(id).or_default();
        if lines.len() == MAX_LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    /// Everything kept for one instance, oldest first.
    #[must_use]
    pub fn lines(&self, id: InstanceId) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .map(|lines| lines.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The last `n` lines — what a failure message shows without a click.
    #[must_use]
    pub fn tail(&self, id: InstanceId, n: usize) -> Vec<String> {
        let lines = self.lines(id);
        lines[lines.len().saturating_sub(n)..].to_vec()
    }

    /// Dropped when an instance is removed, and when it is restarted: the previous run's stderr
    /// explaining a failure that has since been fixed is worse than no stderr at all.
    pub fn clear(&self, id: InstanceId) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }

    /// Reads a child's stderr into the buffer until the pipe closes, which is when the process
    /// ends. Spawned as its own task: a server that never writes to stderr would otherwise block
    /// the connection, and one that writes constantly would block on a full pipe if nobody read.
    pub fn drain<R>(&self, id: InstanceId, stderr: R)
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
    {
        let logs = self.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                logs.push(id, &line);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oldest_lines_go_first_and_the_newest_are_the_tail() {
        let logs = ConnectorLogs::new();
        let id = InstanceId::new();
        for i in 0..(MAX_LINES + 10) {
            logs.push(id, &format!("line {i}"));
        }
        let lines = logs.lines(id);
        assert_eq!(lines.len(), MAX_LINES);
        assert_eq!(lines[0], format!("line {}", 10));
        assert_eq!(
            logs.tail(id, 3),
            vec![
                format!("line {}", MAX_LINES + 7),
                format!("line {}", MAX_LINES + 8),
                format!("line {}", MAX_LINES + 9),
            ]
        );
    }

    #[test]
    fn a_tail_longer_than_the_log_is_the_log() {
        let logs = ConnectorLogs::new();
        let id = InstanceId::new();
        assert!(logs.tail(id, 20).is_empty());
        logs.push(id, "only one");
        assert_eq!(logs.tail(id, 20), vec!["only one".to_owned()]);
    }

    /// The previous run's explanation of a failure that has since been fixed is worse than
    /// nothing: it is a wrong answer that looks like a right one.
    #[test]
    fn a_restart_starts_from_nothing() {
        let logs = ConnectorLogs::new();
        let id = InstanceId::new();
        logs.push(id, "Error: cannot find module 'left-pad'");
        logs.clear(id);
        assert!(logs.lines(id).is_empty());
    }

    #[tokio::test]
    async fn a_pipe_is_read_to_its_end() {
        let logs = ConnectorLogs::new();
        let id = InstanceId::new();
        logs.drain(id, &b"first\nsecond\n"[..]);
        for _ in 0..50 {
            if logs.lines(id).len() == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            logs.lines(id),
            vec!["first".to_owned(), "second".to_owned()]
        );
    }
}
