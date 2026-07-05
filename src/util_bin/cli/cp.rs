// Vendored from s3util-rs@1.2.0
//   src/bin/s3util/cli/cp.rs
// Adjustments: rewrote crate::cli → super; #[cfg(test)] mod tests below
// covers the s3util-rs 1.2.0 --skip-existing branch (target_exists +
// run_cp short-circuit) — kept rather than stripped because s7cmd is the
// shipping surface for these branches.

use anyhow::Result;
use tracing::error;

use s3util_rs::Config;
use s3util_rs::storage::s3::api::{self, HeadError, HeadObjectOpts};
use s3util_rs::types::StoragePath;

use super::{ExitStatus, extract_keys, run_copy_phase};

pub async fn run_cp(config: Config) -> Result<ExitStatus> {
    if config.skip_existing && target_exists(&config).await? {
        let (_, target_key) = extract_keys(&config)?;
        let target_display = match &config.target {
            StoragePath::S3 { bucket, .. } => format!("s3://{bucket}/{target_key}"),
            StoragePath::Local(_) => target_key,
            StoragePath::Stdio => "-".to_string(),
        };
        if config.dry_run {
            tracing::info!(target = %target_display, "[dry-run] would skip: target exists.");
        } else {
            tracing::info!(target = %target_display, "Target exists; skipping copy.");
        }
        return Ok(ExitStatus::Success);
    }

    let phase = run_copy_phase(config).await?;
    if phase.cancelled {
        return Ok(ExitStatus::Cancelled);
    }
    if let Err(e) = phase.transfer_result {
        error!(error = format!("{e:#}"), "copy failed.");
        return Err(e);
    }
    if phase.has_warning {
        return Ok(ExitStatus::Warning);
    }
    Ok(ExitStatus::Success)
}

