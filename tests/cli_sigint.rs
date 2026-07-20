//! SIGINT handling for `batch-run`, which needs no AWS access — so unlike
//! the transfer commands (covered in `tests/e2e_ctrl_c.rs`) these run under
//! a plain `cargo test`.
//!
//! Unix-only: SIGINT delivery via the `nix` crate.

#![cfg(unix)]

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const STARTUP_DELAY: Duration = Duration::from_millis(1500);
const WAIT_TIMEOUT: Duration = Duration::from_secs(30);

fn s7cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_s7cmd"))
}

/// Poll for child exit up to `WAIT_TIMEOUT`. Returns `None` if it never
/// exits, so callers can assert "did not hang" without blocking a CI run
/// forever.
fn wait_bounded(child: &mut std::process::Child) -> Option<std::process::ExitStatus> {
    let deadline = Instant::now() + WAIT_TIMEOUT;
    while Instant::now() < deadline {
        match child.try_wait().expect("try_wait failed") {
            Some(status) => return Some(status),
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    None
}

/// Non-streaming `batch-run -` spends phase 1 in a blocking read of stdin
/// under the *default* SIGINT disposition (the handler is installed only
/// after the script has been read), so Ctrl-C there terminates the process
/// by signal — exactly the "I haven't started anything yet" intuition the
/// phase-1 comment describes. `code()` is `None` for a signal death;
/// the signal number is 2 (SIGINT).
#[test]
fn batch_run_sigint_while_reading_stdin_dies_by_signal() {
    use std::os::unix::process::ExitStatusExt;

    let mut child = s7cmd()
        .args(["batch-run", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn s7cmd");

    std::thread::sleep(STARTUP_DELAY);
    let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGINT);

    let status = wait_bounded(&mut child).unwrap_or_else(|| {
        let _ = child.kill();
        panic!("batch-run did not exit within {WAIT_TIMEOUT:?} of SIGINT during phase-1 read");
    });
    assert_eq!(
        status.signal(),
        Some(2),
        "phase-1 SIGINT must terminate by signal; got {status:?}"
    );
}

/// Streaming `batch-run --streaming -` sitting idle on stdin: SIGINT sets
/// the shared interrupt flag and trips the reader's `ctrl_c` select arm, so
/// once stdin also reaches EOF the run finishes cleanly with exit 0 (no
/// line ever executed, so nothing is "failed" or "skipped").
///
/// The stdin close after the signal is load-bearing, not incidental:
/// streaming mode reads via `tokio::io::stdin()`, which tokio implements as
/// an uncancellable blocking read on a helper thread. Tokio documents that
/// this "can make shutdown of the runtime hang until the user presses
/// enter" — so a `--streaming -` run whose stdin pipe never closes does NOT
/// exit on SIGINT alone. This test therefore pins the reachable contract
/// (signal + EOF → prompt, clean exit); the pipe-stays-open hang is a
/// separate defect in the product, not something to assert as correct here.
#[test]
fn batch_run_streaming_sigint_then_eof_exits_0() {
    let mut child = s7cmd()
        .args(["batch-run", "--streaming", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn s7cmd");

    std::thread::sleep(STARTUP_DELAY);
    let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGINT);

    // Let the signal be observed, then release the blocking stdin read.
    std::thread::sleep(Duration::from_millis(300));
    drop(child.stdin.take());

    let status = wait_bounded(&mut child).unwrap_or_else(|| {
        let _ = child.kill();
        panic!("streaming batch-run did not exit within {WAIT_TIMEOUT:?} of SIGINT + EOF");
    });
    assert_eq!(
        status.code(),
        Some(0),
        "interrupted idle streaming batch must exit 0; got {status:?}"
    );
}

/// The same streaming path with real work queued: lines are written before
/// the signal, stdin is closed after it, and the run still exits cleanly.
/// Every line is a `--dry-run` so no S3 endpoint is contacted.
#[test]
fn batch_run_streaming_sigint_with_queued_lines_exits_cleanly() {
    let mut child = s7cmd()
        .args(["batch-run", "--streaming", "--disable-color-tracing", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn s7cmd");

    {
        let stdin = child.stdin.as_mut().expect("stdin piped");
        for i in 0..3 {
            writeln!(stdin, "create-bucket --dry-run s3://sigint-stream-{i}").expect("write line");
        }
        stdin.flush().expect("flush");
    }

    std::thread::sleep(STARTUP_DELAY);
    let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGINT);
    std::thread::sleep(Duration::from_millis(300));
    drop(child.stdin.take());

    let status = wait_bounded(&mut child).unwrap_or_else(|| {
        let _ = child.kill();
        panic!("streaming batch-run did not exit within {WAIT_TIMEOUT:?} of SIGINT + EOF");
    });
    assert_eq!(
        status.code(),
        Some(0),
        "dry-run lines all succeed, so an interrupt after them exits 0; got {status:?}"
    );
}
