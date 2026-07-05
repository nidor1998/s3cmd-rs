//! Process-level CLI tests for the `rename` subcommand.
//! These run without AWS credentials or network access.

use assert_cmd::Command;
use predicates::prelude::*;

fn s7cmd() -> Command {
    Command::cargo_bin("s7cmd").unwrap()
}

// Express One Zone bucket name usable in no-AWS tests: passes validate() because
// the bucket ends with --x-s3, and source/target share the same bucket name.
const EXPR_BUCKET: &str = "s3://fake-bucket--apne1-az4--x-s3";

#[test]
fn rename_help_succeeds_and_lists_option_groups() {
    s7cmd()
        .args(["rename", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("AWS Configuration"))
        .stdout(predicate::str::contains("Conditional Checks"))
        .stdout(predicate::str::contains("--source-if-match"))
        .stdout(predicate::str::contains("--target-if-none-match"))
        .stdout(predicate::str::contains("--dry-run"));
}

#[test]
fn rename_top_level_help_lists_rename() {
    s7cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("rename"));
}

#[test]
fn rename_missing_args_exits_2() {
    s7cmd().arg("rename").assert().failure().code(2);
}

#[test]
fn rename_missing_target_exits_2() {
    s7cmd()
        .args(["rename", &format!("{EXPR_BUCKET}/src-key")])
        .assert()
        .failure()
        .code(2);
}

// NOTE: s3util-rs's auto_complete_shell_short_circuits_without_positional_args test
// is omitted — s7cmd hides the per-subcommand --auto-complete-shell flag (see
// cli_command() in src/cli.rs). The top-level `s7cmd --auto-complete-shell bash`
// form is tested in tests/cli_help.rs.

#[test]
fn rename_source_bucket_only_exits_2() {
    // s3://bucket with no key → validate() → source_bucket_key() error → exit 2
    s7cmd()
        .args([
            "rename",
            "s3://fake-bucket--apne1-az4--x-s3",
            &format!("{EXPR_BUCKET}/dst"),
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn rename_target_bucket_only_exits_2() {
    // source is valid, but target has no key → exit 2
    s7cmd()
        .args([
            "rename",
            &format!("{EXPR_BUCKET}/src"),
            "s3://fake-bucket--apne1-az4--x-s3",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn rename_non_express_onezone_bucket_exits_2() {
    s7cmd()
        .args([
            "rename",
            "s3://regular-bucket/src-key",
            "s3://regular-bucket/dst-key",
        ])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("Express").or(predicate::str::contains("x-s3")));
}

#[test]
fn rename_different_buckets_exits_2() {
    s7cmd()
        .args([
            "rename",
            "s3://bucket-a--apne1-az4--x-s3/src-key",
            "s3://bucket-b--apne1-az4--x-s3/dst-key",
        ])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("same bucket"));
}

#[test]
fn rename_source_access_key_without_secret_exits_2() {
    s7cmd()
        .args([
            "rename",
            &format!("{EXPR_BUCKET}/src"),
            &format!("{EXPR_BUCKET}/dst"),
            "--source-access-key",
            "AKIAIOSFODNN7EXAMPLE",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn rename_source_profile_conflicts_with_access_key_exits_2() {
    s7cmd()
        .args([
            "rename",
            &format!("{EXPR_BUCKET}/src"),
            &format!("{EXPR_BUCKET}/dst"),
            "--source-profile",
            "myprofile",
            "--source-access-key",
            "AKID",
            "--source-secret-access-key",
            "SECRET",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn rename_source_if_match_and_source_if_none_match_are_mutually_exclusive() {
    s7cmd()
        .args([
            "rename",
            &format!("{EXPR_BUCKET}/src"),
            &format!("{EXPR_BUCKET}/dst"),
            "--source-if-match",
            "\"abc123\"",
            "--source-if-none-match",
            "\"def456\"",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn rename_target_if_match_and_target_if_none_match_are_mutually_exclusive() {
    s7cmd()
        .args([
            "rename",
            &format!("{EXPR_BUCKET}/src"),
            &format!("{EXPR_BUCKET}/dst"),
            "--target-if-match",
            "\"abc123\"",
            "--target-if-none-match",
            "\"def456\"",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn rename_empty_source_if_match_exits_2() {
    s7cmd()
        .args([
            "rename",
            &format!("{EXPR_BUCKET}/src"),
            &format!("{EXPR_BUCKET}/dst"),
            "--source-if-match",
            "",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn rename_empty_source_if_none_match_exits_2() {
    s7cmd()
        .args([
            "rename",
            &format!("{EXPR_BUCKET}/src"),
            &format!("{EXPR_BUCKET}/dst"),
            "--source-if-none-match",
            "",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn rename_empty_target_if_match_exits_2() {
    s7cmd()
        .args([
            "rename",
            &format!("{EXPR_BUCKET}/src"),
            &format!("{EXPR_BUCKET}/dst"),
            "--target-if-match",
            "",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn rename_empty_target_if_none_match_exits_2() {
    s7cmd()
        .args([
            "rename",
            &format!("{EXPR_BUCKET}/src"),
            &format!("{EXPR_BUCKET}/dst"),
            "--target-if-none-match",
            "",
        ])
        .assert()
        .failure()
        .code(2);
}

mod common;

#[test]
fn mock_endpoint_no_such_bucket_exits_1_with_message() {
    // RenameObject → 404 NoSuchBucket: unlike the get-* subcommands, rename
    // maps BucketNotFound to a hard error (exit 1), not exit 4.
    let server =
        common::MockS3Server::start(vec![common::MockResponse::s3_error(404, "NoSuchBucket")]);
    let mut cmd = common::s7cmd_cmd_clean_env();
    // Directory-bucket ops default to S3 Express CreateSession auth; disable
    // it so the canned 404 reaches RenameObject as a plain service error.
    cmd.env("AWS_S3_DISABLE_EXPRESS_SESSION_AUTH", "true");
    cmd.arg("rename")
        .args(common::mock_source_args(&server.endpoint_url()))
        .args([
            "s3://fake-bucket--apne1-az4--x-s3/src-key",
            "s3://fake-bucket--apne1-az4--x-s3/dst-key",
        ]);
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(1),
        "NoSuchBucket on rename should exit 1; stderr: {stderr}"
    );
    assert!(
        stderr.contains("bucket s3://fake-bucket--apne1-az4--x-s3 not found"),
        "expected the bucket-not-found message; got: {stderr}"
    );
}
