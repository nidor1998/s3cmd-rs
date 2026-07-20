#![cfg(e2e_test)]

mod common;

use common::{REGION, TestHelper, create_sized_file, create_temp_dir, generate_bucket_name};

use std::process::{Command, Stdio};

// Process exit codes are defined in src/util_bin/cli/mod.rs:
//   EXIT_CODE_SUCCESS   = 0
//   EXIT_CODE_ERROR     = 1
//   EXIT_CODE_WARNING   = 3
//   EXIT_CODE_CANCELLED = 130 (SIGINT/ctrl-c, covered in e2e_ctrl_c.rs)
//
// These tests invoke the actual binary as a subprocess and assert the
// process-level exit code. They are the only tests that exercise
// src/main.rs's exit-code mapping end to end.

const EXIT_CODE_SUCCESS: i32 = 0;
const EXIT_CODE_ERROR: i32 = 1;
const EXIT_CODE_WARNING: i32 = 3;

/// Exit code produced by clap when argument parsing fails.
///
/// This is not an exit code we set ourselves — it comes from clap's
/// `Error::exit` implementation. As of clap 4.x, every `ErrorKind`
/// variant except `DisplayHelp` / `DisplayVersion` /
/// `DisplayHelpOnMissingArgumentOrSubcommand` (which exit 0) is mapped
/// to exit code 2. This covers: unknown argument, invalid value,
/// missing required argument, value validation, subcommand errors, etc.
///
/// Two tests below assert this convention against two different
/// `ErrorKind` variants (value validation and unknown argument). If
/// both fail, clap has changed the exit-code convention globally —
/// update this constant and re-read clap's current error semantics.
/// If only one fails, the regression is in our own arg definition or
/// value parser rather than clap.
const EXIT_CODE_CLAP_ARG_ERROR: i32 = 2;

