use clap::Parser;

// Re-import the binary crate's modules into the integration test.
// Integration tests can't access the binary's private modules normally,
// so we rebuild Cli here from the public lib types — equivalent shape.
//
// (If/when we expose Cli as part of a thin lib, replace this with
// `use s7cmd::cli::{Cli, Cmd};`.)

#[path = "../src/cli.rs"]
mod cli;

use cli::{Cli, Cmd};

#[test]
fn parses_ls_with_target() {
    let cli = Cli::try_parse_from(["s7cmd", "ls", "s3://bucket"]).expect("ls should parse");
    assert!(matches!(cli.command, Some(Cmd::Ls(_))));
}

#[test]
fn parses_cp_with_two_paths() {
    let cli = Cli::try_parse_from(["s7cmd", "cp", "/tmp/file", "s3://bucket/key"])
        .expect("cp should parse");
    assert!(matches!(cli.command, Some(Cmd::Cp(_))));
}

#[test]
fn parses_mv_with_two_paths() {
    let cli =
        Cli::try_parse_from(["s7cmd", "mv", "s3://b1/k1", "s3://b2/k2"]).expect("mv should parse");
    assert!(matches!(cli.command, Some(Cmd::Mv(_))));
}

#[test]
fn parses_rm_with_one_path() {
    let cli = Cli::try_parse_from(["s7cmd", "rm", "s3://bucket/key"]).expect("rm should parse");
    assert!(matches!(cli.command, Some(Cmd::Rm(_))));
}

#[test]
fn parses_sync_with_two_paths() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "sync",
        "--allow-both-local-storage",
        "/tmp/src",
        "/tmp/dst",
    ])
    .expect("sync should parse");
    assert!(matches!(cli.command, Some(Cmd::Sync(_))));
}

#[test]
fn parses_clean_with_target() {
    let cli = Cli::try_parse_from(["s7cmd", "clean", "s3://bucket"]).expect("clean should parse");
    assert!(matches!(cli.command, Some(Cmd::Clean(_))));
}

#[test]
fn parses_head_object() {
    let cli = Cli::try_parse_from(["s7cmd", "head-object", "s3://bucket/key"])
        .expect("head-object should parse");
    assert!(matches!(cli.command, Some(Cmd::HeadObject(_))));
}

#[test]
fn parses_get_object_tagging() {
    let cli = Cli::try_parse_from(["s7cmd", "get-object-tagging", "s3://bucket/key"])
        .expect("get-object-tagging should parse");
    assert!(matches!(cli.command, Some(Cmd::GetObjectTagging(_))));
}

#[test]
fn parses_put_object_tagging() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-object-tagging",
        "s3://bucket/key",
        "--tagging",
        "k=v",
    ])
    .expect("put-object-tagging should parse");
    assert!(matches!(cli.command, Some(Cmd::PutObjectTagging(_))));
}

#[test]
fn parses_delete_object_tagging() {
    let cli = Cli::try_parse_from(["s7cmd", "delete-object-tagging", "s3://bucket/key"])
        .expect("delete-object-tagging should parse");
    assert!(matches!(cli.command, Some(Cmd::DeleteObjectTagging(_))));
}

#[test]
fn parses_get_object_annotation() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "get-object-annotation",
        "s3://bucket/key",
        "-",
        "--annotation-name",
        "note",
    ])
    .expect("get-object-annotation should parse");
    assert!(matches!(cli.command, Some(Cmd::GetObjectAnnotation(_))));
}

#[test]
fn parses_put_object_annotation() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-object-annotation",
        "s3://bucket/key",
        "--annotation-name",
        "note",
        "--annotation-payload",
        "-",
    ])
    .expect("put-object-annotation should parse");
    assert!(matches!(cli.command, Some(Cmd::PutObjectAnnotation(_))));
}

#[test]
fn parses_delete_object_annotation() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "delete-object-annotation",
        "s3://bucket/key",
        "--annotation-name",
        "note",
    ])
    .expect("delete-object-annotation should parse");
    assert!(matches!(cli.command, Some(Cmd::DeleteObjectAnnotation(_))));
}

