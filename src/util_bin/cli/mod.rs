// Vendored from s3util-rs@1.0.0 (commit 4edffac51939d78b33aae9476ed61be9df1b35c0)
//   src/bin/s3util/cli/mod.rs
// Adjustments: stripped #[cfg(test)] mod tests; commented out per-subcommand
//              pub mod declarations (re-enabled per task as files land);
//              kept ExitStatus, EXIT_CODE_*, run_copy_phase, build_rate_limiter.
// Includes fixes ported from s3util-rs PR#25 (post-1.7.1):
//   - is_user_cancellation: a worker failure that cancels the pipeline token
//     is reported as a failure (exit 1), not a SIGINT cancellation (exit 130);
//   - extract_keys: a local target with a trailing '/' resolves to
//     <dir>/<basename> on every platform (adapted to 1.7.1's
//     fs_util::is_key_a_directory — has_trailing_separator lands later).

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Result, anyhow};
use aws_sdk_s3::types::RequestPayer;
use leaky_bucket::RateLimiter;
use tracing::{info, trace};

use s3util_rs::Config;
use s3util_rs::storage::Storage;
use s3util_rs::storage::StorageFactory;
use s3util_rs::storage::local::LocalStorageFactory;
use s3util_rs::storage::local::fs_util;
use s3util_rs::storage::s3::S3StorageFactory;
use s3util_rs::transfer::{TransferDirection, TransferOutcome, detect_direction};
use s3util_rs::types::StoragePath;
use s3util_rs::types::token::{PipelineCancellationToken, create_pipeline_cancellation_token};

pub mod ctrl_c_handler;
pub mod indicator;
pub mod tagging;
pub mod ui_config;

// Per-subcommand modules — uncomment as each is vendored in later tasks.
pub mod cp;
pub mod create_bucket;
pub mod delete_bucket;
pub mod delete_bucket_cors;
pub mod delete_bucket_encryption;
pub mod delete_bucket_lifecycle_configuration;
pub mod delete_bucket_policy;
pub mod delete_bucket_replication;
pub mod delete_bucket_tagging;
pub mod delete_bucket_website;
pub mod delete_object_annotation;
pub mod delete_object_tagging;
pub mod delete_public_access_block;
pub mod get_bucket_accelerate_configuration;
pub mod get_bucket_cors;
pub mod get_bucket_encryption;
pub mod get_bucket_lifecycle_configuration;
pub mod get_bucket_logging;
pub mod get_bucket_notification_configuration;
pub mod get_bucket_policy;
pub mod get_bucket_policy_status;
pub mod get_bucket_replication;
pub mod get_bucket_request_payment;
pub mod get_bucket_tagging;
pub mod get_bucket_versioning;
pub mod get_bucket_website;
pub mod get_object_annotation;
pub mod get_object_tagging;
pub mod get_public_access_block;
pub mod head_bucket;
pub mod head_object;
pub mod list_object_annotations;
pub mod mv;
pub mod presign;
pub mod put_bucket_accelerate_configuration;
pub mod put_bucket_cors;
pub mod put_bucket_encryption;
pub mod put_bucket_lifecycle_configuration;
pub mod put_bucket_logging;
pub mod put_bucket_notification_configuration;
pub mod put_bucket_policy;
pub mod put_bucket_replication;
pub mod put_bucket_request_payment;
pub mod put_bucket_tagging;
pub mod put_bucket_versioning;
pub mod put_bucket_website;
pub mod put_object_annotation;
pub mod put_object_tagging;
pub mod put_public_access_block;
pub mod rename;
pub mod restore_object;
pub mod rm;

