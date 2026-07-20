//! Process-level CLI tests — invocations whose validation happens entirely
//! before any AWS call. Run as part of default `cargo test`; no AWS profile
//! or network required.
//!
//! Each test asserts on exit code (clap returns 2 for arg errors) and on a
//! non-empty stderr (clap or value-parser error message).

mod common;

use common::{run, s7cmd_cmd};

// ---- Top-level ----

#[test]
fn no_subcommand_exits_2_with_usage() {
    let (code, _stdout, stderr) = run(&mut s7cmd_cmd());
    assert_eq!(code, Some(2), "no subcommand must exit 2; stderr={stderr}");
    assert!(
        stderr.to_lowercase().contains("usage"),
        "expected usage on stderr; got: {stderr}"
    );
}

#[test]
fn unrecognized_subcommand_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("not-a-real-cmd"));
    assert_eq!(code, Some(2), "unrecognized subcommand must exit 2");
    assert!(
        stderr.contains("unrecognized subcommand"),
        "expected 'unrecognized subcommand' on stderr; got: {stderr}"
    );
}

// ---- sync ----

#[test]
fn sync_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("sync"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty(), "sync with no args must produce stderr");
}

// ---- ls ----

#[test]
fn ls_invalid_target_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().args(["ls", "notavalidpath"]));
    assert_eq!(code, Some(2));
    assert!(
        stderr.contains("must be an S3 path"),
        "expected S3 path error; got: {stderr}"
    );
}

// ---- clean ----