/// Successful local→S3 cp must exit 0.
#[tokio::test]
async fn exit_code_success_on_normal_cp() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let local_dir = create_temp_dir();
    let test_file = create_sized_file(&local_dir, "ok.bin", 1024);
    let target = format!("s3://{}/ok.bin", bucket);

    let status = std::process::Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--",
            "cp",
            "--target-profile",
            "s7cmd-e2e-test",
            test_file.to_str().unwrap(),
            &target,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();

    assert_eq!(
        status.code(),
        Some(EXIT_CODE_SUCCESS),
        "successful cp must exit 0, got: {status}"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// cp against a nonexistent bucket must exit 1 (run_cp returns Err).
#[tokio::test]
async fn exit_code_error_on_missing_bucket() {
    // No bucket creation — the target bucket is intentionally absent. Use a
    // unique name so we don't collide with an existing bucket.
    let bucket = format!("nonexistent-{}", uuid::Uuid::new_v4());
    let local_dir = create_temp_dir();
    let test_file = create_sized_file(&local_dir, "err.bin", 1024);
    let target = format!("s3://{}/err.bin", bucket);

    let status = std::process::Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--",
            "cp",
            "--target-profile",
            "s7cmd-e2e-test",
            test_file.to_str().unwrap(),
            &target,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();

    assert_eq!(
        status.code(),
        Some(EXIT_CODE_ERROR),
        "cp to nonexistent bucket must exit 1, got: {status}"
    );

    let _ = std::fs::remove_dir_all(&local_dir);
}

// ---------------------------------------------------------------
// CLI paths in src/main.rs that don't reach run_cp.
// These cover the early-return and validation branches that aren't
// exercised by lib unit tests (they need the actual binary).
// ---------------------------------------------------------------

// NOTE: s3util-rs invokes `cp --auto-complete-shell <SHELL>` here, but s7cmd
// hides the per-subcommand `--auto-complete-shell` flag (see src/cli.rs
// cli_command()) and exposes only the top-level form. The tests below are
// adapted to use `s7cmd --auto-complete-shell <SHELL>` directly — same
// behavior under test (clap_complete script generation, exit 0 short-circuit
// before any AWS/Config validation), just without the redundant `cp` arg.

/// `--auto-complete-shell bash` short-circuits before Config::try_from,
/// generates a shell completion script to stdout, and exits 0.
/// Covers the early-return branch in main.rs at the `auto_complete_shell`
/// check.
#[tokio::test]
async fn auto_complete_shell_emits_script_and_exits_zero() {
    let output = Command::new(env!("CARGO_BIN_EXE_s7cmd"))
        .args(["--auto-complete-shell", "bash"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(EXIT_CODE_SUCCESS),
        "--auto-complete-shell must exit 0, got: {}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // bash completion scripts contain `complete -F <funcname> s7cmd`.
    assert!(
        stdout.contains("s7cmd"),
        "expected bash completion output mentioning 's7cmd', got first 200 chars: {}",
        stdout.chars().take(200).collect::<String>()
    );
}

/// `--auto-complete-shell zsh` generates a zsh completion script to stdout
/// and exits 0. Asserts on the stable `#compdef s7cmd` anchor that
/// `clap_complete`'s zsh generator emits at the top of its script.
#[tokio::test]
async fn auto_complete_shell_zsh() {
    let output = Command::new(env!("CARGO_BIN_EXE_s7cmd"))
        .args(["--auto-complete-shell", "zsh"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(EXIT_CODE_SUCCESS),
        "--auto-complete-shell zsh must exit 0, got: {}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("#compdef s7cmd"),
        "expected zsh completion output containing '#compdef s7cmd', got first 200 chars: {}",
        stdout.chars().take(200).collect::<String>()
    );
}

/// `--auto-complete-shell fish` generates a fish completion script to
/// stdout and exits 0. Asserts on fish's `complete -c <program>` line
/// convention.
#[tokio::test]
async fn auto_complete_shell_fish() {
    let output = Command::new(env!("CARGO_BIN_EXE_s7cmd"))
        .args(["--auto-complete-shell", "fish"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(EXIT_CODE_SUCCESS),
        "--auto-complete-shell fish must exit 0, got: {}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("complete -c s7cmd"),
        "expected fish completion output containing 'complete -c s7cmd', got first 200 chars: {}",
        stdout.chars().take(200).collect::<String>()
    );
}

/// Invalid `--multipart-threshold` value (below the 5 MiB minimum) is
/// rejected by our value parser, which raises a clap error and exits
/// via `clap::Error::exit`. Exercises clap's `ValueValidation` branch.
///
/// Asserts exactly `EXIT_CODE_CLAP_ARG_ERROR` (2) so that any drift
/// in clap's exit-code convention surfaces as a test failure. Paired
/// with `unknown_flag_exits_with_clap_arg_error` below, which hits a
/// different `ErrorKind` — see the `EXIT_CODE_CLAP_ARG_ERROR` doc
/// comment for how to interpret single vs. paired failures.
#[tokio::test]
async fn invalid_multipart_threshold_exits_with_clap_error() {
    let local_dir = create_temp_dir();
    let test_file = create_sized_file(&local_dir, "x.bin", 64);

    let status = std::process::Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--",
            "cp",
            // 1KiB is below the documented 5 MiB minimum → value parser rejects.
            "--multipart-threshold",
            "1KiB",
            test_file.to_str().unwrap(),
            "s3://any-bucket/key",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();

    assert_eq!(
        status.code(),
        Some(EXIT_CODE_CLAP_ARG_ERROR),
        "invalid --multipart-threshold must exit with clap's arg-error code ({EXIT_CODE_CLAP_ARG_ERROR}), got: {status}"
    );

    let _ = std::fs::remove_dir_all(&local_dir);
}

/// An unknown CLI flag triggers clap's `UnknownArgument` branch,
/// which calls `clap::Error::exit` and terminates the process.
///
/// Asserts exactly `EXIT_CODE_CLAP_ARG_ERROR` (2). Together with
/// `invalid_multipart_threshold_exits_with_clap_error` above, this
/// triangulates clap's convention from two different `ErrorKind`
/// variants — see the `EXIT_CODE_CLAP_ARG_ERROR` doc comment.
#[tokio::test]
async fn unknown_flag_exits_with_clap_arg_error() {
    let status = std::process::Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--",
            "cp",
            "--this-flag-does-not-exist",
            "local.txt",
            "s3://any-bucket/key",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();

    assert_eq!(
        status.code(),
        Some(EXIT_CODE_CLAP_ARG_ERROR),
        "unknown flag must exit with clap's arg-error code ({EXIT_CODE_CLAP_ARG_ERROR}), got: {status}"
    );
}

/// A cp that produces a warning (no errors) must exit 3.
///
/// Mirrors the trigger used by `local_to_s3_multipart_e_tag_ng` /
/// `s3_to_local_multipart_e_tag_ng` in `tests/e2e_integrity_check.rs`:
/// upload a 9 MiB file with `--multipart-chunksize=5MiB`, then download
/// without specifying chunksize — the local recompute uses the default
/// 8 MiB and the resulting ETag won't match the source's stored ETag,
/// causing the cp to emit a sync_warning and exit 3.
#[tokio::test]
async fn exit_code_warning_on_etag_mismatch_after_chunksize_change() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let local_dir = create_temp_dir();
    let upload_file = create_sized_file(&local_dir, "warn.bin", 9 * 1024 * 1024);
    let s3_path = format!("s3://{}/warn.bin", bucket);

    // Step 1: upload with non-default chunksize so the stored ETag is built
    // from 5 MiB parts.
    let upload_status = std::process::Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--",
            "cp",
            "--target-profile",
            "s7cmd-e2e-test",
            "--multipart-threshold",
            "5MiB",
            "--multipart-chunksize",
            "5MiB",
            upload_file.to_str().unwrap(),
            &s3_path,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(
        upload_status.code(),
        Some(EXIT_CODE_SUCCESS),
        "warning-test setup upload must succeed first, got: {upload_status}"
    );

    // Step 2: download without chunksize override. Local ETag recompute will
    // use defaults and won't match the stored multipart ETag → warning.
    let dl_file = local_dir.join("warn_dl.bin");
    let dl_status = std::process::Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "--",
            "cp",
            "--source-profile",
            "s7cmd-e2e-test",
            &s3_path,
            dl_file.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();

    assert_eq!(
        dl_status.code(),
        Some(EXIT_CODE_WARNING),
        "ETag mismatch from chunksize change must exit 3, got: {dl_status}"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}