pub use cp::run_cp;
pub use create_bucket::run_create_bucket;
pub use delete_bucket::run_delete_bucket;
pub use delete_bucket_cors::run_delete_bucket_cors;
pub use delete_bucket_encryption::run_delete_bucket_encryption;
pub use delete_bucket_lifecycle_configuration::run_delete_bucket_lifecycle_configuration;
pub use delete_bucket_policy::run_delete_bucket_policy;
pub use delete_bucket_replication::run_delete_bucket_replication;
pub use delete_bucket_tagging::run_delete_bucket_tagging;
pub use delete_bucket_website::run_delete_bucket_website;
pub use delete_object_annotation::run_delete_object_annotation;
pub use delete_object_tagging::run_delete_object_tagging;
pub use delete_public_access_block::run_delete_public_access_block;
pub use get_bucket_accelerate_configuration::run_get_bucket_accelerate_configuration;
pub use get_bucket_cors::run_get_bucket_cors;
pub use get_bucket_encryption::run_get_bucket_encryption;
pub use get_bucket_lifecycle_configuration::run_get_bucket_lifecycle_configuration;
pub use get_bucket_logging::run_get_bucket_logging;
pub use get_bucket_notification_configuration::run_get_bucket_notification_configuration;
pub use get_bucket_policy::run_get_bucket_policy;
pub use get_bucket_policy_status::run_get_bucket_policy_status;
pub use get_bucket_replication::run_get_bucket_replication;
pub use get_bucket_request_payment::run_get_bucket_request_payment;
pub use get_bucket_tagging::run_get_bucket_tagging;
pub use get_bucket_versioning::run_get_bucket_versioning;
pub use get_bucket_website::run_get_bucket_website;
pub use get_object_annotation::run_get_object_annotation;
pub use get_object_tagging::run_get_object_tagging;
pub use get_public_access_block::run_get_public_access_block;
pub use head_bucket::run_head_bucket;
pub use head_object::run_head_object;
pub use list_object_annotations::run_list_object_annotations;
pub use mv::run_mv;
pub use presign::run_presign;
pub use put_bucket_accelerate_configuration::run_put_bucket_accelerate_configuration;
pub use put_bucket_cors::run_put_bucket_cors;
pub use put_bucket_encryption::run_put_bucket_encryption;
pub use put_bucket_lifecycle_configuration::run_put_bucket_lifecycle_configuration;
pub use put_bucket_logging::run_put_bucket_logging;
pub use put_bucket_notification_configuration::run_put_bucket_notification_configuration;
pub use put_bucket_policy::run_put_bucket_policy;
pub use put_bucket_replication::run_put_bucket_replication;
pub use put_bucket_request_payment::run_put_bucket_request_payment;
pub use put_bucket_tagging::run_put_bucket_tagging;
pub use put_bucket_versioning::run_put_bucket_versioning;
pub use put_bucket_website::run_put_bucket_website;
pub use put_object_annotation::run_put_object_annotation;
pub use put_object_tagging::run_put_object_tagging;
pub use put_public_access_block::run_put_public_access_block;
pub use rename::run_rename;
pub use restore_object::run_restore_object;
pub use rm::run_rm;

// Default refill interval is 100ms (= 10 refills per second).
const REFILL_PER_INTERVAL_DIVIDER: usize = 10;

fn build_rate_limiter(config: &Config) -> Option<Arc<RateLimiter>> {
    config.rate_limit_bandwidth.map(|bandwidth| {
        let refill = bandwidth as usize / REFILL_PER_INTERVAL_DIVIDER;
        Arc::new(
            RateLimiter::builder()
                .max(bandwidth as usize)
                .initial(bandwidth as usize)
                .refill(refill)
                .fair(true)
                .build(),
        )
    })
}

#[derive(Debug)]
pub enum ExitStatus {
    Success,
    Warning,
    NotFound,
    Cancelled,
}

impl ExitStatus {
    pub fn code(&self) -> i32 {
        match self {
            ExitStatus::Success => EXIT_CODE_SUCCESS,
            ExitStatus::Warning => EXIT_CODE_WARNING,
            ExitStatus::NotFound => EXIT_CODE_NOT_FOUND,
            ExitStatus::Cancelled => EXIT_CODE_CANCELLED,
        }
    }
}

pub const EXIT_CODE_SUCCESS: i32 = 0;
pub const EXIT_CODE_ERROR: i32 = 1;
pub const EXIT_CODE_WARNING: i32 = 3;
// Returned by the head-* subcommands when the target does not exist
// (HeadBucket / HeadObject service error reports `is_not_found()`).
pub const EXIT_CODE_NOT_FOUND: i32 = 4;
// Standard Unix convention for processes terminated by SIGINT (128 + 2).
pub const EXIT_CODE_CANCELLED: i32 = 130;

/// Intermediate state produced by [`run_copy_phase`].
///
/// [`run_cp`] and [`run_mv`] translate this into an [`ExitStatus`] / cleanup
/// decision. The clone of the source `Storage` is kept so callers can reuse
/// the same factory-built instance for follow-up operations (e.g. `mv`'s
/// post-transfer delete) without rebuilding it.
pub struct CopyPhase {
    pub transfer_result: Result<TransferOutcome>,
    pub source_storage: Storage,
    pub source_key: String,
    pub cancellation_token: PipelineCancellationToken,
    pub cancelled: bool,
    pub has_warning: bool,
}

