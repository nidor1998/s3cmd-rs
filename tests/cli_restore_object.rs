//! Process-level CLI tests for the `restore-object` subcommand.
//! These run without AWS credentials or network access (mock-endpoint
//! tests talk only to a loopback HTTP server).

mod common;
use common::{MockResponse, MockS3Server, mock_target_args, s7cmd_cmd_clean_env};

use std::process::{Command, Stdio};

fn s7cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_s7cmd"))
}

fn run(cmd: &mut Command) -> (bool, String, String, Option<i32>) {
    let output = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to spawn s7cmd binary");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (
        output.status.success(),
        stdout,
        stderr,
        output.status.code(),
    )
}

#[test]
fn help_succeeds_and_lists_option_groups() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["restore-object", "--help"]));
    assert!(ok, "restore-object --help must succeed");
    assert!(stdout.contains("AWS Configuration"));
    assert!(stdout.contains("Retry Options"));
    assert!(stdout.contains("Timeout Options"));
    assert!(
        stdout.contains("--days") && stdout.contains("--tier"),
        "expected --days and --tier in help; got: {stdout}"
    );
}

#[test]
fn missing_target_exits_non_zero() {
    let (ok, _stdout, stderr, code) = run(s7cmd().arg("restore-object"));
    assert!(!ok);
    assert_eq!(
        code,
        Some(2),
        "clap missing-arg should exit 2; stderr: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("required") || stderr.to_lowercase().contains("usage"),
        "expected 'required' or 'usage' in stderr; got: {stderr}"
    );
}

#[test]
fn bucket_only_path_exits_1() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args(["restore-object", "s3://example"]));
    assert!(!ok);
    assert_eq!(code, Some(1), "bucket-only path should exit 1 (validation)");
    assert!(
        !stderr.is_empty(),
        "should have an error message on stderr; got empty"
    );
}

#[test]
fn invalid_tier_exits_2() {
    let (ok, _stdout, stderr, code) =
        run(s7cmd().args(["restore-object", "s3://example/key", "--tier", "TurboMax"]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(2),
        "invalid tier should exit 2 (clap validation); stderr: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("invalid tier")
            || stderr.to_lowercase().contains("invalid value"),
        "expected invalid-tier message; got: {stderr}"
    );
}

#[test]
fn standard_tier_with_days_parses_ok() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "restore-object",
        "s3://example/key",
        "--days",
        "7",
        "--tier",
        "Standard",
    ]));
    assert!(
        code != Some(2),
        "valid tier+days combo must parse without clap error; code={code:?}; stderr={stderr}"
    );
    let _ = ok;
}

#[test]
fn target_access_key_without_secret_exits_non_zero() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "restore-object",
        "s3://example/key",
        "--target-access-key",
        "AKIA",
    ]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(2),
        "clap missing-arg should exit 2; stderr: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("required")
            || stderr.to_lowercase().contains("--target-secret-access-key")
    );
}

#[test]
fn mock_endpoint_restore_initiated_exits_0() {
    let server = MockS3Server::start(vec![MockResponse::new(202, "")]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["restore-object", "-v", "--days", "1"])
        .args(mock_target_args(&server.endpoint_url()))
        .arg("s3://mock-bucket/deep-archive-object");
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(code, Some(0), "expected success; stderr: {stderr}");
    assert!(
        stderr.contains("Restore initiated."),
        "expected the success info line (-v); got: {stderr}"
    );
}
