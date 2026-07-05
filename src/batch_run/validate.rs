//! Per-line validation rules (run after parsing, before execution).

use anyhow::{Result, anyhow};

use crate::cli::Cmd;
use s3util_rs::types::StoragePath;

/// Inspect a parsed `Cmd` and reject the cases that batch-run cannot
/// support. Errors include the line number for human-readable output.
pub fn validate(_line_no: usize, _raw: &str, cmd: &Cmd) -> Result<()> {
    if matches!(cmd, Cmd::BatchRun(_)) {
        return Err(anyhow!("nested batch-run is not allowed"));
    }
    if let Cmd::Cp(args) = cmd {
        let cp_config = s3util_rs::Config::try_from(args.clone()).map_err(|e| anyhow!("{e}"))?;
        if matches!(cp_config.source, StoragePath::Stdio)
            || matches!(cp_config.target, StoragePath::Stdio)
        {
            return Err(anyhow!(
                "stdin/stdout transfers are not allowed inside batch-run"
            ));
        }
    }
    if let Cmd::Mv(args) = cmd {
        let mv_config = s3util_rs::Config::try_from(args.clone()).map_err(|e| anyhow!("{e}"))?;
        if matches!(mv_config.source, StoragePath::Stdio)
            || matches!(mv_config.target, StoragePath::Stdio)
        {
            return Err(anyhow!(
                "stdin/stdout transfers are not allowed inside batch-run"
            ));
        }
    }
    reject_per_line_stdin_config(_line_no, _raw, cmd)?;
    reject_per_line_tracing(_line_no, _raw, cmd)?;
    Ok(())
}

/// The `put-bucket-*` family of subcommands takes a positional argument that
/// is either a file path or `-` to read the configuration JSON from stdin.
/// Inside batch-run, stdin (or the script file) is consumed by batch-run
/// itself, so `-` would clash with the script reader. Reject those lines at
/// validate time.
fn reject_per_line_stdin_config(_line_no: usize, _raw: &str, cmd: &Cmd) -> Result<()> {
    let stdin_arg = match cmd {
        Cmd::PutBucketPolicy(a) => a.policy.as_deref(),
        Cmd::PutBucketLifecycleConfiguration(a) => a.lifecycle_configuration.as_deref(),
        Cmd::PutBucketEncryption(a) => a.server_side_encryption_configuration.as_deref(),
        Cmd::PutBucketCors(a) => a.cors_configuration.as_deref(),
        Cmd::PutPublicAccessBlock(a) => a.public_access_block_configuration.as_deref(),
        Cmd::PutBucketWebsite(a) => a.website_configuration.as_deref(),
        Cmd::PutBucketLogging(a) => a.bucket_logging_status.as_deref(),
        Cmd::PutBucketNotificationConfiguration(a) => a.notification_configuration.as_deref(),
        Cmd::PutBucketReplication(a) => a.replication_configuration.as_deref(),
        // put-object-annotation reads its payload from `--annotation-payload -`
        // (stdin), which clashes with batch-run's script reader — same rule as
        // the put-bucket-* config bodies above.
        Cmd::PutObjectAnnotation(a) => a.annotation_payload.as_deref(),
        _ => return Ok(()),
    };
    if stdin_arg == Some("-") {
        return Err(anyhow!(
            "stdin/stdout transfers are not allowed inside batch-run"
        ));
    }
    Ok(())
}