#[test]
fn parses_list_object_annotations() {
    let cli = Cli::try_parse_from(["s7cmd", "list-object-annotations", "s3://bucket/key"])
        .expect("list-object-annotations should parse");
    assert!(matches!(cli.command, Some(Cmd::ListObjectAnnotations(_))));
}

#[test]
fn parses_create_bucket() {
    let cli = Cli::try_parse_from(["s7cmd", "create-bucket", "s3://bucket"])
        .expect("create-bucket should parse");
    assert!(matches!(cli.command, Some(Cmd::CreateBucket(_))));
}

#[test]
fn parses_delete_bucket() {
    let cli = Cli::try_parse_from(["s7cmd", "delete-bucket", "s3://bucket"])
        .expect("delete-bucket should parse");
    assert!(matches!(cli.command, Some(Cmd::DeleteBucket(_))));
}

#[test]
fn parses_head_bucket() {
    let cli = Cli::try_parse_from(["s7cmd", "head-bucket", "s3://bucket"])
        .expect("head-bucket should parse");
    assert!(matches!(cli.command, Some(Cmd::HeadBucket(_))));
}

#[test]
fn parses_get_bucket_tagging() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-tagging", "s3://bucket"])
        .expect("get-bucket-tagging should parse");
    assert!(matches!(cli.command, Some(Cmd::GetBucketTagging(_))));
}

#[test]
fn parses_put_bucket_tagging() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-bucket-tagging",
        "s3://bucket",
        "--tagging",
        "k=v",
    ])
    .expect("put-bucket-tagging should parse");
    assert!(matches!(cli.command, Some(Cmd::PutBucketTagging(_))));
}

#[test]
fn parses_delete_bucket_tagging() {
    let cli = Cli::try_parse_from(["s7cmd", "delete-bucket-tagging", "s3://bucket"])
        .expect("delete-bucket-tagging should parse");
    assert!(matches!(cli.command, Some(Cmd::DeleteBucketTagging(_))));
}

#[test]
fn parses_get_bucket_policy() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-policy", "s3://bucket"])
        .expect("get-bucket-policy should parse");
    assert!(matches!(cli.command, Some(Cmd::GetBucketPolicy(_))));
}

#[test]
fn parses_put_bucket_policy() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-bucket-policy",
        "s3://bucket",
        "/tmp/policy.json",
    ])
    .expect("put-bucket-policy should parse");
    assert!(matches!(cli.command, Some(Cmd::PutBucketPolicy(_))));
}

#[test]
fn parses_delete_bucket_policy() {
    let cli = Cli::try_parse_from(["s7cmd", "delete-bucket-policy", "s3://bucket"])
        .expect("delete-bucket-policy should parse");
    assert!(matches!(cli.command, Some(Cmd::DeleteBucketPolicy(_))));
}

#[test]
fn parses_get_bucket_versioning() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-versioning", "s3://bucket"])
        .expect("get-bucket-versioning should parse");
    assert!(matches!(cli.command, Some(Cmd::GetBucketVersioning(_))));
}

#[test]
fn parses_put_bucket_versioning() {
    let cli = Cli::try_parse_from(["s7cmd", "put-bucket-versioning", "s3://bucket", "--enabled"])
        .expect("put-bucket-versioning should parse");
    assert!(matches!(cli.command, Some(Cmd::PutBucketVersioning(_))));
}

#[test]
fn parses_top_level_auto_complete_shell() {
    let cli = Cli::try_parse_from(["s7cmd", "--auto-complete-shell", "bash"])
        .expect("top-level --auto-complete-shell should parse");
    assert!(cli.auto_complete_shell.is_some());
    assert!(cli.command.is_none());
}

#[test]
fn parses_top_level_auto_complete_shell_zsh() {
    let cli = Cli::try_parse_from(["s7cmd", "--auto-complete-shell", "zsh"])
        .expect("top-level --auto-complete-shell zsh should parse");
    assert!(cli.auto_complete_shell.is_some());
}