/// Run the copy pipeline (cancellation token, indicator, transfer dispatch,
/// teardown). Returns enough state for callers (`run_cp`, `run_mv`) to decide
/// what exit code to produce.
pub async fn run_copy_phase(config: Config) -> Result<CopyPhase> {
    let cancellation_token = create_pipeline_cancellation_token();

    // Determine transfer direction
    let (source_str, target_str) = get_path_strings(&config.source, &config.target);
    let direction = detect_direction(&source_str, &target_str)?;

    trace!(direction = ?direction, "detected transfer direction");

    check_local_source_not_directory(&config.source, &direction)?;

    // For cp, the full path is always passed as the key to get_object/put_object.
    // Storage instances are created with an empty base path so that key = full path.
    let (source_key, target_key) = extract_keys(&config)?;

    let resolved_target_display = format_target_path(&config.target, &target_key);

    // Dry-run short-circuit: log the would-do action and skip the transfer,
    // indicator, ctrl-c handler, and rate limiter.
    //
    // A real S3-to-S3 run also syncs the source object's annotations (that
    // logic lives in the library `transfer()` this command calls). To keep
    // --dry-run honest we mirror upstream and log each annotation a real run
    // would copy — which needs the real S3 source, so it is built here when
    // (and only when) the direction is S3-to-S3 and
    // --enable-sync-object-annotations is set. `log_dry_run_annotation_sync`
    // is otherwise a no-op and makes only a read-only ListObjectAnnotations
    // call; every other case keeps the cheap LocalStorage placeholder and
    // skips all I/O. run_mv's own dry-run guard returns before the source
    // delete, so whichever source_storage is handed back here is never
    // invoked for a mutation.
    if config.dry_run {
        info!(
            source = %source_str,
            target = %resolved_target_display,
            "[dry-run] would copy."
        );

        let (stats_sender, _stats_receiver) = async_channel::unbounded();

        let (transfer_result, source_storage) = if config.enable_sync_object_annotations
            && matches!(direction, TransferDirection::S3ToS3)
        {
            let source_request_payer = if config.source_request_payer {
                Some(RequestPayer::Requester)
            } else {
                None
            };
            let source = S3StorageFactory::create(
                config.clone(),
                empty_s3_storage_path(&config.source),
                cancellation_token.clone(),
                stats_sender,
                config.source_client_config.clone(),
                source_request_payer,
                None,
                Arc::new(AtomicBool::new(false)),
                None,
            )
            .await;
            let result = s3util_rs::transfer::s3_to_s3::log_dry_run_annotation_sync(
                &config,
                &source,
                &source_key,
            )
            .await
            .map(|()| TransferOutcome::default());
            (result, source)
        } else {
            let placeholder_source = LocalStorageFactory::create(
                config.clone(),
                empty_local_storage_path(),
                cancellation_token.clone(),
                stats_sender,
                None,
                None,
                None,
                Arc::new(AtomicBool::new(false)),
                None,
            )
            .await;
            (Ok(TransferOutcome::default()), placeholder_source)
        };

        return Ok(CopyPhase {
            transfer_result,
            source_storage,
            source_key,
            cancellation_token,
            cancelled: false,
            has_warning: false,
        });
    }

    ctrl_c_handler::spawn_ctrl_c_handler(cancellation_token.clone());

    let (stats_sender, stats_receiver) = async_channel::unbounded();

    let show_progress = ui_config::is_progress_indicator_needed(&config);
    let show_result = ui_config::is_show_result_needed(&config);
    let log_sync_summary = config.tracing_config.is_some();

    // Start indicator
    let indicator_handle = indicator::show_indicator(
        stats_receiver,
        show_progress,
        show_result,
        log_sync_summary,
        resolved_target_display,
        source_key.clone(),
        target_key.clone(),
    );

    let has_warning = Arc::new(AtomicBool::new(false));
    let rate_limit_bandwidth = build_rate_limiter(&config);

    // Each direction builds the source `Storage` it consumes (for transfer)
    // plus a sibling clone (`source_for_caller`) that survives so callers
    // such as `run_mv` can reuse it for follow-up operations. Stdio sources
    // never reach mv, but the type still requires *some* storage — a no-op
    // LocalStorage placeholder fills that slot.
    let (transfer_result, source_for_caller) = match direction {
        TransferDirection::LocalToS3 => {
            let target_request_payer = if config.target_request_payer {
                Some(RequestPayer::Requester)
            } else {
                None
            };

            let source = LocalStorageFactory::create(
                config.clone(),
                empty_local_storage_path(),
                cancellation_token.clone(),
                stats_sender.clone(),
                None,
                None,
                rate_limit_bandwidth.clone(),
                has_warning.clone(),
                None,
            )
            .await;
            let source_for_caller = dyn_clone::clone_box(&*source);

            let target = S3StorageFactory::create(
                config.clone(),
                empty_s3_storage_path(&config.target),
                cancellation_token.clone(),
                stats_sender.clone(),
                config.target_client_config.clone(),
                target_request_payer,
                rate_limit_bandwidth.clone(),
                has_warning.clone(),
                None,
            )
            .await;

            let result = s3util_rs::transfer::local_to_s3::transfer(
                &config,
                source,
                target,
                &source_key,
                &target_key,
                cancellation_token.clone(),
                stats_sender.clone(),
            )
            .await;
            (result, source_for_caller)
        }
        TransferDirection::S3ToLocal => {
            let source_request_payer = if config.source_request_payer {
                Some(RequestPayer::Requester)
            } else {
                None
            };

            let source = S3StorageFactory::create(
                config.clone(),
                empty_s3_storage_path(&config.source),
                cancellation_token.clone(),
                stats_sender.clone(),
                config.source_client_config.clone(),
                source_request_payer,
                rate_limit_bandwidth.clone(),
                has_warning.clone(),
                None,
            )
            .await;
            let source_for_caller = dyn_clone::clone_box(&*source);

            let target = LocalStorageFactory::create(
                config.clone(),
                empty_local_storage_path(),
                cancellation_token.clone(),
                stats_sender.clone(),
                None,
                None,
                rate_limit_bandwidth.clone(),
                has_warning.clone(),
                None,
            )
            .await;

            let result = s3util_rs::transfer::s3_to_local::transfer(
                &config,
                source,
                target,
                &source_key,
                &target_key,
                cancellation_token.clone(),
                stats_sender.clone(),
            )
            .await;
            (result, source_for_caller)
        }
        TransferDirection::S3ToS3 => {
            let source_request_payer = if config.source_request_payer {
                Some(RequestPayer::Requester)
            } else {
                None
            };
            let target_request_payer = if config.target_request_payer {
                Some(RequestPayer::Requester)
            } else {
                None
            };

            let source = S3StorageFactory::create(
                config.clone(),
                empty_s3_storage_path(&config.source),
                cancellation_token.clone(),
                stats_sender.clone(),
                config.source_client_config.clone(),
                source_request_payer,
                rate_limit_bandwidth.clone(),
                has_warning.clone(),
                None,
            )
            .await;
            let source_for_caller = dyn_clone::clone_box(&*source);

            let target = S3StorageFactory::create(
                config.clone(),
                empty_s3_storage_path(&config.target),
                cancellation_token.clone(),
                stats_sender.clone(),
                config.target_client_config.clone(),
                target_request_payer,
                rate_limit_bandwidth.clone(),
                has_warning.clone(),
                None,
            )
            .await;

            let result = s3util_rs::transfer::s3_to_s3::transfer(
                &config,
                source,
                target,
                &source_key,
                &target_key,
                cancellation_token.clone(),
                stats_sender.clone(),
            )
            .await;
            (result, source_for_caller)
        }
        TransferDirection::StdioToS3 => {
            let target_request_payer = if config.target_request_payer {
                Some(RequestPayer::Requester)
            } else {
                None
            };

            let target = S3StorageFactory::create(
                config.clone(),
                empty_s3_storage_path(&config.target),
                cancellation_token.clone(),
                stats_sender.clone(),
                config.target_client_config.clone(),
                target_request_payer,
                rate_limit_bandwidth.clone(),
                has_warning.clone(),
                None,
            )
            .await;

            // Stdio sources never reach run_mv (mv rejects stdio at config
            // validation). Use a placeholder LocalStorage so CopyPhase always
            // owns a valid Storage instance.
            let source_for_caller = LocalStorageFactory::create(
                config.clone(),
                empty_local_storage_path(),
                cancellation_token.clone(),
                stats_sender.clone(),
                None,
                None,
                rate_limit_bandwidth.clone(),
                has_warning.clone(),
                None,
            )
            .await;

            let result = s3util_rs::transfer::stdio_to_s3::transfer(
                &config,
                target,
                &target_key,
                tokio::io::stdin(),
                cancellation_token.clone(),
                stats_sender.clone(),
            )
            .await;
            (result, source_for_caller)
        }
        TransferDirection::S3ToStdio => {
            let source_request_payer = if config.source_request_payer {
                Some(RequestPayer::Requester)
            } else {
                None
            };

            let source = S3StorageFactory::create(
                config.clone(),
                empty_s3_storage_path(&config.source),
                cancellation_token.clone(),
                stats_sender.clone(),
                config.source_client_config.clone(),
                source_request_payer,
                rate_limit_bandwidth.clone(),
                has_warning.clone(),
                None,
            )
            .await;
            let source_for_caller = dyn_clone::clone_box(&*source);

            let result = s3util_rs::transfer::s3_to_stdio::transfer(
                &config,
                source,
                &source_key,
                tokio::io::stdout(),
                cancellation_token.clone(),
                stats_sender.clone(),
            )
            .await;
            (result, source_for_caller)
        }
    };

    // Send error stat if transfer failed, so indicator can suppress summary
    if transfer_result.is_err() {
        let _ = stats_sender
            .send(s3util_rs::types::SyncStatistics::SyncError { key: String::new() })
            .await;
    }

    // Close stats channel to signal indicator to finish
    stats_sender.close();

    // Wait for indicator to finish
    let _ = indicator_handle.await;

    let cancelled = is_user_cancellation(cancellation_token.is_cancelled(), &transfer_result);
    let has_warning = has_warning.load(std::sync::atomic::Ordering::SeqCst);

    Ok(CopyPhase {
        transfer_result,
        source_storage: source_for_caller,
        source_key,
        cancellation_token,
        cancelled,
        has_warning,
    })
}

