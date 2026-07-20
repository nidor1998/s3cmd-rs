//! Process-level CLI tests for the `put-bucket-lifecycle-configuration` subcommand.
//! These run without AWS credentials or network access.

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
    let (ok, stdout, _stderr, _code) =
        run(s7cmd().args(["put-bucket-lifecycle-configuration", "--help"]));
    assert!(ok, "put-bucket-lifecycle-configuration --help must succeed");
    assert!(
        stdout.contains("TARGET") || stdout.contains("BUCKET"),
        "expected TARGET or BUCKET in help; got: {stdout}"
    );
    assert!(
        stdout.contains("LIFECYCLE_CONFIGURATION"),
        "expected LIFECYCLE_CONFIGURATION in help; got: {stdout}"
    );
    assert!(stdout.contains("AWS Configuration"));
    assert!(stdout.contains("Retry Options"));
    assert!(stdout.contains("Timeout Options"));
}

#[test]
fn missing_both_positionals_exits_2() {
    let (ok, _stdout, stderr, code) = run(s7cmd().arg("put-bucket-lifecycle-configuration"));
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
        run(s7cmd().args(["put-bucket-lifecycle-configuration", "s3://example-bucket"]));
    assert!(!ok);
    assert_eq!(code, Some(2), "stderr: {stderr}");
}

#[test]
fn nonexistent_config_file_exits_1() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-lifecycle-configuration",
        "s3://example-bucket",
        "/nonexistent/path/lifecycle-xyz-does-not-exist.json",
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

// NOTE: s3util-rs's auto_complete_shell_short_circuits_without_target test
// is omitted — s7cmd hides the per-subcommand --auto-complete-shell flag
// and exposes only the top-level form (covered by tests/cli_help.rs).

#[test]
fn target_access_key_without_secret_exits_non_zero() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-lifecycle-configuration",
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
fn malformed_json_exits_1() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), b"not valid json {").unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-lifecycle-configuration",
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

/// A top-level `TransitionDefaultMinimumObjectSize` — as emitted by
/// `get-bucket-lifecycle-configuration` — must be rejected with guidance, not
/// silently dropped (dropping it resets the bucket to S3's default on a
/// get-edit-put roundtrip). Adapted from s3util-rs PR#25; the parse rejection
/// fires before any AWS call, so this runs without credentials or network.
#[test]
fn transition_default_minimum_object_size_in_json_exits_1_with_guidance() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        tmp.path(),
        br#"{"Rules": [{"ID": "r1", "Status": "Enabled",
            "Expiration": {"Days": 30}, "Filter": {"Prefix": ""}}],
            "TransitionDefaultMinimumObjectSize": "all_storage_classes_128K"}"#,
    )
    .unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-lifecycle-configuration",
        "s3://example-bucket",
        tmp.path().to_str().unwrap(),
    ]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(1),
        "TransitionDefaultMinimumObjectSize in the document must exit 1; got {code:?}; stderr: {stderr}"
    );
    assert!(
        stderr.contains("TransitionDefaultMinimumObjectSize")
            && stderr.contains("request parameter"),
        "expected guidance naming the field and calling it a request parameter; got: {stderr}"
    );
}

#[test]
fn target_no_sign_request_conflicts_with_target_profile() {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-lifecycle-configuration",
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
        "put-bucket-lifecycle-configuration",
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