#[test]
fn parses_get_bucket_lifecycle_configuration() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-lifecycle-configuration", "s3://bucket"])
        .expect("get-bucket-lifecycle-configuration should parse");
    assert!(matches!(
        cli.command,
        Some(Cmd::GetBucketLifecycleConfiguration(_))
    ));
}

#[test]
fn parses_put_bucket_lifecycle_configuration() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-bucket-lifecycle-configuration",
        "s3://bucket",
        "/tmp/lifecycle.json",
    ])
    .expect("put-bucket-lifecycle-configuration should parse");
    assert!(matches!(
        cli.command,
        Some(Cmd::PutBucketLifecycleConfiguration(_))
    ));
}

#[test]
fn parses_delete_bucket_lifecycle_configuration() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "delete-bucket-lifecycle-configuration",
        "s3://bucket",
    ])
    .expect("delete-bucket-lifecycle-configuration should parse");
    assert!(matches!(
        cli.command,
        Some(Cmd::DeleteBucketLifecycleConfiguration(_))
    ));
}

#[test]
fn parses_get_bucket_encryption() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-encryption", "s3://bucket"])
        .expect("get-bucket-encryption should parse");
    assert!(matches!(cli.command, Some(Cmd::GetBucketEncryption(_))));
}

#[test]
fn parses_put_bucket_encryption() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-bucket-encryption",
        "s3://bucket",
        "/tmp/enc.json",
    ])
    .expect("put-bucket-encryption should parse");
    assert!(matches!(cli.command, Some(Cmd::PutBucketEncryption(_))));
}

#[test]
fn parses_delete_bucket_encryption() {
    let cli = Cli::try_parse_from(["s7cmd", "delete-bucket-encryption", "s3://bucket"])
        .expect("delete-bucket-encryption should parse");
    assert!(matches!(cli.command, Some(Cmd::DeleteBucketEncryption(_))));
}

#[test]
fn parses_get_bucket_cors() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-cors", "s3://bucket"])
        .expect("get-bucket-cors should parse");
    assert!(matches!(cli.command, Some(Cmd::GetBucketCors(_))));
}

#[test]
fn parses_put_bucket_cors() {
    let cli = Cli::try_parse_from(["s7cmd", "put-bucket-cors", "s3://bucket", "/tmp/cors.json"])
        .expect("put-bucket-cors should parse");
    assert!(matches!(cli.command, Some(Cmd::PutBucketCors(_))));
}

#[test]
fn parses_delete_bucket_cors() {
    let cli = Cli::try_parse_from(["s7cmd", "delete-bucket-cors", "s3://bucket"])
        .expect("delete-bucket-cors should parse");
    assert!(matches!(cli.command, Some(Cmd::DeleteBucketCors(_))));
}

#[test]
fn parses_get_public_access_block() {
    let cli = Cli::try_parse_from(["s7cmd", "get-public-access-block", "s3://bucket"])
        .expect("get-public-access-block should parse");
    assert!(matches!(cli.command, Some(Cmd::GetPublicAccessBlock(_))));
}

#[test]
fn parses_put_public_access_block() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-public-access-block",
        "s3://bucket",
        "/tmp/pab.json",
    ])
    .expect("put-public-access-block should parse");
    assert!(matches!(cli.command, Some(Cmd::PutPublicAccessBlock(_))));
}

#[test]
fn parses_delete_public_access_block() {
    let cli = Cli::try_parse_from(["s7cmd", "delete-public-access-block", "s3://bucket"])
        .expect("delete-public-access-block should parse");
    assert!(matches!(cli.command, Some(Cmd::DeletePublicAccessBlock(_))));
}

#[test]
fn parses_get_bucket_website() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-website", "s3://bucket"])
        .expect("get-bucket-website should parse");
    assert!(matches!(cli.command, Some(Cmd::GetBucketWebsite(_))));
}

