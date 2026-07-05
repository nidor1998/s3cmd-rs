//! Single-command dispatch table.
//!
//! Each arm of `dispatch` is the body of what used to be a match arm
//! in `main.rs`. Returning `i32` instead of calling `std::process::exit`
//! lets `batch_run` invoke commands in a loop without killing the
//! process between them. Single-subcommand invocations route through
//! `main.rs`, which calls `dispatch` once and exits with its return.

use crate::batch_run;
use crate::clean_bin;
use crate::cli::Cmd;
use crate::ls_bin;
use crate::sync_bin;
use crate::util_bin;

// `dispatch` matches over ~50 `Cmd` variants. The compiled future is the
// max-size variant of every arm's inline state machine — for Cp / Mv / Sync
// the inner pipelines (`run_cp` → `run_copy_phase`, `run_mv`, s3sync `run`)
// add multi-megabyte futures of their own. On runners with ~2 MB test-thread
// stacks (GitHub Actions Linux / Windows in debug builds) that overflows.
// `Box::pin` moves those inner futures to the heap so dispatch's per-arm
// frame stays small.
pub async fn dispatch(cmd: Cmd) -> i32 {
    match cmd {
        Cmd::BatchRun(args) => batch_run::run(args).await,

        // sync_bin::cli::run already returns Result<i32> (Task 2).
        Cmd::Sync(boxed_args) => {
            let mut config = match s3sync::Config::try_from(*boxed_args) {
                Ok(c) => c,
                Err(msg) => {
                    let _ = clap::Error::raw(clap::error::ErrorKind::ValueValidation, msg).print();
                    return 2;
                }
            };
            // s3sync's main: when reporting sync status, force dry_run=true.
            if config.report_sync_status {
                config.dry_run = true;
            }
            if let Some(tc) = &config.tracing_config {
                sync_bin::tracing::init_tracing(tc);
            }
            tracing::trace!(target: "s3sync", "config = {:?}", config);
            match Box::pin(sync_bin::cli::run(config)).await {
                Ok(code) => code,
                Err(e) => {
                    tracing::error!(error = format!("{e:#}"));
                    1
                }
            }
        }

        // The Ls / Clean arms intentionally do NOT call
        // `*_bin::load_config_exit_if_err` — that helper calls
        // `clap::Error::exit()` (i.e. `std::process::exit(2)`) on bad
        // config, which would kill the whole batch-run process if any
        // single line had an invalid Ls/Clean config. Instead, mirror the
        // Cp/Mv/Sync pattern: print the clap-formatted error and return
        // exit code 2, letting batch-run record the failure and continue
        // (or bail with fail-fast).
        Cmd::Ls(boxed_args) => {
            let config = match s3ls_rs::config::Config::try_from(*boxed_args) {
                Ok(c) => c,
                Err(msg) => {
                    let _ = clap::Error::raw(clap::error::ErrorKind::ValueValidation, msg).print();
                    return 2;
                }
            };
            ls_bin::start_tracing_if_necessary(&config);
            tracing::trace!(target: "s3ls", "config = {:?}", config);
            match ls_bin::run(config).await {
                Ok(code) => code,
                Err(e) => {
                    tracing::error!(error = format!("{e:#}"));
                    1
                }
            }
        }

        Cmd::Clean(boxed_args) => {
            let config = match s3rm_rs::config::Config::try_from(*boxed_args) {
                Ok(c) => c,
                Err(msg) => {
                    let _ = clap::Error::raw(clap::error::ErrorKind::ValueValidation, msg).print();
                    return 2;
                }
            };
            clean_bin::start_tracing_if_necessary(&config);
            tracing::trace!(target: "s3rm", "config = {:?}", config);
            match clean_bin::run(config).await {
                Ok(code) => code,
                Err(e) => {
                    tracing::error!(error = format!("{e:#}"));
                    1
                }
            }
        }

        // Cp / Mv use `CommonTransferArgs`, which has no
        // `build_tracing_config()`. The tracing config is derived from
        // the converted `s3util_rs::Config`, so tracing init has to
        // happen here (after `try_from`) — `init_tracing_for` returns
        // early for these variants.
        Cmd::Cp(args) => {
            let config = match s3util_rs::Config::try_from(args) {
                Ok(c) => c,
                Err(msg) => {
                    let _ = clap::Error::raw(clap::error::ErrorKind::ValueValidation, msg).print();
                    return 2;
                }
            };
            start_tracing_if_necessary(&config);
            trace_config_summary(&config);
            match Box::pin(util_bin::cli::run_cp(config)).await {
                Ok(status) => status.code(),
                Err(e) => {
                    tracing::error!(error = format!("{e:#}"));
                    util_bin::cli::EXIT_CODE_ERROR
                }
            }
        }

        Cmd::Mv(args) => {
            let config = match s3util_rs::Config::try_from(args) {
                Ok(c) => c,
                Err(msg) => {
                    let _ = clap::Error::raw(clap::error::ErrorKind::ValueValidation, msg).print();
                    return 2;
                }
            };
            start_tracing_if_necessary(&config);
            trace_config_summary(&config);
            match Box::pin(util_bin::cli::run_mv(config)).await {
                Ok(status) => status.code(),
                Err(e) => {
                    tracing::error!(error = format!("{e:#}"));
                    util_bin::cli::EXIT_CODE_ERROR
                }
            }
        }

        Cmd::Rename(args) => {
            if let Err(e) = args.validate() {
                let _ = clap::Error::raw(clap::error::ErrorKind::ValueValidation, e).print();
                return 2;
            }
            let client_config = args.build_client_config();
            match util_bin::cli::run_rename(args, client_config).await {
                Ok(status) => status.code(),
                Err(e) => {
                    tracing::error!(error = format!("{e:#}"));
                    util_bin::cli::EXIT_CODE_ERROR
                }
            }
        }

        // Rm and bucket/object metadata commands all use the same shape:
        // build_client_config -> run with (args, config) -> map result to i32.
        // Use the helpers below to avoid 35 near-identical arms. Tracing init
        // for these variants is handled centrally by `main.rs::init_tracing_for`.
        Cmd::Rm(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_rm(args, client_config).await)
        }

        Cmd::CreateBucket(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_create_bucket(args, client_config).await)
        }
        Cmd::DeleteBucket(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_delete_bucket(args, client_config).await)
        }
        Cmd::HeadBucket(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_head_bucket(args, client_config).await)
        }

        Cmd::HeadObject(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_head_object(args, client_config).await)
        }
        Cmd::GetObjectTagging(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_object_tagging(args, client_config).await)
        }
        Cmd::PutObjectTagging(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_object_tagging(args, client_config).await)
        }
        Cmd::DeleteObjectTagging(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_delete_object_tagging(args, client_config).await)
        }

        // Object annotations. Unlike the tagging put/delete runners (which
        // return `()`), all four annotation runners return an `ExitStatus`
        // (they surface NotFound as exit 4), so every arm uses status_to_exit.
        //
        // The runners embed large SDK operation futures (and, for get, the
        // buffered payload + integrity-check machinery); like Cp/Mv/Sync
        // above, `Box::pin` moves them to the heap so dispatch's per-arm frame
        // stays small enough for the ~2 MB test-thread stacks used in CI.
        Cmd::GetObjectAnnotation(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(
                Box::pin(util_bin::cli::run_get_object_annotation(
                    args,
                    client_config,
                ))
                .await,
            )
        }
        Cmd::PutObjectAnnotation(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(
                Box::pin(util_bin::cli::run_put_object_annotation(
                    args,
                    client_config,
                ))
                .await,
            )
        }
        Cmd::DeleteObjectAnnotation(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(
                Box::pin(util_bin::cli::run_delete_object_annotation(
                    args,
                    client_config,
                ))
                .await,
            )
        }
        Cmd::ListObjectAnnotations(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(
                Box::pin(util_bin::cli::run_list_object_annotations(
                    args,
                    client_config,
                ))
                .await,
            )
        }

        Cmd::GetBucketTagging(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_bucket_tagging(args, client_config).await)
        }
        Cmd::PutBucketTagging(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_bucket_tagging(args, client_config).await)
        }
        Cmd::DeleteBucketTagging(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_delete_bucket_tagging(args, client_config).await)
        }

        Cmd::GetBucketPolicy(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_bucket_policy(args, client_config).await)
        }
        Cmd::PutBucketPolicy(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_bucket_policy(args, client_config).await)
        }
        Cmd::DeleteBucketPolicy(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_delete_bucket_policy(args, client_config).await)
        }

        Cmd::GetBucketVersioning(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_bucket_versioning(args, client_config).await)
        }
        Cmd::PutBucketVersioning(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_bucket_versioning(args, client_config).await)
        }

        Cmd::GetBucketLifecycleConfiguration(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(
                util_bin::cli::run_get_bucket_lifecycle_configuration(args, client_config).await,
            )
        }
        Cmd::PutBucketLifecycleConfiguration(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(
                util_bin::cli::run_put_bucket_lifecycle_configuration(args, client_config).await,
            )
        }
        Cmd::DeleteBucketLifecycleConfiguration(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(
                util_bin::cli::run_delete_bucket_lifecycle_configuration(args, client_config).await,
            )
        }

        Cmd::GetBucketEncryption(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_bucket_encryption(args, client_config).await)
        }
        Cmd::PutBucketEncryption(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_bucket_encryption(args, client_config).await)
        }
        Cmd::DeleteBucketEncryption(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_delete_bucket_encryption(args, client_config).await)
        }

        Cmd::GetBucketCors(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_bucket_cors(args, client_config).await)
        }
        Cmd::PutBucketCors(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_bucket_cors(args, client_config).await)
        }
        Cmd::DeleteBucketCors(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_delete_bucket_cors(args, client_config).await)
        }

        Cmd::GetPublicAccessBlock(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_public_access_block(args, client_config).await)
        }
        Cmd::PutPublicAccessBlock(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_public_access_block(args, client_config).await)
        }
        Cmd::DeletePublicAccessBlock(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_delete_public_access_block(args, client_config).await)
        }

        Cmd::GetBucketWebsite(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_bucket_website(args, client_config).await)
        }
        Cmd::PutBucketWebsite(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_bucket_website(args, client_config).await)
        }
        Cmd::DeleteBucketWebsite(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_delete_bucket_website(args, client_config).await)
        }

        Cmd::GetBucketLogging(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_bucket_logging(args, client_config).await)
        }
        Cmd::PutBucketLogging(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_bucket_logging(args, client_config).await)
        }

        Cmd::GetBucketNotificationConfiguration(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(
                util_bin::cli::run_get_bucket_notification_configuration(args, client_config).await,
            )
        }
        Cmd::PutBucketNotificationConfiguration(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(
                util_bin::cli::run_put_bucket_notification_configuration(args, client_config).await,
            )
        }

        Cmd::GetBucketReplication(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_bucket_replication(args, client_config).await)
        }
        Cmd::PutBucketReplication(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_bucket_replication(args, client_config).await)
        }
        Cmd::DeleteBucketReplication(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_delete_bucket_replication(args, client_config).await)
        }

        Cmd::GetBucketAccelerateConfiguration(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(
                util_bin::cli::run_get_bucket_accelerate_configuration(args, client_config).await,
            )
        }
        Cmd::PutBucketAccelerateConfiguration(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(
                util_bin::cli::run_put_bucket_accelerate_configuration(args, client_config).await,
            )
        }

        Cmd::GetBucketRequestPayment(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_bucket_request_payment(args, client_config).await)
        }
        Cmd::PutBucketRequestPayment(args) => {
            let client_config = args.common.build_client_config();
            unit_to_exit(util_bin::cli::run_put_bucket_request_payment(args, client_config).await)
        }

        Cmd::GetBucketPolicyStatus(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_get_bucket_policy_status(args, client_config).await)
        }

        Cmd::RestoreObject(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_restore_object(args, client_config).await)
        }

        Cmd::Presign(args) => {
            let client_config = args.common.build_client_config();
            status_to_exit(util_bin::cli::run_presign(args, client_config).await)
        }
    }
}

