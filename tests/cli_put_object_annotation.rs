//! Process-level CLI tests for the `put-object-annotation` subcommand.
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
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["put-object-annotation", "--help"]));
    assert!(ok, "put-object-annotation --help must succeed");
    assert!(stdout.contains("AWS Configuration"));
    assert!(stdout.contains("Retry Options"));
    assert!(stdout.contains("Timeout Options"));
}

#[test]
fn missing_positional_exits_2() {
    let (ok, _stdout, stderr, code) = run(s7cmd().arg("put-object-annotation"));
    assert!(!ok);
    assert_eq!(code, Some(2), "clap missing-arg should exit 2");
    assert!(stderr.to_lowercase().contains("required") || stderr.to_lowercase().contains("usage"));
}

#[test]
fn missing_annotation_name_exits_2() {
    // target + --annotation-payload present, but --annotation-name is required.
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-object-annotation",
        "s3://bucket/key",
        "--annotation-payload",
        "-",
    ]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(2),
        "--annotation-name is required; should exit 2"
    );
    assert!(stderr.to_lowercase().contains("required") || stderr.to_lowercase().contains("usage"));
}

#[test]
fn missing_annotation_payload_exits_2() {
    // target + --annotation-name present, but --annotation-payload is required.
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-object-annotation",
        "s3://bucket/key",
        "--annotation-name",
        "note",
    ]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(2),
        "--annotation-payload is required; should exit 2"
    );
    assert!(stderr.to_lowercase().contains("required") || stderr.to_lowercase().contains("usage"));
}

#[test]
fn bucket_only_path_no_key_exits_1() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-object-annotation",
        "s3://bucket",
        "--annotation-name",
        "note",
        "--annotation-payload",
        "-",
    ]));
    assert!(!ok);
    assert_eq!(code, Some(1), "bucket-only path should exit 1 (validation)");
    assert!(
        !stderr.is_empty(),
        "should have an error message on stderr; got empty"
    );
}

// NOTE: s3util-rs's auto_complete_shell_short_circuits_without_target test
// is omitted — s7cmd hides the per-subcommand --auto-complete-shell flag
// and exposes only the top-level form (covered by tests/cli_help.rs).

#[test]
fn target_access_key_without_secret_exits_non_zero() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-object-annotation",
        "s3://bucket/key",
        "--annotation-name",
        "note",
        "--annotation-payload",
        "-",
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
fn help_mentions_annotation_options_and_target_version_id() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["put-object-annotation", "--help"]));
    assert!(ok);
    assert!(
        stdout.contains("annotation-name"),
        "help should list --annotation-name; got: {stdout}"
    );
    assert!(
        stdout.contains("annotation-payload"),
        "help should list --annotation-payload; got: {stdout}"
    );
    assert!(
        stdout.contains("target-version-id"),
        "help should list --target-version-id; got: {stdout}"
    );
}
// NOTE: --dry-run smoke + help-exposure coverage for put-/delete-object-annotation
// lives centrally in tests/cli_dry_run.rs, matching the other mutating subcommands.

// ---------- stdin payload and mock-endpoint tests ----------

#[test]
fn stdin_payload_dry_run_exits_0() {
    // `--annotation-payload -` reads the payload from stdin; --dry-run
    // validates it and short-circuits before any network call.
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_s7cmd"))
        .args([
            "put-object-annotation",
            "--dry-run",
            "--annotation-name",
            "note",
            "--annotation-payload",
            "-",
            "s3://mock-bucket/mock-key",
        ])
        .env_remove("RUST_LOG")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn s7cmd binary");
    child
        .stdin
        .take()
        .expect("child stdin")
        .write_all(b"annotation payload via stdin")
        .expect("write payload to child stdin");
    let output = child.wait_with_output().expect("wait for s7cmd");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stdin payload dry-run should exit 0; stderr: {stderr}"
    );
    assert!(
        stderr.contains("[dry-run] would put object annotation."),
        "expected the dry-run line; got: {stderr}"
    );
}

fn put_annotation_cmd(server: &MockS3Server, payload: &std::path::Path, extra: &[&str]) -> Command {
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args([
        "put-object-annotation",
        "--annotation-name",
        "note",
        "--annotation-payload",
        payload.to_str().unwrap(),
    ])
    .args(extra)
    .args(mock_target_args(&server.endpoint_url()))
    .arg("s3://mock-bucket/mock-key");
    cmd
}

#[test]
fn mock_endpoint_no_such_version_exits_4_with_version_in_message() {
    let payload = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(payload.path(), b"mock payload").unwrap();
    let server = MockS3Server::start(vec![MockResponse::s3_error(404, "NoSuchVersion")]);
    let (code, _stdout, stderr) = common::run(&mut put_annotation_cmd(
        &server,
        payload.path(),
        &["--target-version-id", "mock-version"],
    ));
    assert_eq!(
        code,
        Some(4),
        "NoSuchVersion should exit 4; stderr: {stderr}"
    );
    assert!(
        stderr.contains("s3://mock-bucket/mock-key (versionId=mock-version) not found"),
        "expected the versioned not-found message; got: {stderr}"
    );
}

#[test]
fn mock_endpoint_access_denied_exits_1() {
    let payload = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(payload.path(), b"mock payload").unwrap();
    let server = MockS3Server::start(vec![MockResponse::s3_error(403, "AccessDenied")]);
    let (code, _stdout, stderr) =
        common::run(&mut put_annotation_cmd(&server, payload.path(), &[]));
    assert_eq!(
        code,
        Some(1),
        "unclassified S3 errors should exit 1; stderr: {stderr}"
    );
    assert!(
        stderr.contains("AccessDenied"),
        "expected the AccessDenied error chain; got: {stderr}"
    );
}