/// Tracing flags belong to `batch-run` itself, not to per-line subcommands.
/// Reject any line that sets a tracing flag.
///
/// IMPORTANT: this function inspects the per-args tracing fields, not raw
/// argv strings (so it catches all clap-accepted spellings: long form,
/// `--flag=value`, etc.). The fields to check are:
///   - json_tracing
///   - aws_sdk_tracing
///   - span_events_tracing
///   - disable_color_tracing
///
/// Different subcommand args structs put these on different paths:
///   - Cp/Mv: `args.common.<field>`  (CommonTransferArgs)
///   - Rm and bucket/object metadata commands: `args.common.<field>`
///     (CommonClientArgs)
///   - Ls (s3ls_rs::CLIArgs): top-level public fields (no `.common`)
///   - Clean (s3rm_rs::CLIArgs): top-level public fields (no `.common`)
///   - Sync (s3sync::CLIArgs): top-level fields are private — read them
///     via `s3sync::Config::try_from(...)`.
fn reject_per_line_tracing(_line_no: usize, _raw: &str, cmd: &Cmd) -> Result<()> {
    // Accept the tracing-fields struct directly so callers pass `a.common`
    // for args with a `.common` wrapper, or `a` for flat args (Ls, Clean, Rename).
    macro_rules! check_common {
        ($c:expr) => {{
            if $c.json_tracing
                || $c.aws_sdk_tracing
                || $c.span_events_tracing
                || $c.disable_color_tracing
            {
                return Err(tracing_error());
            }
        }};
    }
    match cmd {
        // Already handled by validate() before this fn runs:
        Cmd::BatchRun(_) => {}

        // s3ls_rs / s3rm_rs CLIArgs and RenameArgs expose tracing fields as
        // public top-level fields (no `.common` wrapper).
        Cmd::Ls(boxed) => check_common!(boxed),
        Cmd::Clean(boxed) => check_common!(boxed),

        // s3sync::CLIArgs has private tracing fields, but exposes them
        // through `Config::try_from(...).tracing_config`. The conversion
        // can also fail for unrelated reasons (storage validation,
        // conflicting options, etc.); surface those at validate time so
        // read-all mode bails before any earlier line in the batch
        // executes — matching the Cp/Mv branches above. With the default
        // WarnLevel verbosity, `tracing_config` is `Some(_)`; with `-qq`
        // (silent) it is `None`, in which case the tracing flag values
        // cannot raise the level so the user-visible behaviour is
        // unaffected.
        Cmd::Sync(boxed) => {
            let cfg = s3sync::Config::try_from((**boxed).clone()).map_err(|e| anyhow!("{e}"))?;
            if let Some(t) = cfg.tracing_config
                && (t.json_tracing
                    || t.aws_sdk_tracing
                    || t.span_events_tracing
                    || t.disable_color_tracing)
            {
                return Err(tracing_error());
            }
        }

        // Cp/Mv: tracing fields live in args.common (CommonTransferArgs).
        Cmd::Cp(a) => check_common!(a.common),
        Cmd::Mv(a) => check_common!(a.common),
        // Rename: tracing fields are top-level (no .common wrapper).
        Cmd::Rename(a) => check_common!(a),

        // Rm + bucket/object subcommands: tracing fields in args.common (CommonClientArgs).
        Cmd::Rm(a) => check_common!(a.common),
        Cmd::CreateBucket(a) => check_common!(a.common),
        Cmd::DeleteBucket(a) => check_common!(a.common),
        Cmd::HeadBucket(a) => check_common!(a.common),
        Cmd::HeadObject(a) => check_common!(a.common),
        Cmd::GetObjectTagging(a) => check_common!(a.common),
        Cmd::PutObjectTagging(a) => check_common!(a.common),
        Cmd::DeleteObjectTagging(a) => check_common!(a.common),
        Cmd::GetObjectAnnotation(a) => check_common!(a.common),
        Cmd::PutObjectAnnotation(a) => check_common!(a.common),
        Cmd::DeleteObjectAnnotation(a) => check_common!(a.common),
        Cmd::ListObjectAnnotations(a) => check_common!(a.common),
        Cmd::GetBucketTagging(a) => check_common!(a.common),
        Cmd::PutBucketTagging(a) => check_common!(a.common),
        Cmd::DeleteBucketTagging(a) => check_common!(a.common),
        Cmd::GetBucketPolicy(a) => check_common!(a.common),
        Cmd::PutBucketPolicy(a) => check_common!(a.common),
        Cmd::DeleteBucketPolicy(a) => check_common!(a.common),
        Cmd::GetBucketVersioning(a) => check_common!(a.common),
        Cmd::PutBucketVersioning(a) => check_common!(a.common),
        Cmd::GetBucketLifecycleConfiguration(a) => check_common!(a.common),
        Cmd::PutBucketLifecycleConfiguration(a) => check_common!(a.common),
        Cmd::DeleteBucketLifecycleConfiguration(a) => check_common!(a.common),
        Cmd::GetBucketEncryption(a) => check_common!(a.common),
        Cmd::PutBucketEncryption(a) => check_common!(a.common),
        Cmd::DeleteBucketEncryption(a) => check_common!(a.common),
        Cmd::GetBucketCors(a) => check_common!(a.common),
        Cmd::PutBucketCors(a) => check_common!(a.common),
        Cmd::DeleteBucketCors(a) => check_common!(a.common),
        Cmd::GetPublicAccessBlock(a) => check_common!(a.common),
        Cmd::PutPublicAccessBlock(a) => check_common!(a.common),
        Cmd::DeletePublicAccessBlock(a) => check_common!(a.common),
        Cmd::GetBucketWebsite(a) => check_common!(a.common),
        Cmd::PutBucketWebsite(a) => check_common!(a.common),
        Cmd::DeleteBucketWebsite(a) => check_common!(a.common),
        Cmd::GetBucketLogging(a) => check_common!(a.common),
        Cmd::PutBucketLogging(a) => check_common!(a.common),
        Cmd::GetBucketNotificationConfiguration(a) => check_common!(a.common),
        Cmd::PutBucketNotificationConfiguration(a) => check_common!(a.common),
        Cmd::GetBucketReplication(a) => check_common!(a.common),
        Cmd::PutBucketReplication(a) => check_common!(a.common),
        Cmd::DeleteBucketReplication(a) => check_common!(a.common),
        Cmd::GetBucketAccelerateConfiguration(a) => check_common!(a.common),
        Cmd::PutBucketAccelerateConfiguration(a) => check_common!(a.common),
        Cmd::GetBucketRequestPayment(a) => check_common!(a.common),
        Cmd::PutBucketRequestPayment(a) => check_common!(a.common),
        Cmd::GetBucketPolicyStatus(a) => check_common!(a.common),
        Cmd::RestoreObject(a) => check_common!(a.common),
        Cmd::Presign(a) => check_common!(a.common),
    }
    Ok(())
}