#[test]
fn parses_put_bucket_website() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-bucket-website",
        "s3://bucket",
        "/tmp/web.json",
    ])
    .expect("put-bucket-website should parse");
    assert!(matches!(cli.command, Some(Cmd::PutBucketWebsite(_))));
}

#[test]
fn parses_delete_bucket_website() {
    let cli = Cli::try_parse_from(["s7cmd", "delete-bucket-website", "s3://bucket"])
        .expect("delete-bucket-website should parse");
    assert!(matches!(cli.command, Some(Cmd::DeleteBucketWebsite(_))));
}

#[test]
fn parses_get_bucket_logging() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-logging", "s3://bucket"])
        .expect("get-bucket-logging should parse");
    assert!(matches!(cli.command, Some(Cmd::GetBucketLogging(_))));
}

#[test]
fn parses_put_bucket_logging() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-bucket-logging",
        "s3://bucket",
        "/tmp/log.json",
    ])
    .expect("put-bucket-logging should parse");
    assert!(matches!(cli.command, Some(Cmd::PutBucketLogging(_))));
}

#[test]
fn parses_get_bucket_notification_configuration() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "get-bucket-notification-configuration",
        "s3://bucket",
    ])
    .expect("get-bucket-notification-configuration should parse");
    assert!(matches!(
        cli.command,
        Some(Cmd::GetBucketNotificationConfiguration(_))
    ));
}

#[test]
fn parses_put_bucket_notification_configuration() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-bucket-notification-configuration",
        "s3://bucket",
        "/tmp/notif.json",
    ])
    .expect("put-bucket-notification-configuration should parse");
    assert!(matches!(
        cli.command,
        Some(Cmd::PutBucketNotificationConfiguration(_))
    ));
}

#[test]
fn parses_get_bucket_replication() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-replication", "s3://bucket"])
        .expect("get-bucket-replication should parse");
    assert!(matches!(cli.command, Some(Cmd::GetBucketReplication(_))));
}

#[test]
fn parses_put_bucket_replication() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-bucket-replication",
        "s3://bucket",
        "/tmp/rep.json",
    ])
    .expect("put-bucket-replication should parse");
    assert!(matches!(cli.command, Some(Cmd::PutBucketReplication(_))));
}

#[test]
fn parses_delete_bucket_replication() {
    let cli = Cli::try_parse_from(["s7cmd", "delete-bucket-replication", "s3://bucket"])
        .expect("delete-bucket-replication should parse");
    assert!(matches!(cli.command, Some(Cmd::DeleteBucketReplication(_))));
}

#[test]
fn parses_get_bucket_accelerate_configuration() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "get-bucket-accelerate-configuration",
        "s3://bucket",
    ])
    .expect("get-bucket-accelerate-configuration should parse");
    assert!(matches!(
        cli.command,
        Some(Cmd::GetBucketAccelerateConfiguration(_))
    ));
}

#[test]
fn parses_put_bucket_accelerate_configuration() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-bucket-accelerate-configuration",
        "s3://bucket",
        "--enabled",
    ])
    .expect("put-bucket-accelerate-configuration should parse");
    assert!(matches!(
        cli.command,
        Some(Cmd::PutBucketAccelerateConfiguration(_))
    ));
}

#[test]
fn parses_get_bucket_request_payment() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-request-payment", "s3://bucket"])
        .expect("get-bucket-request-payment should parse");
    assert!(matches!(cli.command, Some(Cmd::GetBucketRequestPayment(_))));
}

#[test]
fn parses_put_bucket_request_payment() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "put-bucket-request-payment",
        "s3://bucket",
        "--requester",
    ])
    .expect("put-bucket-request-payment should parse");
    assert!(matches!(cli.command, Some(Cmd::PutBucketRequestPayment(_))));
}

#[test]
fn parses_get_bucket_policy_status() {
    let cli = Cli::try_parse_from(["s7cmd", "get-bucket-policy-status", "s3://bucket"])
        .expect("get-bucket-policy-status should parse");
    assert!(matches!(cli.command, Some(Cmd::GetBucketPolicyStatus(_))));
}

