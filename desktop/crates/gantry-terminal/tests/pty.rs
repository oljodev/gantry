//! A real shell on a real pty. The unit tests cover the decoding; this covers the part that
//! only a process can prove — that something typed reaches the shell and its answer comes back.

#![cfg(unix)]

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use gantry_terminal::{Open, TerminalSink, Terminals};

#[derive(Default)]
struct Collector {
    text: Mutex<String>,
    exited: Mutex<Option<Option<i32>>>,
}

impl TerminalSink for Collector {
    fn output(&self, _id: &str, data: &str) {
        self.text
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_str(data);
    }
    fn exited(&self, _id: &str, code: Option<i32>) {
        *self.exited.lock().unwrap_or_else(|e| e.into_inner()) = Some(code);
    }
}

impl Collector {
    /// Waits until `needle` has arrived `times` times, or the deadline passes.
    ///
    /// The count is the whole point. A shell echoes what was typed the instant it is typed and
    /// answers it a moment later, so waiting for one occurrence returns on the echo and a test
    /// that asserts on the answer reads the buffer before it is there.
    fn wait_for(&self, needle: &str, times: usize) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let text = self.text.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if text.matches(needle).count() >= times || Instant::now() > deadline {
                return text;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn a_command_typed_into_a_terminal_answers() {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(Collector::default());
    let terminals = Terminals::new();

    let opened = terminals
        .open(
            "t1",
            &Open {
                cwd: Some(dir.path().to_path_buf()),
                cols: 80,
                rows: 24,
            },
            Arc::clone(&sink) as Arc<dyn TerminalSink>,
        )
        .expect("a shell starts");
    assert!(!opened.shell.is_empty(), "the tab can name the shell");

    // `\r` is Enter: the front end sends keystrokes, not lines.
    terminals.write("t1", "echo gantry-terminal-ok\r").unwrap();
    let text = sink.wait_for("gantry-terminal-ok", 2);
    assert!(
        text.matches("gantry-terminal-ok").count() >= 2,
        "the shell echoes what was typed and then answers it: {text:?}"
    );

    // The scrollback is what a reopened tab is redrawn from.
    let kept = terminals.scrollback("t1").expect("still open");
    assert!(kept.contains("gantry-terminal-ok"), "{kept:?}");

    terminals.close("t1");
    assert!(!terminals.is_open("t1"));
    assert!(terminals.write("t1", "echo x\r").is_err(), "and it is gone");
}

#[test]
fn a_shell_that_exits_is_reported() {
    let sink = Arc::new(Collector::default());
    let terminals = Terminals::new();
    terminals
        .open(
            "t2",
            &Open {
                cwd: None,
                cols: 40,
                rows: 10,
            },
            Arc::clone(&sink) as Arc<dyn TerminalSink>,
        )
        .unwrap();

    terminals.write("t2", "exit 3\r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let code = loop {
        let seen = *sink.exited.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(code) = seen {
            break code;
        }
        assert!(Instant::now() < deadline, "the exit was never reported");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(code, Some(3), "with the status the shell exited on");
    terminals.close("t2");
}
