//! Process-level CLI tests for the `put-bucket-replication` subcommand.
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
fn help_shows_both_positionals() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["put-bucket-replication", "--help"]));
    assert!(ok, "put-bucket-replication --help must succeed");
    assert!(
        stdout.contains("TARGET") || stdout.contains("BUCKET"),
        "expected TARGET or BUCKET in help; got: {stdout}"
    );
    assert!(
        stdout.contains("REPLICATION_CONFIGURATION"),
        "expected REPLICATION_CONFIGURATION in help; got: {stdout}"
    );
    assert!(stdout.contains("AWS Configuration"));
    assert!(stdout.contains("Retry Options"));
    assert!(stdout.contains("Timeout Options"));
}

#[test]
fn missing_both_positionals_exits_2() {
    let (ok, _stdout, stderr, code) = run(s7cmd().arg("put-bucket-replication"));
    assert!(!ok);
    assert_eq!(code, Some(2), "stderr: {stderr}");
    assert!(
        stderr.to_lowercase().contains("required") || stderr.to_lowercase().contains("usage"),
        "expected 'required' or 'usage'; got: {stderr}"
    );
}

#[test]
fn missing_config_positional_exits_2() {
    let (ok, _stdout, stderr, code) =
        run(s7cmd().args(["put-bucket-replication", "s3://example-bucket"]));
    assert!(!ok);
    assert_eq!(code, Some(2), "stderr: {stderr}");
}

#[test]
fn nonexistent_config_file_exits_1() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-replication",
        "s3://example-bucket",
        "/nonexistent/path/replication-xyz-does-not-exist.json",
    ]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(1),
        "reading non-existent file must exit 1; got {code:?}; stderr: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("no such file")
            || stderr.to_lowercase().contains("not found")
            || stderr.to_lowercase().contains("os error"),
        "expected file-not-found error in stderr; got: {stderr}"
    );
}

#[test]
fn malformed_json_exits_1() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), b"not valid json {").unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-replication",
        "s3://example-bucket",
        tmp.path().to_str().unwrap(),
    ]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(1),
        "malformed JSON must exit 1; got {code:?}; stderr: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("parsing json from"),
        "expected 'parsing JSON from' in stderr; got: {stderr}"
    );
}

#[test]
fn target_access_key_without_secret_exits_non_zero() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-replication",
        "s3://example",
        tmp.path().to_str().unwrap(),
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
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-replication",
        "s3://example",
        tmp.path().to_str().unwrap(),
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
fn bucket_with_key_exits_1() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-replication",
        "s3://example/key",
        tmp.path().to_str().unwrap(),
    ]));
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
fn mock_endpoint_put_replication_succeeds() {
    let config = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        config.path(),
        r#"{"Role":"arn:aws:iam::123456789012:role/replication","Rules":[{"ID":"rule-1","Status":"Enabled","Destination":{"Bucket":"arn:aws:s3:::dest-bucket"}}]}"#,
    )
    .unwrap();
    let server = MockS3Server::start(vec![MockResponse::new(200, "")]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["put-bucket-replication", "-v"])
        .args(mock_target_args(&server.endpoint_url()))
        .args(["s3://mock-bucket", config.path().to_str().unwrap()]);
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(code, Some(0), "expected success; stderr: {stderr}");
    assert!(
        stderr.contains("Bucket replication set."),
        "expected the success info line (-v); got: {stderr}"
    );
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.starts_with("PUT ") && r.contains("replication")),
        "expected a PutBucketReplication request; got: {:?}",
        server.requests()
    );
}
