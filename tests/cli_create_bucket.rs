//! Process-level CLI tests for the `create-bucket` subcommand.
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
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["create-bucket", "--help"]));
    assert!(ok, "create-bucket --help must succeed");
    assert!(stdout.contains("AWS Configuration"));
    assert!(stdout.contains("Retry Options"));
    assert!(stdout.contains("Timeout Options"));
}

#[test]
fn help_mentions_tagging_option() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["create-bucket", "--help"]));
    assert!(ok);
    assert!(
        stdout.contains("--tagging"),
        "expected --tagging in help output: {stdout}"
    );
}

#[test]
fn help_mentions_if_not_exists_option() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["create-bucket", "--help"]));
    assert!(ok);
    assert!(
        stdout.contains("--if-not-exists"),
        "expected --if-not-exists in help output: {stdout}"
    );
}

#[test]
fn missing_target_exits_non_zero() {
    let (ok, _stdout, stderr, code) = run(s7cmd().arg("create-bucket"));
    assert!(!ok);
    assert_eq!(code, Some(2), "clap missing-arg should exit 2");
    assert!(stderr.to_lowercase().contains("required") || stderr.to_lowercase().contains("usage"));
}

// NOTE: s3util-rs has a `auto_complete_shell_short_circuits_without_target`
// test for each subcommand. s7cmd intentionally hides the per-subcommand
// `--auto-complete-shell` flag (see src/cli.rs cli_command()) and exposes
// only the top-level `s7cmd --auto-complete-shell <SHELL>` form, which is
// covered by tests/cli_help.rs::top_level_auto_complete_shell_runs.

#[test]
fn target_access_key_without_secret_exits_non_zero() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "create-bucket",
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
            || stderr.to_lowercase().contains("--target-secret-access-key")
    );
}

#[test]
fn target_no_sign_request_conflicts_with_target_profile() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "create-bucket",
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
fn mock_endpoint_if_not_exists_dry_run_skips_existing_bucket() {
    // HeadBucket → 200 (bucket exists) → --dry-run reports the would-skip.
    let server = MockS3Server::start(vec![MockResponse::new(200, "")]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["create-bucket", "--if-not-exists", "--dry-run"])
        .args(mock_target_args(&server.endpoint_url()))
        .arg("s3://mock-bucket");
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(code, Some(0), "expected success; stderr: {stderr}");
    assert!(
        stderr.contains("[dry-run] would skip: bucket exists."),
        "expected the dry-run skip line; got: {stderr}"
    );
}

#[test]
fn mock_endpoint_if_not_exists_head_failure_exits_1() {
    // HeadBucket → 403 (not a 404) → HeadError::Other bubbles up as an error.
    let server = MockS3Server::start(vec![MockResponse::s3_error(403, "AccessDenied")]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["create-bucket", "--if-not-exists"])
        .args(mock_target_args(&server.endpoint_url()))
        .arg("s3://mock-bucket");
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(1),
        "non-404 HeadBucket failure should exit 1; stderr: {stderr}"
    );
    assert!(
        stderr.contains("head-bucket on s3://mock-bucket"),
        "expected the head-bucket error context; got: {stderr}"
    );
}

#[test]
fn mock_endpoint_tagging_failure_after_create_warns_exit_3() {
    // CreateBucket → 200, then PutBucketTagging → 403: the bucket exists
    // untagged, so the run must exit 3 (warning), not 0 and not 1.
    let server = MockS3Server::start(vec![
        MockResponse::new(200, ""),
        MockResponse::s3_error(403, "AccessDenied"),
    ]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["create-bucket", "--tagging", "team=mock"])
        .args(mock_target_args(&server.endpoint_url()))
        .arg("s3://mock-bucket");
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(3),
        "tagging failure after create should exit 3; stderr: {stderr}"
    );
    assert!(
        stderr.contains("was created but PutBucketTagging failed"),
        "expected the partial-state warning; got: {stderr}"
    );
    let requests = server.requests();
    assert!(
        requests.iter().any(|r| r.contains("tagging")),
        "expected a PutBucketTagging request after CreateBucket; got: {requests:?}"
    );
}