// ── Vendored from s3util-rs@1.1.0/src/bin/s3util/main.rs ─────────────
fn start_tracing_if_necessary(config: &s3util_rs::Config) -> bool {
    if config.tracing_config.is_none() {
        return false;
    }
    util_bin::tracing_init::init_tracing(config.tracing_config.as_ref().unwrap());
    true
}

// Trace only non-sensitive summary fields. Avoids `{:?}` on the full Config,
// which would risk leaking credentials or SSE-C key material if a future field
// is added without a redacting Debug impl.
fn trace_config_summary(config: &s3util_rs::Config) {
    tracing::trace!(
        target: "s3util",
        "config = {{ source: {:?}, target: {:?}, transfer_config: {:?}, server_side_copy: {}, version_id: {:?} }}",
        config.source,
        config.target,
        config.transfer_config,
        config.server_side_copy,
        config.version_id,
    );
}

fn status_to_exit(result: anyhow::Result<util_bin::cli::ExitStatus>) -> i32 {
    match result {
        Ok(status) => status.code(),
        Err(e) => {
            tracing::error!(error = format!("{e:#}"));
            util_bin::cli::EXIT_CODE_ERROR
        }
    }
}

fn unit_to_exit(result: anyhow::Result<()>) -> i32 {
    match result {
        Ok(()) => util_bin::cli::EXIT_CODE_SUCCESS,
        Err(e) => {
            tracing::error!(error = format!("{e:#}"));
            util_bin::cli::EXIT_CODE_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Cli;
    use clap::Parser;

    /// Parse top-level s7cmd args and return the unwrapped `Cmd`. Panics
    /// on parse failure or missing subcommand — every caller here passes
    /// known-good argv vectors.
    fn cmd_from(argv: &[&str]) -> Cmd {
        Cli::try_parse_from(argv)
            .expect("cli args should parse")
            .command
            .expect("subcommand should be present")
    }

    // ---------- Helper functions ----------

    #[test]
    fn status_to_exit_ok_success_returns_zero() {
        let code = status_to_exit(Ok(util_bin::cli::ExitStatus::Success));
        assert_eq!(code, util_bin::cli::EXIT_CODE_SUCCESS);
    }

    #[test]
    fn status_to_exit_ok_warning_returns_warning_code() {
        let code = status_to_exit(Ok(util_bin::cli::ExitStatus::Warning));
        assert_eq!(code, util_bin::cli::EXIT_CODE_WARNING);
    }

    #[test]
    fn status_to_exit_err_returns_error_code() {
        let code = status_to_exit(Err(anyhow::anyhow!("boom")));
        assert_eq!(code, util_bin::cli::EXIT_CODE_ERROR);
    }

    #[test]
    fn unit_to_exit_ok_returns_success() {
        let code = unit_to_exit(Ok(()));
        assert_eq!(code, util_bin::cli::EXIT_CODE_SUCCESS);
    }

    #[test]
    fn unit_to_exit_err_returns_error() {
        let code = unit_to_exit(Err(anyhow::anyhow!("boom")));
        assert_eq!(code, util_bin::cli::EXIT_CODE_ERROR);
    }

    #[test]
    fn start_tracing_if_necessary_returns_false_when_no_tracing_config() {
        // -qqq drops below all tracing levels → tracing_config is None.
        let cli = s3util_rs::config::args::Cli::try_parse_from([
            "s3util",
            "cp",
            "-qqq",
            "/tmp/src",
            "s3://bucket/key",
        ])
        .unwrap();
        let cp_args = match cli.command {
            s3util_rs::config::args::Commands::Cp(a) => a,
            _ => unreachable!(),
        };
        let config = s3util_rs::Config::try_from(cp_args).unwrap();
        assert!(config.tracing_config.is_none());
        assert!(!start_tracing_if_necessary(&config));
    }

    #[test]
    fn trace_config_summary_does_not_panic() {
        let cli = s3util_rs::config::args::Cli::try_parse_from([
            "s3util",
            "cp",
            "/tmp/src",
            "s3://bucket/key",
        ])
        .unwrap();
        let cp_args = match cli.command {
            s3util_rs::config::args::Commands::Cp(a) => a,
            _ => unreachable!(),
        };
        let config = s3util_rs::Config::try_from(cp_args).unwrap();
        // Just make sure it doesn't panic; output goes to tracing.
        trace_config_summary(&config);
    }

    // ---------- dispatch routing ----------
    //
    // Each `Cmd::*` arm under test is dispatched with a fake bucket. The
    // helpers above (`status_to_exit` / `unit_to_exit`) catch any error
    // from the inner runner and translate it to an exit code. We don't
    // assert on the specific exit code (it depends on whether the inner
    // call short-circuits or fails) — the assertion is only that
    // `dispatch` returns *some* `i32` without panicking, which means the
    // routing arm executed end-to-end.
    //
    // For mutating subcommands we pass `--dry-run` so the inner runner
    // returns Ok before any network call. For read-only subcommands we
    // point at a non-routable endpoint so the SDK fails quickly.

    const FAKE_ENDPOINT: &str = "http://127.0.0.1:1";
    const FAKE_BUCKET: &str = "s3://nonexistent-bucket-for-s7cmd-dispatch-tests";

    #[tokio::test]
    async fn dispatch_cp_invalid_args_returns_two() {
        // CommonTransferArgs requires either source or target to be S3 (or stdio).
        // Both /tmp paths → validate_storage_config returns Err → dispatch
        // prints clap error and returns 2.
        let cmd = cmd_from(&["s7cmd", "cp", "/tmp/local-src", "/tmp/local-dst"]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 2);
    }

    #[tokio::test]
    async fn dispatch_mv_invalid_args_returns_two() {
        let cmd = cmd_from(&["s7cmd", "mv", "/tmp/local-src", "/tmp/local-dst"]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 2);
    }

    #[tokio::test]
    async fn dispatch_rm_dry_run_succeeds() {
        let cmd = cmd_from(&["s7cmd", "rm", "--dry-run", &format!("{FAKE_BUCKET}/key")]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_create_bucket_dry_run_succeeds() {
        let cmd = cmd_from(&["s7cmd", "create-bucket", "--dry-run", FAKE_BUCKET]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_bucket_dry_run_succeeds() {
        let cmd = cmd_from(&["s7cmd", "delete-bucket", "--dry-run", FAKE_BUCKET]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_object_tagging_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "put-object-tagging",
            "--dry-run",
            "--tagging",
            "k=v",
            &format!("{FAKE_BUCKET}/key"),
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_object_tagging_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "delete-object-tagging",
            "--dry-run",
            &format!("{FAKE_BUCKET}/key"),
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_object_annotation_dry_run_succeeds() {
        // put-object-annotation reads the payload file before the dry-run
        // short-circuit, so seed a real temp file.
        let tmp = std::env::temp_dir().join(format!(
            "s7cmd_dispatch_annotation_{}.bin",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&tmp, b"payload").unwrap();
        let cmd = cmd_from(&[
            "s7cmd",
            "put-object-annotation",
            "--dry-run",
            "--annotation-name",
            "note",
            "--annotation-payload",
            tmp.to_str().unwrap(),
            &format!("{FAKE_BUCKET}/key"),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_object_annotation_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "delete-object-annotation",
            "--dry-run",
            "--annotation-name",
            "note",
            &format!("{FAKE_BUCKET}/key"),
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_object_annotation_against_fake_endpoint_returns_error() {
        let out = std::env::temp_dir().join(format!(
            "s7cmd_dispatch_annotation_out_{}.bin",
            uuid::Uuid::new_v4()
        ));
        let cmd = cmd_from(&[
            "s7cmd",
            "get-object-annotation",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            "--annotation-name",
            "note",
            &format!("{FAKE_BUCKET}/key"),
            out.to_str().unwrap(),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&out);
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_list_object_annotations_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "list-object-annotations",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            &format!("{FAKE_BUCKET}/key"),
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_tagging_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-tagging",
            "--dry-run",
            "--tagging",
            "k=v",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_bucket_tagging_dry_run_succeeds() {
        let cmd = cmd_from(&["s7cmd", "delete-bucket-tagging", "--dry-run", FAKE_BUCKET]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_versioning_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-versioning",
            "--dry-run",
            "--enabled",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_policy_dry_run_succeeds() {
        // put-bucket-policy reads its body file before the AWS call. Use a
        // temp file so dry-run can short-circuit successfully.
        let tmp = std::env::temp_dir().join(format!(
            "s7cmd_dispatch_policy_{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&tmp, b"{}").unwrap();
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-policy",
            "--dry-run",
            FAKE_BUCKET,
            tmp.to_str().unwrap(),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_bucket_policy_dry_run_succeeds() {
        let cmd = cmd_from(&["s7cmd", "delete-bucket-policy", "--dry-run", FAKE_BUCKET]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_lifecycle_configuration_dry_run_succeeds() {
        let tmp = std::env::temp_dir().join(format!(
            "s7cmd_dispatch_lifecycle_{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&tmp, b"{\"Rules\": []}").unwrap();
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-lifecycle-configuration",
            "--dry-run",
            FAKE_BUCKET,
            tmp.to_str().unwrap(),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_bucket_lifecycle_configuration_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "delete-bucket-lifecycle-configuration",
            "--dry-run",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_encryption_dry_run_succeeds() {
        let tmp =
            std::env::temp_dir().join(format!("s7cmd_dispatch_enc_{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, b"{\"Rules\":[]}").unwrap();
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-encryption",
            "--dry-run",
            FAKE_BUCKET,
            tmp.to_str().unwrap(),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_bucket_encryption_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "delete-bucket-encryption",
            "--dry-run",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_cors_dry_run_succeeds() {
        let tmp =
            std::env::temp_dir().join(format!("s7cmd_dispatch_cors_{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, b"{\"CORSRules\":[]}").unwrap();
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-cors",
            "--dry-run",
            FAKE_BUCKET,
            tmp.to_str().unwrap(),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_bucket_cors_dry_run_succeeds() {
        let cmd = cmd_from(&["s7cmd", "delete-bucket-cors", "--dry-run", FAKE_BUCKET]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_public_access_block_dry_run_succeeds() {
        let tmp =
            std::env::temp_dir().join(format!("s7cmd_dispatch_pab_{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, b"{}").unwrap();
        let cmd = cmd_from(&[
            "s7cmd",
            "put-public-access-block",
            "--dry-run",
            FAKE_BUCKET,
            tmp.to_str().unwrap(),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_public_access_block_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "delete-public-access-block",
            "--dry-run",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_website_dry_run_succeeds() {
        let tmp =
            std::env::temp_dir().join(format!("s7cmd_dispatch_web_{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, b"{}").unwrap();
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-website",
            "--dry-run",
            FAKE_BUCKET,
            tmp.to_str().unwrap(),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_bucket_website_dry_run_succeeds() {
        let cmd = cmd_from(&["s7cmd", "delete-bucket-website", "--dry-run", FAKE_BUCKET]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_logging_dry_run_succeeds() {
        let tmp =
            std::env::temp_dir().join(format!("s7cmd_dispatch_log_{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, b"{}").unwrap();
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-logging",
            "--dry-run",
            FAKE_BUCKET,
            tmp.to_str().unwrap(),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_notification_configuration_dry_run_succeeds() {
        let tmp = std::env::temp_dir().join(format!(
            "s7cmd_dispatch_notif_{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&tmp, b"{}").unwrap();
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-notification-configuration",
            "--dry-run",
            FAKE_BUCKET,
            tmp.to_str().unwrap(),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_replication_dry_run_succeeds() {
        let tmp =
            std::env::temp_dir().join(format!("s7cmd_dispatch_repl_{}.json", uuid::Uuid::new_v4()));
        std::fs::write(
            &tmp,
            br#"{"Role":"arn:aws:iam::000000000000:role/r","Rules":[]}"#,
        )
        .unwrap();
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-replication",
            "--dry-run",
            FAKE_BUCKET,
            tmp.to_str().unwrap(),
        ]);
        let code = dispatch(cmd).await;
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_delete_bucket_replication_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "delete-bucket-replication",
            "--dry-run",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_accelerate_configuration_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-accelerate-configuration",
            "--dry-run",
            "--enabled",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_put_bucket_request_payment_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "put-bucket-request-payment",
            "--dry-run",
            "--requester",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_restore_object_dry_run_succeeds() {
        let cmd = cmd_from(&[
            "s7cmd",
            "restore-object",
            "--dry-run",
            "--days",
            "1",
            "--tier",
            "Standard",
            &format!("{FAKE_BUCKET}/key"),
        ]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 0);
    }

    /// presign has no `--dry-run` (signing is local; nothing to defer). The
    /// bucket-only-path branch is the only one that fails before any SDK
    /// work, so it's the right shape for a routing-only assertion: clap
    /// accepts `s3://bucket`, `bucket_key()` rejects it post-parse, and
    /// `run_presign` surfaces the error → status_to_exit gives exit 1.
    #[tokio::test]
    async fn dispatch_presign_bucket_only_path_returns_one() {
        let cmd = cmd_from(&["s7cmd", "presign", FAKE_BUCKET]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 1);
    }

    // ---------- Get / Head arms (no --dry-run; aim a fake endpoint) ----------
    //
    // These hit the unreachable endpoint (127.0.0.1:1) so the SDK call
    // returns Err, exercising the `Err(e) => ...` branch under each arm.

    #[tokio::test]
    async fn dispatch_head_bucket_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "head-bucket",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    // Excluded on Windows: the GitHub Actions Windows runner overflows its
    // stack driving this fake-endpoint error path (deep AWS SDK / TLS
    // error-handling recursion), which aborts the whole test binary. The
    // behavior is unrelated to s7cmd logic and is covered on the other
    // platforms, so compile the test out on Windows entirely (rather than
    // `#[ignore]`, which `--include-ignored` could still trip).
    #[cfg(not(target_os = "windows"))]
    #[tokio::test]
    async fn dispatch_head_object_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "head-object",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            &format!("{FAKE_BUCKET}/key"),
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_object_tagging_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-object-tagging",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            &format!("{FAKE_BUCKET}/key"),
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_tagging_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-tagging",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_policy_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-policy",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_versioning_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-versioning",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_lifecycle_configuration_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-lifecycle-configuration",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_encryption_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-encryption",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_cors_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-cors",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_public_access_block_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-public-access-block",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_website_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-website",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_logging_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-logging",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_notification_configuration_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-notification-configuration",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_replication_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-replication",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_accelerate_configuration_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-accelerate-configuration",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_request_payment_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-request-payment",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_get_bucket_policy_status_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "get-bucket-policy-status",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    // ---------- Sync / Ls / Clean arms ----------
    //
    // These have separate runners (sync_bin / ls_bin / clean_bin) that
    // initialize their own tracing inside `dispatch`. We exercise the
    // routing with config shapes that fail fast.

    /// Temp dir helper — created with a UUID suffix and removed on Drop so
    /// each test gets a fresh, isolated directory.
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("s7cmd_dispatch_unit_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn dispatch_sync_local_to_local_runs() {
        // s3sync allows local→local with --allow-both-local-storage; this
        // exercises the Sync arm (config build + tracing init + run).
        let src = TempDir::new();
        let dst = TempDir::new();
        std::fs::write(src.path().join("a.txt"), b"x").unwrap();
        let src_str = format!("{}/", src.path().to_string_lossy());
        let dst_str = format!("{}/", dst.path().to_string_lossy());
        let cmd = cmd_from(&[
            "s7cmd",
            "sync",
            "--allow-both-local-storage",
            &src_str,
            &dst_str,
        ]);
        // Don't assert on exit code — pipeline may finish 0 or report
        // warnings depending on environment. Just routes through the arm.
        let _ = dispatch(cmd).await;
    }

    #[tokio::test]
    async fn dispatch_sync_invalid_config_returns_two() {
        // Two local paths without --allow-both-local-storage is a config
        // validation error in s3sync → dispatch returns 2.
        let cmd = cmd_from(&["s7cmd", "sync", "/tmp/src", "/tmp/dst"]);
        let code = dispatch(cmd).await;
        assert_eq!(code, 2);
    }

    /// `--report-sync-status` triggers the dispatch arm's
    /// `config.dry_run = true` override, regardless of what the user
    /// passed for `--dry-run`. Routing through the arm with a local
    /// source+target (so Config::try_from succeeds without S3) is
    /// enough — we don't assert on exit code.
    #[tokio::test]
    async fn dispatch_sync_with_report_sync_status_routes_through_arm() {
        let src = TempDir::new();
        let dst = TempDir::new();
        std::fs::write(src.path().join("a.txt"), b"x").unwrap();
        let src_str = format!("{}/", src.path().to_string_lossy());
        let dst_str = format!("{}/", dst.path().to_string_lossy());
        let cmd = cmd_from(&[
            "s7cmd",
            "sync",
            "--allow-both-local-storage",
            "--report-sync-status",
            &src_str,
            &dst_str,
        ]);
        let _ = dispatch(cmd).await;
    }

    #[tokio::test]
    async fn dispatch_ls_against_fake_endpoint_returns_error() {
        let cmd = cmd_from(&[
            "s7cmd",
            "ls",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let code = dispatch(cmd).await;
        assert_ne!(code, 0);
    }

    #[tokio::test]
    async fn dispatch_clean_routes_through_arm() {
        // Clean's runner catches the listing error and may map it to 0
        // (no objects to delete) or non-zero depending on SDK timing —
        // we only care that the routing arm executed end-to-end.
        let cmd = cmd_from(&[
            "s7cmd",
            "clean",
            "--force",
            "--target-endpoint-url",
            FAKE_ENDPOINT,
            "--target-region",
            "us-east-1",
            FAKE_BUCKET,
        ]);
        let _ = dispatch(cmd).await;
    }

    // NOTE: a `dispatch_batch_run_*` test would have to drive `dispatch(Cmd::BatchRun(_))`,
    // which calls `batch_run::run(...)` and reads from `tokio::io::stdin()`.
    // Under interactive `cargo test`, stdin is the terminal (not EOF), so
    // such a test hangs. Coverage of the BatchRun dispatch arm is provided
    // by the process-level integration tests in `tests/batch_run.rs`.
}
