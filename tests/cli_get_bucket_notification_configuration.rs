//! Process-level CLI tests for the `get-bucket-notification-configuration` subcommand.
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
    let (ok, stdout, _stderr, _code) =
        run(s7cmd().args(["get-bucket-notification-configuration", "--help"]));
    assert!(
        ok,
        "get-bucket-notification-configuration --help must succeed"
    );
    assert!(stdout.contains("AWS Configuration"));
    assert!(stdout.contains("Retry Options"));
    assert!(stdout.contains("Timeout Options"));
}

#[test]
fn missing_target_exits_non_zero() {
    let (ok, _stdout, stderr, code) = run(s7cmd().arg("get-bucket-notification-configuration"));
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

// NOTE: s3util-rs's auto_complete_shell_short_circuits_without_target test
// is omitted — s7cmd hides the per-subcommand --auto-complete-shell flag
// and exposes only the top-level form (covered by tests/cli_help.rs).

#[test]
fn bucket_with_key_exits_1() {
    let (ok, _stdout, stderr, code) =
        run(s7cmd().args(["get-bucket-notification-configuration", "s3://example/key"]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(1),
        "bucket path with key should exit 1 (validation)"
    );
    assert!(
        !stderr.is_empty(),
        "should have an error message on stderr; got empty"
    );
}

#[test]
fn target_access_key_without_secret_exits_non_zero() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "get-bucket-notification-configuration",
        "s3://example",
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
            || stderr.to_lowercase().contains("--target-secret-access-key"),
        "expected clap error about missing secret key; got: {stderr}"
    );
}

#[test]
fn target_no_sign_request_conflicts_with_target_profile() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "get-bucket-notification-configuration",
        "s3://example",
        "--target-no-sign-request",
        "--target-profile",
        "default",
    ]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(2),
        "clap conflict should exit 2; stderr: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("cannot be used")
            || stderr.to_lowercase().contains("conflict"),
        "expected clap conflict message; got: {stderr}"
    );
}

#[test]
fn mock_endpoint_configured_notifications_print_json() {
    let server = MockS3Server::start(vec![MockResponse::new(
        200,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <NotificationConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
           <TopicConfiguration>\
             <Id>mock-notification</Id>\
             <Topic>arn:aws:sns:us-east-1:123456789012:mock-topic</Topic>\
             <Event>s3:ObjectCreated:*</Event>\
           </TopicConfiguration>\
         </NotificationConfiguration>",
    )]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.arg("get-bucket-notification-configuration")
        .args(mock_target_args(&server.endpoint_url()))
        .arg("s3://mock-bucket");
    let (code, stdout, stderr) = common::run(&mut cmd);
    assert_eq!(code, Some(0), "expected success; stderr: {stderr}");
    assert!(
        stdout.contains("arn:aws:sns:us-east-1:123456789012:mock-topic"),
        "stdout should contain the configured topic ARN; got: {stdout}"
    );
}
