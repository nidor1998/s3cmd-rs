//! Process-level tests for runtime error paths that are reachable without
//! AWS: every command below points at a loopback endpoint with nothing
//! listening (connection refused is immediate), with fake credentials in
//! env so client construction succeeds and failure happens at the request
//! layer. Each test pins the family's error exit code and the message its
//! runner logs on the failure path.

mod common;

use common::{run, s7cmd_cmd_clean_env};
use std::process::Command;

/// Refused loopback endpoint; nothing ever listens on port 9 (discard).
const DEAD_ENDPOINT: &str = "http://127.0.0.1:9";

fn with_fake_aws_env(cmd: &mut Command) -> &mut Command {
    cmd.env("AWS_ACCESS_KEY_ID", "test")
        .env("AWS_SECRET_ACCESS_KEY", "test")
        .env("AWS_REGION", "us-east-1")
}

/// A sync whose listing fails must take the `has_error` path: log
/// "s7cmd sync failed." (sync_bin/cli — sync's tracing sink is stdout),
/// return Err, and exit 1 via the dispatch sync arm's error branch.
#[test]
fn sync_pipeline_failure_logs_and_exits_1() {
    let mut cmd = s7cmd_cmd_clean_env();
    with_fake_aws_env(&mut cmd).args([
        "sync",
        "s3://src-bucket/",
        "s3://dst-bucket/",
        "--source-endpoint-url",
        DEAD_ENDPOINT,
        "--target-endpoint-url",
        DEAD_ENDPOINT,
        "--aws-max-attempts",
        "1",
    ]);
    let (code, stdout, stderr) = run(&mut cmd);
    assert_eq!(
        code,
        Some(1),
        "failed sync must exit 1; stdout={stdout} stderr={stderr}"
    );
    assert!(
        stdout.contains("s7cmd sync failed."),
        "expected the sync failure log on stdout (sync's tracing sink); \
         stdout={stdout} stderr={stderr}"
    );
}

/// A clean whose object listing fails must take the `has_error` path:
/// log "s7cmd clean failed." (clean_bin) and exit 1.
#[test]
fn clean_listing_failure_logs_and_exits_1() {
    let mut cmd = s7cmd_cmd_clean_env();
    with_fake_aws_env(&mut cmd).args([
        "clean",
        "--force",
        "s3://some-bucket/prefix/",
        "--target-endpoint-url",
        DEAD_ENDPOINT,
        "--aws-max-attempts",
        "1",
    ]);
    let (code, _stdout, stderr) = run(&mut cmd);
    assert_eq!(code, Some(1), "failed clean must exit 1; stderr={stderr}");
    assert!(
        stderr.contains("s7cmd clean failed."),
        "expected the clean failure log; got: {stderr}"
    );
}

/// `cp --dry-run` with annotation sync enabled builds a source client
/// (including the `--source-request-payer` requester option) and lists
/// object annotations even in dry-run; with a dead endpoint the listing
/// fails and cp exits 1 with the annotation error surfaced.
#[test]
fn cp_dry_run_annotation_sync_with_request_payer_fails_cleanly() {
    let mut cmd = s7cmd_cmd_clean_env();
    with_fake_aws_env(&mut cmd).args([
        "cp",
        "--dry-run",
        "--enable-sync-object-annotations",
        "--source-request-payer",
        "s3://src-bucket/key",
        "s3://dst-bucket/key",
        "--source-endpoint-url",
        DEAD_ENDPOINT,
        "--target-endpoint-url",
        DEAD_ENDPOINT,
        "--aws-max-attempts",
        "1",
    ]);
    let (code, _stdout, stderr) = run(&mut cmd);
    assert_eq!(
        code,
        Some(1),
        "annotation listing failure must exit 1; stderr={stderr}"
    );
    assert!(
        stderr.contains("list_object_annotations"),
        "expected the annotation-listing error; got: {stderr}"
    );
}