#[test]
fn parses_restore_object() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "restore-object",
        "s3://bucket/key",
        "--days",
        "1",
        "--tier",
        "Standard",
    ])
    .expect("restore-object should parse");
    assert!(matches!(cli.command, Some(Cmd::RestoreObject(_))));
}

#[test]
fn parses_presign() {
    let cli =
        Cli::try_parse_from(["s7cmd", "presign", "s3://bucket/key"]).expect("presign should parse");
    assert!(matches!(cli.command, Some(Cmd::Presign(_))));
}

#[test]
fn parses_presign_with_expires_in() {
    let cli = Cli::try_parse_from(["s7cmd", "presign", "s3://bucket/key", "--expires-in", "120"])
        .expect("presign --expires-in should parse");
    let Some(Cmd::Presign(args)) = cli.command else {
        panic!("expected Presign");
    };
    assert_eq!(args.expires_in, 120);
}

#[test]
fn parses_batch_run_with_stdin_dash() {
    let cli = Cli::try_parse_from(["s7cmd", "batch-run", "-"]).expect("batch-run - should parse");
    let Some(Cmd::BatchRun(args)) = cli.command else {
        panic!("expected BatchRun");
    };
    assert_eq!(args.script, "-");
}

#[test]
fn parses_batch_run_with_file_path() {
    let cli = Cli::try_parse_from(["s7cmd", "batch-run", "/tmp/script.txt"])
        .expect("batch-run <file> should parse");
    let Some(Cmd::BatchRun(args)) = cli.command else {
        panic!("expected BatchRun");
    };
    assert_eq!(args.script, "/tmp/script.txt");
}

#[test]
fn batch_run_requires_script_positional() {
    // Mirrors put-bucket-policy: omitting the required positional must
    // fail at parse time.
    let res = Cli::try_parse_from(["s7cmd", "batch-run"]);
    assert!(res.is_err(), "missing script positional must fail");
}

#[test]
fn parses_batch_run_with_parallel_streaming_continue() {
    let cli = Cli::try_parse_from([
        "s7cmd",
        "batch-run",
        "--parallel",
        "8",
        "--streaming",
        "--continue-on-error",
        "--no-summary",
        "-",
    ])
    .expect("batch-run with all flags should parse");
    let Some(Cmd::BatchRun(args)) = cli.command else {
        panic!("expected BatchRun");
    };
    assert_eq!(args.parallel, 8);
    assert!(args.streaming);
    assert!(args.continue_on_error);
    assert!(args.no_summary);
    assert_eq!(args.script, "-");
}

/// The per-subcommand --auto-complete-shell inherited from the upstream args
/// structs must be fully disarmed on every subcommand: hidden from help, no
/// long name (clap_complete ignores hide(true)), and no env source — an
/// exported AUTO_COMPLETE_SHELL would otherwise re-arm the hidden arg and
/// fire its clap side effects (default_value_if on sync/clean paths,
/// required_unless_present on util targets). Only the top-level
/// --auto-complete-shell remains user-visible.
#[test]
fn per_subcommand_auto_complete_shell_is_fully_disarmed() {
    let cmd = cli::cli_command();
    let mut seen = 0;
    for sub in cmd.get_subcommands() {
        for arg in sub.get_arguments() {
            if arg.get_id().as_str() == "auto_complete_shell" {
                seen += 1;
                assert!(
                    arg.is_hide_set(),
                    "{}: --auto-complete-shell not hidden",
                    sub.get_name()
                );
                assert!(
                    arg.get_long().is_none(),
                    "{}: --auto-complete-shell long name still set",
                    sub.get_name()
                );
                assert!(
                    arg.get_env().is_none(),
                    "{}: --auto-complete-shell env source still set",
                    sub.get_name()
                );
            }
        }
    }
    assert!(
        seen > 30,
        "expected the inherited flag on most subcommands, saw only {seen}"
    );
}