/// Decide whether a finished copy phase should be reported to the user as a
/// cancellation (exit 130) rather than a failure (exit 1).
///
/// A cancelled token on its own does not mean SIGINT. The parallel s3-to-stdio
/// workers also cancel the pipeline token to stop their peers when a chunk GET
/// or a stdout write fails, so reading the token alone reported exit 130 for a
/// failed download and suppressed the error message entirely — while the same
/// failure on the serial path exited 1 with the cause logged.
///
/// The transfer result is the authoritative signal: a genuine SIGINT surfaces
/// as `S3syncError::Cancelled` (possibly wrapped in `.context()`), and anything
/// else is a real failure that must be reported as one.
fn is_user_cancellation(
    token_cancelled: bool,
    transfer_result: &Result<s3util_rs::transfer::TransferOutcome>,
) -> bool {
    token_cancelled
        && transfer_result
            .as_ref()
            .err()
            .is_none_or(s3util_rs::types::error::is_cancelled_error)
}

fn get_path_strings(source: &StoragePath, target: &StoragePath) -> (String, String) {
    let source_str = match source {
        StoragePath::S3 { bucket, prefix } => {
            if prefix.is_empty() {
                format!("s3://{}", bucket)
            } else {
                format!("s3://{}/{}", bucket, prefix)
            }
        }
        StoragePath::Local(path) => path.to_string_lossy().to_string(),
        StoragePath::Stdio => "-".to_string(),
    };
    let target_str = match target {
        StoragePath::S3 { bucket, prefix } => {
            if prefix.is_empty() {
                format!("s3://{}", bucket)
            } else {
                format!("s3://{}/{}", bucket, prefix)
            }
        }
        StoragePath::Local(path) => path.to_string_lossy().to_string(),
        StoragePath::Stdio => "-".to_string(),
    };
    (source_str, target_str)
}