/// Check whether the target already exists. For S3 targets this issues
/// HeadObject; for local targets this is a filesystem exists check. Stdio
/// targets are rejected by `validate_storage_config`, so they cannot reach
/// here — the match arm is `unreachable!`.
async fn target_exists(config: &Config) -> anyhow::Result<bool> {
    let (_, target_key) = extract_keys(config)?;
    match &config.target {
        StoragePath::S3 { bucket, .. } => {
            let target_client_config = config.target_client_config.as_ref().ok_or_else(|| {
                anyhow::anyhow!("internal error: target_client_config missing for s3 target")
            })?;
            let client = target_client_config.create_client().await;
            let opts = HeadObjectOpts {
                version_id: None,
                sse_c: config.target_sse_c.clone(),
                sse_c_key: config.target_sse_c_key.key.clone(),
                sse_c_key_md5: config.target_sse_c_key_md5.clone(),
                enable_additional_checksum: false,
            };
            match api::head_object(&client, bucket, &target_key, opts).await {
                Ok(_) => Ok(true),
                Err(HeadError::NotFound) | Err(HeadError::BucketNotFound) => Ok(false),
                Err(HeadError::Other(e)) => Err(e),
            }
        }
        StoragePath::Local(_) => Ok(tokio::fs::try_exists(&target_key).await?),
        StoragePath::Stdio => {
            unreachable!("validate_storage_config rejects --skip-existing with stdout target")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s3util_rs::config::TransferConfig;
    use s3util_rs::types::{SseCustomerKey, SseKmsKeyId};
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Build a minimally-populated `Config` with a Local target at
    /// `target_path`. Mirrors the upstream helper in s3util-rs's cp tests.
    /// `skip_existing` is left to the caller so individual tests can flip
    /// it. `dry_run` defaults to false; tests that need it set it
    /// explicitly after construction.
    fn build_local_target_config(target_path: &str, skip_existing: bool) -> Config {
        Config {
            source: StoragePath::S3 {
                bucket: "src".to_string(),
                prefix: "k".to_string(),
            },
            target: StoragePath::Local(PathBuf::from(target_path)),
            show_progress: false,
            source_client_config: None,
            target_client_config: None,
            tracing_config: None,
            transfer_config: TransferConfig {
                multipart_threshold: 8 * 1024 * 1024,
                multipart_chunksize: 8 * 1024 * 1024,
                auto_chunksize: false,
            },
            disable_tagging: false,
            server_side_copy: false,
            no_guess_mime_type: false,
            disable_multipart_verify: false,
            disable_etag_verify: false,
            disable_additional_checksum_verify: false,
            storage_class: None,
            sse: None,
            sse_kms_key_id: SseKmsKeyId { id: None },
            source_sse_c: None,
            source_sse_c_key: SseCustomerKey { key: None },
            source_sse_c_key_md5: None,
            target_sse_c: None,
            target_sse_c_key: SseCustomerKey { key: None },
            target_sse_c_key_md5: None,
            canned_acl: None,
            additional_checksum_mode: None,
            additional_checksum_algorithm: None,
            cache_control: None,
            content_disposition: None,
            content_encoding: None,
            content_language: None,
            content_type: None,
            expires: None,
            metadata: Some(HashMap::new()),
            no_sync_system_metadata: false,
            no_sync_user_defined_metadata: false,
            website_redirect: None,
            tagging: None,
            put_last_modified_metadata: false,
            disable_payload_signing: false,
            disable_content_md5_header: false,
            full_object_checksum: false,
            source_accelerate: false,
            target_accelerate: false,
            source_request_payer: false,
            target_request_payer: false,
            if_none_match: false,
            disable_stalled_stream_protection: false,
            disable_express_one_zone_additional_checksum: false,
            max_parallel_uploads: 16,
            rate_limit_bandwidth: None,
            version_id: None,
            is_stdio_source: false,
            is_stdio_target: false,
            no_fail_on_verify_error: false,
            skip_existing,
            dry_run: false,
            // Added in s3util-rs 1.6.0: object-annotation sync toggles for
            // cp/mv. Off by default, matching the upstream test fixture.
            enable_sync_object_annotations: false,
            disable_check_annotation_etag: false,
        }
    }

    // ---- target_exists: Local branch ----

    #[tokio::test]
    async fn target_exists_local_returns_true_for_existing_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let config = build_local_target_config(&path, true);
        let exists = target_exists(&config).await.unwrap();
        assert!(
            exists,
            "expected existing tempfile to be reported as exists"
        );
    }

    #[tokio::test]
    async fn target_exists_local_returns_false_for_missing_path() {
        // Build a path that definitely does not exist within a real tempdir
        // so the parent directory is valid but the file itself is absent.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("definitely-missing-file.dat");
        let path = missing.to_string_lossy().to_string();
        let config = build_local_target_config(&path, true);
        let exists = target_exists(&config).await.unwrap();
        assert!(
            !exists,
            "expected missing path to be reported as not exists"
        );
    }

    // ---- run_cp: --skip-existing short-circuit ----
    //
    // These cover the s7cmd-shipped wrapper around target_exists. The
    // assertion is exit-status only — we cannot observe the tracing
    // emit without standing up a subscriber, but the
    // (s3util-rs side) tests already cover that the info!() lines fire,
    // and the cli_dry_run process-level test below covers it
    // end-to-end through the binary.

    #[tokio::test]
    async fn run_cp_skip_existing_local_target_exists_returns_success_without_transfer() {
        // If skip_existing did NOT short-circuit, run_copy_phase would be
        // invoked and panic / error against the placeholder S3 source
        // (no client config). Reaching ExitStatus::Success therefore
        // proves the short-circuit fired.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let config = build_local_target_config(&path, true);
        let status = run_cp(config).await.expect("run_cp must succeed");
        assert!(
            matches!(status, ExitStatus::Success),
            "expected Success, got {status:?}"
        );
    }

    #[tokio::test]
    async fn run_cp_skip_existing_local_target_exists_dry_run_returns_success() {
        // Dry-run + skip-existing + existing target: same short-circuit
        // arm but takes the [dry-run] branch of the info!() log.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_string_lossy().to_string();
        let mut config = build_local_target_config(&path, true);
        config.dry_run = true;
        let status = run_cp(config).await.expect("run_cp must succeed");
        assert!(matches!(status, ExitStatus::Success));
    }
}
