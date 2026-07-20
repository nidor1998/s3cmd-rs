//! Every tracing family (sync, clean, ls, util) routes its output through a
//! `PipeSafeWriter` that swallows `BrokenPipe` so a closed stderr (e.g.
//! `s7cmd ... 2>&1 | head`) cannot panic the process. These tests pin that
//! contract end to end: each family runs an offline-failing command twice —
//! once normally and once with the parent's read end of the stderr pipe
//! closed immediately after spawn — and must exit with the same code both
//! times. A panic in the writer would surface as a different exit
//! (101/abort) and fail the assertion.
//!
//! The commands point at a loopback endpoint with nothing listening, so
//! the error output (the writes that must survive EPIPE) is produced
//! without AWS access. Closing the pipe races the child's first write; the
//! child needs tens of milliseconds of startup before it first logs, so
//! the close wins in practice — and even when it doesn't, the assertion
//! still holds (the run just degrades to the open-stderr case).

mod common;

use common::s7cmd_cmd_clean_env;
use std::process::{Command, Stdio};

const DEAD_ENDPOINT: &str = "http://127.0.0.1:9";

fn with_fake_aws_env(cmd: &mut Command) -> &mut Command {
    cmd.env("AWS_ACCESS_KEY_ID", "test")
        .env("AWS_SECRET_ACCESS_KEY", "test")
        .env("AWS_REGION", "us-east-1")
}

/// Spawn with stderr piped, close the read end immediately, and return the
/// exit code. Stdout is discarded; stdin is closed.
fn exit_code_with_closed_stderr(cmd: &mut Command) -> Option<i32> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn s7cmd");
    drop(child.stderr.take());
    child.wait().expect("child.wait() failed").code()
}

fn assert_same_exit_with_closed_stderr(args: &[&str], expected: i32) {
    let mut open = s7cmd_cmd_clean_env();
    with_fake_aws_env(&mut open).args(args);
    let open_code = open
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("open-stderr run failed")
        .code();
    assert_eq!(
        open_code,
        Some(expected),
        "baseline exit code changed for {args:?}"
    );

    let mut closed = s7cmd_cmd_clean_env();
    with_fake_aws_env(&mut closed).args(args);
    let closed_code = exit_code_with_closed_stderr(&mut closed);
    assert_eq!(
        closed_code,
        Some(expected),
        "closed stderr must not change the exit code (a panic would) for {args:?}"
    );
}

/// sync is the odd one out: its tracing sink is stdout, so the closed
/// stream must be stdout for the EPIPE-swallowing writer to be exercised.
#[test]
fn sync_with_closed_stdout_does_not_panic() {
    let args = [
        "sync",
        "s3://src-bucket/",
        "s3://dst-bucket/",
        "--source-endpoint-url",
        DEAD_ENDPOINT,
        "--target-endpoint-url",
        DEAD_ENDPOINT,
        "--aws-max-attempts",
        "1",
    ];

    let mut open = s7cmd_cmd_clean_env();
    with_fake_aws_env(&mut open).args(args);
    let open_code = open
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("open-stdout run failed")
        .code();
    assert_eq!(open_code, Some(1), "baseline sync exit code changed");

    let mut closed = s7cmd_cmd_clean_env();
    with_fake_aws_env(&mut closed).args(args);
    let mut child = closed
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn s7cmd");
    drop(child.stdout.take());
    let closed_code = child.wait().expect("child.wait() failed").code();
    assert_eq!(
        closed_code,
        Some(1),
        "closed stdout must not change sync's exit code (a panic would)"
    );
}

#[test]
fn clean_with_closed_stderr_does_not_panic() {
    assert_same_exit_with_closed_stderr(
        &[
            "clean",
            "--force",
            "s3://some-bucket/prefix/",
            "--target-endpoint-url",
            DEAD_ENDPOINT,
            "--aws-max-attempts",
            "1",
        ],
        1,
    );
}

#[test]
fn ls_with_closed_stderr_does_not_panic() {
    assert_same_exit_with_closed_stderr(
        &[
            "ls",
            "s3://some-bucket",
            "--target-endpoint-url",
            DEAD_ENDPOINT,
            "--aws-max-attempts",
            "1",
        ],
        1,
    );
}

#[test]
fn util_command_with_closed_stderr_does_not_panic() {
    assert_same_exit_with_closed_stderr(
        &[
            "get-bucket-versioning",
            "s3://some-bucket",
            "--target-endpoint-url",
            DEAD_ENDPOINT,
            "--aws-max-attempts",
            "1",
        ],
        1,
    );
}