/// Extract the full path as the key for each side.
/// For cp, the full path is always passed to get_object/put_object.
/// Storage instances are created with empty base paths.
pub(super) fn extract_keys(config: &Config) -> Result<(String, String)> {
    let source_key = match &config.source {
        StoragePath::S3 { prefix, .. } => {
            if prefix.is_empty() {
                return Err(anyhow!("source S3 key is required (e.g. s3://bucket/key)"));
            }
            prefix.clone()
        }
        StoragePath::Local(path) => path.to_string_lossy().to_string(),
        StoragePath::Stdio => String::new(),
    };
    let source_basename = std::path::Path::new(&source_key)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or(source_key.clone());

    let target_key = match &config.target {
        StoragePath::S3 { prefix, .. } => {
            // If target is empty or ends with '/', treat as directory prefix — append source basename
            if prefix.is_empty() || prefix.ends_with('/') {
                // With a stdin source there's no basename to derive, so the user must
                // spell the target key explicitly (e.g. `s3://bucket/key`).
                if source_basename.is_empty() {
                    return Err(anyhow!(
                        "target S3 key is required when source is stdin (e.g. s3://bucket/key)"
                    ));
                }
                format!("{prefix}{source_basename}")
            } else {
                prefix.clone()
            }
        }
        StoragePath::Local(path) => {
            let p = path.clone();
            // If target is a directory (existing dir or ends with separator),
            // append the source object's basename — like `aws s3 cp s3://bucket/key .`
            // The separator test must accept '/' on Windows too, so that `out/`
            // resolves to `out/<basename>` rather than staying the literal key
            // `out/`, which the storage layer would treat as a directory and
            // silently write nothing for. (Upstream calls
            // fs_util::has_trailing_separator, introduced after 1.7.1;
            // is_key_a_directory is the same trailing-separator test.)
            if p.is_dir() || fs_util::is_key_a_directory(&p.to_string_lossy()) {
                p.join(&source_basename).to_string_lossy().to_string()
            } else {
                p.to_string_lossy().to_string()
            }
        }
        StoragePath::Stdio => String::new(),
    };
    Ok((source_key, target_key))
}

/// Format the resolved target path for display.
fn format_target_path(target: &StoragePath, target_key: &str) -> String {
    match target {
        StoragePath::S3 { bucket, .. } => format!("s3://{bucket}/{target_key}"),
        StoragePath::Local(_) => target_key.to_string(),
        StoragePath::Stdio => "-".to_string(),
    }
}