fn tracing_error() -> anyhow::Error {
    anyhow!(
        "tracing flags are not allowed inside batch-run lines; \
         pass them to batch-run itself, e.g. `s7cmd batch-run --aws-sdk-tracing < cmds.txt`"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::Parser;

    fn parse_cmd(argv: &[&str]) -> Cmd {
        Cli::try_parse_from(argv).unwrap().command.unwrap()
    }

    #[test]
    fn rejects_nested_batch_run() {
        let cmd = parse_cmd(&["s7cmd", "batch-run", "-"]);
        let err = validate(7, "batch-run -", &cmd).unwrap_err();
        assert!(err.to_string().contains("nested batch-run"));
    }

    #[test]
    fn rejects_cp_with_stdio_target() {
        let cmd = parse_cmd(&["s7cmd", "cp", "s3://b/k", "-"]);
        let err = validate(3, "cp s3://b/k -", &cmd).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("stdin/stdout"), "msg: {msg}");
    }

    #[test]
    fn rejects_cp_with_stdio_source() {
        let cmd = parse_cmd(&["s7cmd", "cp", "-", "s3://b/k"]);
        let err = validate(1, "cp - s3://b/k", &cmd).unwrap_err();
        assert!(err.to_string().contains("stdin/stdout"));
    }

    #[test]
    fn allows_normal_cp() {
        let cmd = parse_cmd(&["s7cmd", "cp", "s3://b/a", "s3://b/b"]);
        validate(1, "cp s3://b/a s3://b/b", &cmd).unwrap();
    }

    #[test]
    fn rejects_mv_with_stdio_target() {
        let cmd = parse_cmd(&["s7cmd", "mv", "s3://b/k", "-"]);
        let err = validate(4, "mv s3://b/k -", &cmd).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("stdin/stdout"), "msg: {msg}");
    }

    #[test]
    fn rejects_mv_with_stdio_source() {
        let cmd = parse_cmd(&["s7cmd", "mv", "-", "s3://b/k"]);
        let err = validate(2, "mv - s3://b/k", &cmd).unwrap_err();
        assert!(err.to_string().contains("stdin/stdout"));
    }

    #[test]
    fn allows_normal_mv() {
        let cmd = parse_cmd(&["s7cmd", "mv", "s3://b/a", "s3://b/b"]);
        validate(1, "mv s3://b/a s3://b/b", &cmd).unwrap();
    }

    #[test]
    fn rejects_per_line_aws_sdk_tracing() {
        let cmd = parse_cmd(&["s7cmd", "head-bucket", "--aws-sdk-tracing", "s3://b"]);
        let err = validate(2, "head-bucket --aws-sdk-tracing s3://b", &cmd).unwrap_err();
        assert!(err.to_string().contains("tracing flags"));
    }

    #[test]
    fn rejects_per_line_json_tracing_on_cp() {
        // Use s3-to-s3 paths so `s3util_rs::Config::try_from` succeeds on
        // every platform (a local path like `/tmp/b` is rejected on Windows
        // before the tracing check runs).
        let cmd = parse_cmd(&["s7cmd", "cp", "--json-tracing", "s3://b/a", "s3://b/b"]);
        let err = validate(2, "cp --json-tracing s3://b/a s3://b/b", &cmd).unwrap_err();
        assert!(err.to_string().contains("tracing flags"));
    }

    /// Helper: parse argv, run `validate`, and assert the error mentions the
    /// stdin/stdout-transfers rule.
    fn assert_rejects_stdin_dash(argv: &[&str]) {
        let cmd = parse_cmd(argv);
        let raw = argv[1..].join(" ");
        let err = validate(1, &raw, &cmd).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("stdin/stdout"),
            "expected stdin/stdout error for argv {argv:?}, got: {msg}"
        );
    }

    #[test]
    fn rejects_put_bucket_commands_reading_stdin_dash() {
        // Each of these put-bucket-* commands takes a positional file path
        // (or `-` for stdin). In batch-run, stdin is the script source, so
        // `-` would clash. Reject at validate time.
        let cases = [
            "put-bucket-policy",
            "put-bucket-lifecycle-configuration",
            "put-bucket-encryption",
            "put-bucket-cors",
            "put-public-access-block",
            "put-bucket-website",
            "put-bucket-logging",
            "put-bucket-notification-configuration",
            "put-bucket-replication",
        ];
        for sub in cases {
            assert_rejects_stdin_dash(&["s7cmd", sub, "s3://b", "-"]);
        }
    }

    #[test]
    fn allows_put_bucket_policy_with_file_path() {
        // A regular file path positional must still pass validation.
        let cmd = parse_cmd(&["s7cmd", "put-bucket-policy", "s3://b", "/tmp/policy.json"]);
        validate(1, "put-bucket-policy s3://b /tmp/policy.json", &cmd).unwrap();
    }

    #[test]
    fn rejects_put_object_annotation_reading_stdin_dash() {
        // put-object-annotation reads its payload from stdin when
        // --annotation-payload is `-`, clashing with batch-run's script reader.
        assert_rejects_stdin_dash(&[
            "s7cmd",
            "put-object-annotation",
            "s3://b/k",
            "--annotation-name",
            "note",
            "--annotation-payload",
            "-",
        ]);
    }

    #[test]
    fn allows_put_object_annotation_with_file_payload() {
        // A regular file-path payload must still pass validation.
        let cmd = parse_cmd(&[
            "s7cmd",
            "put-object-annotation",
            "s3://b/k",
            "--annotation-name",
            "note",
            "--annotation-payload",
            "/tmp/payload.bin",
        ]);
        validate(1, "put-object-annotation ...", &cmd).unwrap();
    }

    #[test]
    fn allows_get_object_annotation_to_stdout_dash() {
        // The get-object-annotation outfile `-` writes to *stdout* (not read
        // from stdin), so it does not clash with batch-run's script reader.
        let cmd = parse_cmd(&[
            "s7cmd",
            "get-object-annotation",
            "s3://b/k",
            "-",
            "--annotation-name",
            "note",
        ]);
        validate(
            1,
            "get-object-annotation s3://b/k - --annotation-name note",
            &cmd,
        )
        .unwrap();
    }

    #[test]
    fn allows_create_bucket_without_tracing_flags() {
        let cmd = parse_cmd(&["s7cmd", "create-bucket", "s3://b"]);
        validate(1, "create-bucket s3://b", &cmd).unwrap();
    }

    // --- Per-variant tracing-rejection coverage ------------------------------
    //
    // Each entry below exercises a distinct `Cmd` arm in
    // `reject_per_line_tracing()`. Adding `--json-tracing` to the per-line
    // invocation must produce the "tracing flags" error.
    //
    // For variants whose `args` is parsed into a struct that flatten()s
    // `CommonClientArgs` (most of them), `--json-tracing` is accepted by clap
    // and the validator's `check_common!` macro returns the tracing error.
    // For `Ls` and `Clean` the flag lives directly on the args struct.
    // For `Sync` we go through `s3sync::Config::try_from(...)`.
    //
    // The per-line subcommand below is given just enough positional/flag
    // arguments to satisfy clap parsing — file-path positionals (e.g. policy
    // JSON) are not opened by the parser, so a bogus path is fine: we never
    // get past `validate()`'s tracing check for non-Cp/Mv variants.

    /// Helper: parse the given argv, run `validate`, and assert the error
    /// message mentions the tracing-flag rule.
    fn assert_rejects_tracing(argv: &[&str]) {
        let cmd = parse_cmd(argv);
        let raw = argv[1..].join(" ");
        let err = validate(1, &raw, &cmd).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("tracing flags"),
            "expected tracing-flags error for argv {argv:?}, got: {msg}"
        );
    }

    #[test]
    fn rejects_per_line_tracing_on_ls() {
        // `Ls` exposes tracing fields at the top level (no `.common`).
        assert_rejects_tracing(&["s7cmd", "ls", "--json-tracing", "s3://b"]);
    }

    #[test]
    fn rejects_per_line_tracing_on_clean() {
        // `Clean` exposes tracing fields at the top level (no `.common`).
        // `--force` is required by clean's clap parser.
        assert_rejects_tracing(&["s7cmd", "clean", "--force", "--json-tracing", "s3://b"]);
    }

    /// Each non-`json_tracing` flag on `Clean` is checked individually so
    /// the boolean-OR chain is exercised past the short-circuit on
    /// `json_tracing` — covering the per-flag arms in `reject_per_line_tracing`.
    #[test]
    fn rejects_per_line_tracing_on_clean_aws_sdk_tracing() {
        assert_rejects_tracing(&["s7cmd", "clean", "--force", "--aws-sdk-tracing", "s3://b"]);
    }

    #[test]
    fn rejects_per_line_tracing_on_clean_span_events_tracing() {
        assert_rejects_tracing(&[
            "s7cmd",
            "clean",
            "--force",
            "--span-events-tracing",
            "s3://b",
        ]);
    }

    #[test]
    fn rejects_per_line_tracing_on_clean_disable_color_tracing() {
        assert_rejects_tracing(&[
            "s7cmd",
            "clean",
            "--force",
            "--disable-color-tracing",
            "s3://b",
        ]);
    }

    #[test]
    fn rejects_per_line_tracing_on_sync() {
        // `Sync` reads tracing flags through `s3sync::Config::try_from(...)`.
        // A simple s3-to-s3 invocation is enough for `Config::try_from` to
        // succeed and surface the tracing flag.
        assert_rejects_tracing(&["s7cmd", "sync", "--json-tracing", "s3://b1", "s3://b2"]);
    }

    /// Each non-`json_tracing` flag on `Sync` is checked individually so
    /// the OR chain inside the `if let Some(t) = cfg.tracing_config &&
    /// (t.json_tracing || ...)` block is fully exercised.
    #[test]
    fn rejects_per_line_tracing_on_sync_aws_sdk_tracing() {
        assert_rejects_tracing(&["s7cmd", "sync", "--aws-sdk-tracing", "s3://b1", "s3://b2"]);
    }

    #[test]
    fn rejects_per_line_tracing_on_sync_span_events_tracing() {
        assert_rejects_tracing(&[
            "s7cmd",
            "sync",
            "--span-events-tracing",
            "s3://b1",
            "s3://b2",
        ]);
    }

    #[test]
    fn rejects_per_line_tracing_on_sync_disable_color_tracing() {
        assert_rejects_tracing(&[
            "s7cmd",
            "sync",
            "--disable-color-tracing",
            "s3://b1",
            "s3://b2",
        ]);
    }

    #[test]
    fn rejects_invalid_sync_config_at_validate_time() {
        // Local-to-local sync without `--allow-both-local-storage` is a
        // config error in s3sync. Validate must surface this so read-all
        // mode bails before earlier lines in the batch run.
        let cmd = parse_cmd(&["s7cmd", "sync", "/tmp/src", "/tmp/dst"]);
        let err = validate(4, "sync /tmp/src /tmp/dst", &cmd).unwrap_err();
        let msg = err.to_string();
        // The error is from s3sync; it must not be empty.
        assert!(
            !msg.is_empty(),
            "expected a non-empty validate error, got empty"
        );
    }

    #[test]
    fn rejects_per_line_tracing_on_mv() {
        // `Mv` uses `args.common` (CommonTransferArgs). `validate()` calls
        // `s3util_rs::Config::try_from` first, which succeeds for a plain
        // s3-to-s3 move, so we then reach the tracing check.
        assert_rejects_tracing(&["s7cmd", "mv", "--json-tracing", "s3://b/a", "s3://b/b"]);
    }

    /// Single parameterized test that covers every remaining Cmd variant
    /// whose tracing check goes through the `check_common!` macro
    /// (CommonClientArgs). Each tuple is `(argv, label)` — the label is
    /// only used in failure messages.
    #[test]
    fn rejects_per_line_tracing_for_all_common_client_variants() {
        // Variants whose required positionals are just an s3 bucket/key.
        let bucket_only = [
            "rm",
            "create-bucket",
            "delete-bucket",
            "head-bucket",
            "get-bucket-tagging",
            "delete-bucket-tagging",
            "get-bucket-policy",
            "delete-bucket-policy",
            "get-bucket-versioning",
            "get-bucket-lifecycle-configuration",
            "delete-bucket-lifecycle-configuration",
            "get-bucket-encryption",
            "delete-bucket-encryption",
            "get-bucket-cors",
            "delete-bucket-cors",
            "get-public-access-block",
            "delete-public-access-block",
            "get-bucket-website",
            "delete-bucket-website",
            "get-bucket-logging",
            "get-bucket-notification-configuration",
            "get-bucket-replication",
            "delete-bucket-replication",
            "get-bucket-accelerate-configuration",
            "get-bucket-request-payment",
            "get-bucket-policy-status",
        ];
        for sub in bucket_only {
            // `rm` requires a key, otherwise `s3://b` is enough.
            let target: &str = if sub == "rm" { "s3://b/k" } else { "s3://b" };
            assert_rejects_tracing(&["s7cmd", sub, "--json-tracing", target]);
        }

        // Variants whose required positionals are `<bucket>/<key>`.
        let object_keyed = [
            "head-object",
            "get-object-tagging",
            "delete-object-tagging",
            "restore-object",
            "presign",
        ];
        for sub in object_keyed {
            assert_rejects_tracing(&["s7cmd", sub, "--json-tracing", "s3://b/k"]);
        }

        // `put-bucket-versioning` requires one of `--enabled`/`--suspended`
        // at execution time, but clap parsing accepts neither (validation
        // happens via `validate_state_flag()` after parse). Supply
        // `--enabled` so the resulting Cmd is meaningful.
        assert_rejects_tracing(&[
            "s7cmd",
            "put-bucket-versioning",
            "--json-tracing",
            "--enabled",
            "s3://b",
        ]);

        // `put-bucket-accelerate-configuration` follows the same flag-style
        // pattern as `put-bucket-versioning` (mutually exclusive
        // `--enabled` / `--suspended`).
        assert_rejects_tracing(&[
            "s7cmd",
            "put-bucket-accelerate-configuration",
            "--json-tracing",
            "--enabled",
            "s3://b",
        ]);

        // `put-bucket-request-payment` uses mutually exclusive
        // `--requester` / `--bucket-owner` flags.
        assert_rejects_tracing(&[
            "s7cmd",
            "put-bucket-request-payment",
            "--json-tracing",
            "--requester",
            "s3://b",
        ]);

        // `put-bucket-tagging` and `put-object-tagging` take
        // `--tagging key=val` (not a file path) and the value must satisfy
        // `parse_tagging`.
        assert_rejects_tracing(&[
            "s7cmd",
            "put-bucket-tagging",
            "--json-tracing",
            "--tagging",
            "k=v",
            "s3://b",
        ]);
        assert_rejects_tracing(&[
            "s7cmd",
            "put-object-tagging",
            "--json-tracing",
            "--tagging",
            "k=v",
            "s3://b/k",
        ]);

        // The remaining `put-*` commands take a second positional that is a
        // file path; clap does not open the file at parse time, so any
        // string works. The validator's tracing check fires before any
        // file I/O.
        let put_with_file = [
            "put-bucket-policy",
            "put-bucket-lifecycle-configuration",
            "put-bucket-encryption",
            "put-bucket-cors",
            "put-public-access-block",
            "put-bucket-website",
            "put-bucket-logging",
            "put-bucket-notification-configuration",
            "put-bucket-replication",
        ];
        for sub in put_with_file {
            assert_rejects_tracing(&["s7cmd", sub, "--json-tracing", "s3://b", "/dev/null"]);
        }
    }

    /// Object-annotation subcommands each have a distinct required-arg shape,
    /// so they are exercised here rather than in the loops above. All four
    /// flatten `CommonClientArgs`, so `--json-tracing` must be rejected.
    #[test]
    fn rejects_per_line_tracing_for_annotation_variants() {
        // list-object-annotations: target only.
        assert_rejects_tracing(&[
            "s7cmd",
            "list-object-annotations",
            "--json-tracing",
            "s3://b/k",
        ]);
        // delete-object-annotation: target + --annotation-name.
        assert_rejects_tracing(&[
            "s7cmd",
            "delete-object-annotation",
            "--json-tracing",
            "s3://b/k",
            "--annotation-name",
            "note",
        ]);
        // get-object-annotation: target + outfile + --annotation-name. Outfile
        // `-` (stdout) is allowed inside batch-run, so it does not trip the
        // stdin rule that runs before the tracing check.
        assert_rejects_tracing(&[
            "s7cmd",
            "get-object-annotation",
            "--json-tracing",
            "s3://b/k",
            "-",
            "--annotation-name",
            "note",
        ]);
        // put-object-annotation: use a non-dash payload path so the stdin rule
        // (which runs before the tracing rule) does not fire first.
        assert_rejects_tracing(&[
            "s7cmd",
            "put-object-annotation",
            "--json-tracing",
            "--annotation-name",
            "note",
            "--annotation-payload",
            "/dev/null",
            "s3://b/k",
        ]);
    }
}
