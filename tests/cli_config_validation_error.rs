//! Process-level test: invalid args that survive clap's own parsing but fail
//! `Config::try_from` must be re-raised through clap's error machinery so the
//! user sees the validation message on stderr and the process exits non-zero.
//!
//! Covers `src/main.rs` — the `Err(error_message)` arm
//! of `match Config::try_from(cp_args)`.
//!
//! Doesn't require AWS: the error fires before any S3 call.

use std::process::{Command, Stdio};

#[test]
fn both_local_paths_exit_non_zero_with_validation_message_on_stderr() {
    let bin = env!("CARGO_BIN_EXE_s7cmd");

    // Two local paths are valid per clap's per-arg value_parser (check_storage_path)
    // but rejected by Config::try_from's check_both_local guard. That error is
    // re-wrapped as clap::ErrorKind::ValueValidation and printed by .exit().
    let output = Command::new(bin)
        .args(["cp", "/tmp/s7cmd_e2e_src", "/tmp/s7cmd_e2e_dst"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to spawn s7cmd binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !output.status.success(),
        "both-local-paths invocation must exit non-zero.\n\
         status: {:?}\n--- stderr ---\n{stderr}\n--- stdout ---\n{stdout}",
        output.status.code()
    );

    assert!(
        stderr.contains("source and target cannot both be local paths"),
        "expected the check_both_local validation message on stderr.\n\
         --- stderr ---\n{stderr}"
    );

    assert!(
        stderr.ends_with('\n'),
        "validation message must end with a newline so the shell prompt \
         starts on its own line.\n--- stderr ---\n{stderr:?}"
    );
}

/// Run `s7cmd <args>` and assert the config-validation contract shared by
/// every dispatch arm that routes through `print_config_error`: exit code 2,
/// the expected message on stderr, and the message terminated by exactly one
/// newline (clap::Error::raw appends none on its own — without the
/// normalization in `print_config_error` the shell prompt would land on the
/// same line as the error text).
fn assert_validation_error_ends_with_one_newline(args: &[&str], expected_msg: &str) {
    let bin = env!("CARGO_BIN_EXE_s7cmd");

    let output = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to spawn s7cmd binary");

    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(2),
        "s7cmd {args:?} must exit 2 on config validation failure.\n\
         --- stderr ---\n{stderr}"
    );
    assert!(
        stderr.contains(expected_msg),
        "expected {expected_msg:?} on stderr for s7cmd {args:?}.\n\
         --- stderr ---\n{stderr}"
    );
    assert!(
        stderr.ends_with('\n') && !stderr.ends_with("\n\n"),
        "validation message for s7cmd {args:?} must end with exactly one \
         newline.\n--- stderr ---\n{stderr:?}"
    );
}

// One test per dispatch arm that prints config-validation failures through
// `print_config_error` (ls, clean, cp, mv, rename, sync). Each drives an
// invocation that survives clap parsing but fails the library-side config
// validation, pinning the exit-2 + single-trailing-newline contract.

#[test]
fn ls_recursive_without_path_prints_newline_terminated_error() {
    // Bucket-listing mode (no path) rejects --recursive in
    // s3ls_rs Config::try_from.
    assert_validation_error_ends_with_one_newline(
        &["ls", "-r"],
        "--recursive is not valid for bucket listing",
    );
}

#[test]
fn clean_rate_limit_below_batch_size_prints_newline_terminated_error() {
    // s3rm_rs Config::try_from rejects --rate-limit-objects below the
    // batch size (default 200).
    assert_validation_error_ends_with_one_newline(
        &["clean", "--rate-limit-objects", "100", "s3://bucket"],
        "must be greater than or equal to --batch-size",
    );
}

#[test]
fn cp_source_url_with_trailing_slash_prints_newline_terminated_error() {
    // The trailing-'/' source message is one of the s3util-rs validation
    // strings without a hand-embedded '\n' (fixed bin-side upstream in
    // s3util-rs 1.10.1; s7cmd normalizes in print_config_error).
    assert_validation_error_ends_with_one_newline(
        &["cp", "s3://bucket/dir/", "/tmp/"],
        "object, not a prefix",
    );
}

#[test]
fn mv_source_url_with_trailing_slash_prints_newline_terminated_error() {
    assert_validation_error_ends_with_one_newline(
        &["mv", "s3://bucket/dir/", "/tmp/"],
        "object, not a prefix",
    );
}

#[test]
fn rename_on_general_purpose_bucket_prints_newline_terminated_error() {
    // rename requires an S3 Express One Zone bucket; args.validate()
    // rejects a general-purpose bucket name.
    assert_validation_error_ends_with_one_newline(
        &["rename", "s3://bucket/a", "s3://bucket/b"],
        "only supported on S3 Express One Zone buckets",
    );
}

#[test]
fn sync_missing_local_source_prints_newline_terminated_error() {
    // A nonexistent local source fails s3sync Config::try_from before
    // any S3 call.
    let missing_src = std::env::temp_dir().join(format!(
        "s7cmd_validation_missing_src_{}",
        uuid::Uuid::new_v4()
    ));
    let missing_dst = std::env::temp_dir().join(format!(
        "s7cmd_validation_missing_dst_{}",
        uuid::Uuid::new_v4()
    ));
    assert_validation_error_ends_with_one_newline(
        &[
            "sync",
            missing_src.to_str().unwrap(),
            missing_dst.to_str().unwrap(),
        ],
        "source file/directory not found",
    );
}

#[test]
fn source_no_sign_request_env_var_triggers_conflict_at_parse_time() {
    // Regression guard for the `env` attribute on `--source-no-sign-request`.
    //
    // Rather than mutate the test process's env (which races with parallel
    // tests that parse CpArgs), we isolate the env var to a child `s7cmd`
    // invocation. If clap reads SOURCE_NO_SIGN_REQUEST, combining it with
    // --source-profile will trip the `conflicts_with_all` at parse time and
    // the command exits non-zero with "cannot be used with" on stderr.
    // If clap ever silently drops the env binding, --source-profile alone
    // would be accepted and the command would proceed — a regression we
    // want to catch.
    let bin = env!("CARGO_BIN_EXE_s7cmd");

    let output = Command::new(bin)
        .args([
            "cp",
            "s3://b/k",
            "/tmp/out",
            "--source-profile",
            "myprofile",
        ])
        .env("SOURCE_NO_SIGN_REQUEST", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to spawn s7cmd binary");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !output.status.success(),
        "SOURCE_NO_SIGN_REQUEST + --source-profile must exit non-zero.\n\
         status: {:?}\n--- stderr ---\n{stderr}\n--- stdout ---\n{stdout}",
        output.status.code()
    );

    assert!(
        stderr.contains("cannot be used with"),
        "expected clap conflict message on stderr.\n\
         --- stderr ---\n{stderr}"
    );
}