/// Reject local source directories for `cp`.
///
/// LocalStorage::head_object returns a 0-byte success for directories (inherited
/// from s3sync's recursive-sync semantics). `s3util cp` is single-file only, so
/// without this guard a command like `s3util cp /tmp/ s3://bucket/` would silently
/// upload an empty object.
fn check_local_source_not_directory(
    source: &StoragePath,
    direction: &TransferDirection,
) -> Result<()> {
    if !matches!(direction, TransferDirection::LocalToS3) {
        return Ok(());
    }
    if let StoragePath::Local(path) = source
        && path.is_dir()
    {
        return Err(anyhow!(
            "source is a directory: {}. cp command copies a single file.",
            path.display()
        ));
    }
    Ok(())
}

/// Create a LocalStorage base path (empty — full path is passed as the key).
fn empty_local_storage_path() -> StoragePath {
    StoragePath::Local(".".into())
}

/// Create an S3Storage base path with empty prefix (full key is passed to operations).
fn empty_s3_storage_path(original: &StoragePath) -> StoragePath {
    match original {
        StoragePath::S3 { bucket, .. } => StoragePath::S3 {
            bucket: bucket.clone(),
            prefix: String::new(),
        },
        _ => unreachable!("expected S3 storage path"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use s3util_rs::config::args::{Commands, parse_from_args};

    use super::*;

    fn build_config(args: Vec<&str>) -> Config {
        let cli = parse_from_args(args).unwrap();
        let Commands::Cp(cp_args) = cli.command else {
            panic!("expected Cp variant");
        };
        Config::try_from(cp_args).unwrap()
    }

    /// Create a temp directory under the user's tmp dir using a UUID
    /// suffix. Returned path is removed at end of test via the Drop helper.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("s7cmd_unit_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create temp dir");
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

    /// Create a temporary file with a unique path, returns its path.
    /// The file persists until `remove_file` is called manually or test exits.
    fn temp_file_path() -> PathBuf {
        std::env::temp_dir().join(format!("s7cmd_unit_file_{}.dat", uuid::Uuid::new_v4()))
    }

    #[test]
    fn exit_status_codes() {
        assert_eq!(ExitStatus::Success.code(), EXIT_CODE_SUCCESS);
        assert_eq!(ExitStatus::Warning.code(), EXIT_CODE_WARNING);
        assert_eq!(ExitStatus::NotFound.code(), EXIT_CODE_NOT_FOUND);
        assert_eq!(ExitStatus::Cancelled.code(), EXIT_CODE_CANCELLED);
        assert_eq!(EXIT_CODE_SUCCESS, 0);
        assert_eq!(EXIT_CODE_ERROR, 1);
        assert_eq!(EXIT_CODE_WARNING, 3);
        assert_eq!(EXIT_CODE_NOT_FOUND, 4);
        assert_eq!(EXIT_CODE_CANCELLED, 130);
    }

    #[test]
    fn get_path_strings_formats_each_storage_kind() {
        let s3_with_prefix = StoragePath::S3 {
            bucket: "b".to_string(),
            prefix: "k/v".to_string(),
        };
        let s3_no_prefix = StoragePath::S3 {
            bucket: "b".to_string(),
            prefix: String::new(),
        };
        let local = StoragePath::Local(PathBuf::from("/tmp/x"));
        let stdio = StoragePath::Stdio;

        let (src, tgt) = get_path_strings(&s3_with_prefix, &local);
        assert_eq!(src, "s3://b/k/v");
        assert_eq!(tgt, "/tmp/x");

        let (src, tgt) = get_path_strings(&s3_no_prefix, &stdio);
        assert_eq!(src, "s3://b");
        assert_eq!(tgt, "-");

        let (src, tgt) = get_path_strings(&stdio, &s3_with_prefix);
        assert_eq!(src, "-");
        assert_eq!(tgt, "s3://b/k/v");

        // Cover the empty-prefix arm of target_str.
        let (src, tgt) = get_path_strings(&local, &s3_no_prefix);
        assert_eq!(src, "/tmp/x");
        assert_eq!(tgt, "s3://b");
    }

    #[test]
    fn format_target_path_for_each_storage_kind() {
        let s3 = StoragePath::S3 {
            bucket: "mybucket".to_string(),
            prefix: String::new(),
        };
        assert_eq!(format_target_path(&s3, "k/v.dat"), "s3://mybucket/k/v.dat");

        let local = StoragePath::Local(PathBuf::from("/x"));
        assert_eq!(format_target_path(&local, "ignored"), "ignored");

        assert_eq!(format_target_path(&StoragePath::Stdio, "ignored"), "-");
    }

    #[test]
    fn empty_local_storage_path_is_dot() {
        let StoragePath::Local(p) = empty_local_storage_path() else {
            panic!("expected Local");
        };
        assert_eq!(p, PathBuf::from("."));
    }

    #[test]
    fn empty_s3_storage_path_clears_prefix_keeps_bucket() {
        let original = StoragePath::S3 {
            bucket: "mybucket".to_string(),
            prefix: "some/key".to_string(),
        };
        let StoragePath::S3 { bucket, prefix } = empty_s3_storage_path(&original) else {
            panic!("expected S3");
        };
        assert_eq!(bucket, "mybucket");
        assert_eq!(prefix, "");
    }

    #[test]
    fn build_rate_limiter_returns_none_when_unset() {
        let config = build_config(vec!["s3util", "cp", "/tmp/a", "s3://b/k"]);
        assert!(config.rate_limit_bandwidth.is_none());
        assert!(build_rate_limiter(&config).is_none());
    }

    #[test]
    fn build_rate_limiter_returns_some_when_set() {
        let config = build_config(vec![
            "s3util",
            "cp",
            "--rate-limit-bandwidth",
            "10MiB",
            "/tmp/a",
            "s3://b/k",
        ]);
        assert!(config.rate_limit_bandwidth.is_some());
        assert!(build_rate_limiter(&config).is_some());
    }

    #[test]
    fn extract_keys_local_to_s3_object_target() {
        let config = build_config(vec!["s3util", "cp", "/tmp/source.dat", "s3://b/key.dat"]);
        let (src, tgt) = extract_keys(&config).unwrap();
        assert_eq!(src, "/tmp/source.dat");
        assert_eq!(tgt, "key.dat");
    }

    #[test]
    fn extract_keys_local_to_s3_bucket_only_uses_basename() {
        let config = build_config(vec!["s3util", "cp", "/tmp/source.dat", "s3://b"]);
        let (_, tgt) = extract_keys(&config).unwrap();
        assert_eq!(tgt, "source.dat");
    }

    #[test]
    fn extract_keys_local_to_s3_prefix_with_slash_appends_basename() {
        let config = build_config(vec!["s3util", "cp", "/tmp/source.dat", "s3://b/dir/"]);
        let (_, tgt) = extract_keys(&config).unwrap();
        assert_eq!(tgt, "dir/source.dat");
    }

    #[test]
    fn extract_keys_s3_to_local_with_no_source_key_errors() {
        let dir = TempDir::new();
        let target = dir.path().join("dst").to_string_lossy().to_string();
        let config = build_config(vec!["s3util", "cp", "s3://b", &target]);
        let err = extract_keys(&config).unwrap_err();
        assert!(err.to_string().contains("source S3 key is required"));
    }

    #[test]
    fn extract_keys_stdio_target_yields_empty_target_key() {
        let config = build_config(vec!["s3util", "cp", "s3://b/key", "-"]);
        let (src, tgt) = extract_keys(&config).unwrap();
        assert_eq!(src, "key");
        assert_eq!(tgt, "");
    }

    #[test]
    fn extract_keys_stdio_source_yields_empty_source_key() {
        let config = build_config(vec!["s3util", "cp", "-", "s3://b/key"]);
        let (src, tgt) = extract_keys(&config).unwrap();
        assert_eq!(src, "");
        assert_eq!(tgt, "key");
    }

    #[test]
    fn extract_keys_stdio_to_s3_bucket_only_errors() {
        let config = build_config(vec!["s3util", "cp", "-", "s3://b"]);
        let err = extract_keys(&config).unwrap_err();
        assert!(err.to_string().contains("target S3 key is required"));
    }

    #[test]
    fn extract_keys_stdio_to_s3_prefix_with_slash_errors() {
        let config = build_config(vec!["s3util", "cp", "-", "s3://b/dir/"]);
        let err = extract_keys(&config).unwrap_err();
        assert!(err.to_string().contains("target S3 key is required"));
    }

    #[test]
    fn check_local_source_not_directory_rejects_directory() {
        let tmp = TempDir::new();
        let source = StoragePath::Local(tmp.path().to_path_buf());
        let err =
            check_local_source_not_directory(&source, &TransferDirection::LocalToS3).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("source is a directory"), "message: {msg}");
    }

    #[test]
    fn check_local_source_not_directory_allows_file() {
        let path = temp_file_path();
        std::fs::write(&path, b"x").unwrap();
        let source = StoragePath::Local(path.clone());
        let result = check_local_source_not_directory(&source, &TransferDirection::LocalToS3);
        let _ = std::fs::remove_file(&path);
        result.unwrap();
    }

    #[test]
    fn check_local_source_not_directory_allows_nonexistent_path() {
        // head_object downstream turns this into a "file not found" error;
        // the directory guard should not pre-empt that path.
        let source = StoragePath::Local(PathBuf::from("/nonexistent/path/for/test"));
        check_local_source_not_directory(&source, &TransferDirection::LocalToS3).unwrap();
    }

    #[test]
    fn check_local_source_not_directory_skips_non_local_to_s3_direction() {
        let tmp = TempDir::new();
        let source = StoragePath::Local(tmp.path().to_path_buf());
        for direction in [
            TransferDirection::S3ToLocal,
            TransferDirection::S3ToS3,
            TransferDirection::StdioToS3,
            TransferDirection::S3ToStdio,
        ] {
            check_local_source_not_directory(&source, &direction).unwrap();
        }
    }

    #[test]
    fn extract_keys_s3_to_existing_local_directory_appends_basename() {
        let tmp = TempDir::new();
        let target_arg = tmp.path().to_string_lossy().to_string();
        let config = build_config(vec![
            "s3util",
            "cp",
            "s3://b/remote/file.dat",
            target_arg.as_str(),
        ]);
        let (_, tgt) = extract_keys(&config).unwrap();
        let expected = tmp.path().join("file.dat").to_string_lossy().to_string();
        assert_eq!(tgt, expected);
    }

    #[test]
    fn extract_keys_s3_to_local_path_with_trailing_separator_appends_basename() {
        let tmp = TempDir::new();
        let sep = std::path::MAIN_SEPARATOR;
        let target_arg = format!("{}{sep}", tmp.path().to_string_lossy());
        let config = build_config(vec![
            "s3util",
            "cp",
            "s3://b/remote/object.bin",
            target_arg.as_str(),
        ]);
        let (_, tgt) = extract_keys(&config).unwrap();
        assert!(tgt.ends_with("object.bin"), "target was: {tgt}");
    }

    /// Same as above but with a forward slash instead of the platform separator.
    /// Windows accepts `/` as a separator, and the storage layer treats a
    /// trailing `/` as a directory on every platform — so key resolution must
    /// append the basename here too. Leaving the key as the literal `out/` made
    /// the storage layer skip the write entirely and report success.
    #[test]
    fn extract_keys_s3_to_local_forward_slash_target_appends_basename() {
        let tmp = TempDir::new();
        let target_arg = format!("{}/", tmp.path().to_string_lossy());
        let config = build_config(vec![
            "s3util",
            "cp",
            "s3://b/remote/object.bin",
            target_arg.as_str(),
        ]);
        let (_, tgt) = extract_keys(&config).unwrap();
        assert!(
            tgt.ends_with("object.bin"),
            "forward-slash directory target must resolve to <dir>/<basename>, got: {tgt}"
        );
        assert!(
            !fs_util::is_key_a_directory(&tgt),
            "resolved key must not still look like a directory to the storage layer: {tgt}"
        );
    }

    /// Only a genuine SIGINT may be reported as a cancellation. The parallel
    /// s3-to-stdio workers cancel the same token to stop their peers after a
    /// chunk GET or stdout write fails; reporting that as exit 130 hid the
    /// error message and disagreed with the serial path, which exits 1.
    #[test]
    fn worker_failure_that_cancels_the_token_is_not_user_cancellation() {
        use anyhow::Context;
        use s3util_rs::transfer::TransferOutcome;
        use s3util_rs::types::error::S3syncError;

        // A real failure that also cancelled the token => a failure, not 130.
        let failed: Result<TransferOutcome> = Err(anyhow!("failed to download chunk: broken pipe"));
        assert!(
            !is_user_cancellation(true, &failed),
            "a transfer error must be reported as a failure even though the \
             worker cancelled the token to stop its peers"
        );

        // The same error wrapped in context must still be treated as a failure.
        let wrapped: Result<TransferOutcome> = Err(anyhow!("connection reset"))
            .context("failed to download chunk: s3://b/k bytes=0-8388607");
        assert!(!is_user_cancellation(true, &wrapped));

        // A genuine SIGINT surfaces as S3syncError::Cancelled => exit 130.
        let cancelled: Result<TransferOutcome> = Err(anyhow!(S3syncError::Cancelled));
        assert!(is_user_cancellation(true, &cancelled));

        // ...including when it is wrapped in context on the way up.
        let cancelled_wrapped: Result<TransferOutcome> =
            Err(anyhow!(S3syncError::Cancelled)).context("failed to download source object: k");
        assert!(is_user_cancellation(true, &cancelled_wrapped));

        // Cancelled token with a successful transfer (SIGINT observed after the
        // body completed) is still a cancellation.
        assert!(is_user_cancellation(true, &Ok(TransferOutcome::default())));

        // An un-cancelled token is never a cancellation, whatever the result.
        assert!(!is_user_cancellation(
            false,
            &Ok(TransferOutcome::default())
        ));
        let e: Result<TransferOutcome> = Err(anyhow!("some failure"));
        assert!(!is_user_cancellation(false, &e));
    }
}
