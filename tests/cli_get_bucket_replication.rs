//! Process-level CLI tests for the `get-bucket-replication` subcommand.
//! These run without AWS credentials or network access (the mock-endpoint
//! test talks only to a loopback HTTP server).

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
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["get-bucket-replication", "--help"]));
    assert!(ok, "get-bucket-replication --help must succeed");
    assert!(stdout.contains("AWS Configuration"));
    assert!(stdout.contains("Retry Options"));
    assert!(stdout.contains("Timeout Options"));
}

#[test]
fn missing_target_exits_non_zero() {
    let (ok, _stdout, stderr, code) = run(s7cmd().arg("get-bucket-replication"));
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
fn bucket_with_key_exits_1() {
    let (ok, _stdout, stderr, code) =
        run(s7cmd().args(["get-bucket-replication", "s3://example/key"]));
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
fn mock_endpoint_success_prints_replication_json() {
    let server = MockS3Server::start(vec![MockResponse::new(
        200,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
         <ReplicationConfiguration>\
           <Role>arn:aws:iam::123456789012:role/replication</Role>\
           <Rule>\
             <ID>rule-1</ID>\
             <Status>Enabled</Status>\
             <Prefix></Prefix>\
             <Destination><Bucket>arn:aws:s3:::dest-bucket</Bucket></Destination>\
           </Rule>\
         </ReplicationConfiguration>",
    )]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.arg("get-bucket-replication")
        .args(mock_target_args(&server.endpoint_url()))
        .arg("s3://mock-bucket");
    let (code, stdout, stderr) = common::run(&mut cmd);
    assert_eq!(code, Some(0), "expected success; stderr: {stderr}");
    assert!(
        stdout.contains("\"Role\": \"arn:aws:iam::123456789012:role/replication\""),
        "stdout should contain the pretty-printed Role; got: {stdout}"
    );
    assert!(
        stdout.contains("\"Rules\""),
        "stdout should contain the Rules array; got: {stdout}"
    );
    assert!(
        server
            .requests()
            .iter()
            .any(|r| r.starts_with("GET ") && r.contains("replication")),
        "expected a GetBucketReplication request; got: {:?}",
        server.requests()
    );
}

#[test]
fn target_access_key_without_secret_exits_non_zero() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "get-bucket-replication",
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