#[test]
fn clean_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("clean"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- cp ----

#[test]
fn cp_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("cp"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

#[test]
fn cp_missing_target_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().args(["cp", "s3://b/k"]));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- mv ----

#[test]
fn mv_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("mv"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

#[test]
fn mv_missing_target_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().args(["mv", "s3://b/k"]));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- mv self-move guard (ported from s3util-rs PR#25) ----
//
// mv is copy-then-delete, so moving an object onto itself would delete the
// object the copy just wrote. The guard fires inside run_mv before any client
// is built, so these run without AWS credentials or network — but as a
// runtime rejection, not a clap error, the exit code is 1 (not 2).

#[test]
fn mv_self_move_identical_keys_exits_1() {
    let (code, _stdout, stderr) =
        run(s7cmd_cmd().args(["mv", "s3://b/dir/k.txt", "s3://b/dir/k.txt"]));
    assert_eq!(code, Some(1), "mv onto itself must exit 1; stderr={stderr}");
    assert!(
        stderr.contains("onto itself"),
        "expected the self-move rejection on stderr; got: {stderr}"
    );
}

#[test]
fn mv_self_move_directory_style_target_exits_1() {
    // `mv s3://b/dir/k.txt s3://b/dir/` resolves the target to the source key
    // by appending the basename — the same data-loss case spelled differently.
    let (code, _stdout, stderr) = run(s7cmd_cmd().args(["mv", "s3://b/dir/k.txt", "s3://b/dir/"]));
    assert_eq!(
        code,
        Some(1),
        "directory-style self-move must exit 1; stderr={stderr}"
    );
    assert!(
        stderr.contains("onto itself"),
        "expected the self-move rejection on stderr; got: {stderr}"
    );
}

// ---- cp --skip-existing validation (s3util-rs 1.2.0) ----

#[test]
fn cp_skip_existing_with_stdio_target_rejected() {
    // s3util-rs 1.2.0 rejects --skip-existing with a stdout target at
    // Config::try_from. s7cmd surfaces the message via dispatch and
    // returns 2 (clap ValueValidation).
    let (code, _stdout, stderr) = run(s7cmd_cmd().args(["cp", "--skip-existing", "s3://b/k", "-"]));
    assert_eq!(
        code,
        Some(2),
        "cp --skip-existing with stdout target must exit 2; stderr={stderr}"
    );
    assert!(
        stderr.contains("stdout target"),
        "expected stdout target error.\n--- stderr ---\n{stderr}"
    );
}

#[test]
fn cp_skip_existing_with_if_none_match_rejected() {
    // --skip-existing (skip-if-exists) is the inverse of --if-none-match
    // (fail-if-exists). s3util-rs 1.2.0 rejects the combination.
    let (code, _stdout, stderr) = run(s7cmd_cmd().args([
        "cp",
        "--skip-existing",
        "--if-none-match",
        "/tmp/a",
        "s3://b/k",
    ]));
    assert_eq!(
        code,
        Some(2),
        "cp --skip-existing --if-none-match must exit 2; stderr={stderr}"
    );
    assert!(
        stderr.contains("--if-none-match"),
        "expected --if-none-match error.\n--- stderr ---\n{stderr}"
    );
}

// ---- rm ----

#[test]
fn rm_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("rm"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- head-object ----

#[test]
fn head_object_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("head-object"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-object-tagging ----

#[test]
fn get_object_tagging_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-object-tagging"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-object-tagging ----

#[test]
fn put_object_tagging_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-object-tagging"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-object-tagging ----

#[test]
fn delete_object_tagging_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-object-tagging"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-object-annotation ----

#[test]
fn get_object_annotation_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-object-annotation"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-object-annotation ----

#[test]
fn put_object_annotation_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-object-annotation"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-object-annotation ----

#[test]
fn delete_object_annotation_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-object-annotation"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- list-object-annotations ----

#[test]
fn list_object_annotations_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("list-object-annotations"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- create-bucket ----

#[test]
fn create_bucket_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("create-bucket"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-bucket ----

#[test]
fn delete_bucket_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-bucket"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- head-bucket ----

#[test]
fn head_bucket_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("head-bucket"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-tagging ----

#[test]
fn get_bucket_tagging_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-tagging"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-tagging ----

#[test]
fn put_bucket_tagging_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-tagging"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-bucket-tagging ----

#[test]
fn delete_bucket_tagging_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-bucket-tagging"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-policy ----

#[test]
fn get_bucket_policy_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-policy"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-policy ----

#[test]
fn put_bucket_policy_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-policy"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-bucket-policy ----

#[test]
fn delete_bucket_policy_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-bucket-policy"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-versioning ----

#[test]
fn get_bucket_versioning_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-versioning"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-versioning ----

#[test]
fn put_bucket_versioning_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-versioning"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-lifecycle-configuration ----

#[test]
fn get_bucket_lifecycle_configuration_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-lifecycle-configuration"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-lifecycle-configuration ----

#[test]
fn put_bucket_lifecycle_configuration_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-lifecycle-configuration"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-bucket-lifecycle-configuration ----

#[test]
fn delete_bucket_lifecycle_configuration_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-bucket-lifecycle-configuration"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-encryption ----

#[test]
fn get_bucket_encryption_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-encryption"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-encryption ----

#[test]
fn put_bucket_encryption_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-encryption"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-bucket-encryption ----

#[test]
fn delete_bucket_encryption_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-bucket-encryption"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-cors ----

#[test]
fn get_bucket_cors_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-cors"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-cors ----

#[test]
fn put_bucket_cors_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-cors"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-bucket-cors ----

#[test]
fn delete_bucket_cors_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-bucket-cors"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-public-access-block ----

#[test]
fn get_public_access_block_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-public-access-block"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-public-access-block ----

#[test]
fn put_public_access_block_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-public-access-block"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-public-access-block ----

#[test]
fn delete_public_access_block_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-public-access-block"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-website ----

#[test]
fn get_bucket_website_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-website"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-website ----

#[test]
fn put_bucket_website_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-website"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-bucket-website ----

#[test]
fn delete_bucket_website_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-bucket-website"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-logging ----

#[test]
fn get_bucket_logging_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-logging"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-logging ----

#[test]
fn put_bucket_logging_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-logging"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-notification-configuration ----

#[test]
fn get_bucket_notification_configuration_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-notification-configuration"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-notification-configuration ----

#[test]
fn put_bucket_notification_configuration_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-notification-configuration"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-replication ----

#[test]
fn get_bucket_replication_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-replication"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-replication ----

#[test]
fn put_bucket_replication_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-replication"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- delete-bucket-replication ----

#[test]
fn delete_bucket_replication_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("delete-bucket-replication"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-accelerate-configuration ----

#[test]
fn get_bucket_accelerate_configuration_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-accelerate-configuration"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-accelerate-configuration ----

#[test]
fn put_bucket_accelerate_configuration_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-accelerate-configuration"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-request-payment ----

#[test]
fn get_bucket_request_payment_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-request-payment"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- put-bucket-request-payment ----

#[test]
fn put_bucket_request_payment_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("put-bucket-request-payment"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- get-bucket-policy-status ----

#[test]
fn get_bucket_policy_status_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("get-bucket-policy-status"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- restore-object ----

#[test]
fn restore_object_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("restore-object"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}

// ---- presign ----

#[test]
fn presign_no_args_exits_2() {
    let (code, _stdout, stderr) = run(s7cmd_cmd().arg("presign"));
    assert_eq!(code, Some(2));
    assert!(!stderr.is_empty());
}
