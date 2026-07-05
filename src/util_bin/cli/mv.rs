// Vendored from s3util-rs@1.1.0
//   src/bin/s3util/cli/mv.rs
// Adjustments: stripped #[cfg(test)] mod tests; rewrote crate::cli → super

use anyhow::{Result, anyhow};
use tracing::{error, info};

use s3util_rs::Config;

use super::{CopyPhase, ExitStatus, run_copy_phase};

pub async fn run_mv(config: Config) -> Result<ExitStatus> {
    let phase = run_copy_phase(config.clone()).await?;
    apply_mv_decision_tree(config, phase).await
}

async fn apply_mv_decision_tree(config: Config, phase: CopyPhase) -> Result<ExitStatus> {
    // Gate 1: cancellation observed during/after transfer
    if phase.cancelled {
        return Ok(ExitStatus::Cancelled);
    }

    // Gate 2: transfer error
    let outcome = match phase.transfer_result {
        Ok(o) => o,
        Err(e) => {
            error!(error = format!("{e:#}"), "copy failed; source not deleted.");
            return Err(e);
        }
    };

    // Gate 3: verification warning
    if phase.has_warning && !config.no_fail_on_verify_error {
        let msg =
            "verification failed; source not deleted (use --no-fail-on-verify-error to override)";
        error!("{msg}");
        return Err(anyhow!(msg));
    }

    // Gate 4: defensive cancellation re-check (token may have flipped between
    // gate 1 and now if a SIGINT arrived while gate 2/3 were evaluating).
    if phase.cancellation_token.is_cancelled() {
        return Ok(ExitStatus::Cancelled);
    }

    // Resolve version-id: explicit user-supplied --source-version-id wins;
    // otherwise fall back to the value captured by the transfer.
    let version_id = config.version_id.clone().or(outcome.source_version_id);

    let version_id_for_log = version_id.clone().unwrap_or_default();

    if config.dry_run {
        info!(
            key = %phase.source_key,
            version_id = %version_id_for_log,
            "[dry-run] would delete source object."
        );
        return Ok(ExitStatus::Success);
    }

    match phase
        .source_storage
        .delete_object(&phase.source_key, version_id)
        .await
    {
        Ok(_) => {
            info!(
                key = %phase.source_key,
                version_id = %version_id_for_log,
                "Source delete completed."
            );
            Ok(ExitStatus::Success)
        }
        Err(e) => {
            error!(error = format!("{e:#}"), "source delete failed.");
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for `apply_mv_decision_tree`.
    //!
    //! Mirrors the upstream s3util-rs test pattern: synthesize a `CopyPhase`
    //! directly with a `FakeSourceStorage` so each gate can be exercised
    //! without going through `run_copy_phase` (which would require a real
    //! S3 client and real network I/O).
    //!
    //! The fake `Storage` impl records every `delete_object` call and returns
    //! a configurable result. Only `delete_object` and a handful of trivial
    //! getters are reachable from `apply_mv_decision_tree`; the other
    //! `StorageTrait` methods are `unimplemented!()` and are not invoked here.
    use super::*;
    use anyhow::anyhow;
    use async_channel::Sender;
    use async_trait::async_trait;
    use aws_sdk_s3::Client;
    use aws_sdk_s3::operation::delete_object::DeleteObjectOutput;
    use aws_sdk_s3::operation::get_object::GetObjectOutput;
    use aws_sdk_s3::operation::get_object_tagging::GetObjectTaggingOutput;
    use aws_sdk_s3::operation::head_object::HeadObjectOutput;
    use aws_sdk_s3::operation::put_object::PutObjectOutput;
    use aws_sdk_s3::operation::put_object_tagging::PutObjectTaggingOutput;
    use aws_sdk_s3::types::{ChecksumMode, ObjectPart, Tagging};
    use leaky_bucket::RateLimiter;
    use s3util_rs::config::TransferConfig;
    use s3util_rs::storage::{Storage, StorageTrait};
    use s3util_rs::transfer::TransferOutcome;
    use s3util_rs::types::token::{PipelineCancellationToken, create_pipeline_cancellation_token};
    use s3util_rs::types::{ObjectChecksum, SseCustomerKey, StoragePath, SyncStatistics};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    type DeleteCall = (String, Option<String>);

    #[derive(Clone, Debug)]
    enum DeleteResult {
        Ok,
        Err(String),
    }

    #[derive(Clone, Debug)]
    struct FakeSourceStorage {
        delete_calls: Arc<Mutex<Vec<DeleteCall>>>,
        delete_result: Arc<Mutex<DeleteResult>>,
    }

    impl FakeSourceStorage {
        fn new(delete_result: DeleteResult) -> Self {
            Self {
                delete_calls: Arc::new(Mutex::new(Vec::new())),
                delete_result: Arc::new(Mutex::new(delete_result)),
            }
        }
    }

    #[async_trait]
    impl StorageTrait for FakeSourceStorage {
        fn is_local_storage(&self) -> bool {
            false
        }
        fn is_express_onezone_storage(&self) -> bool {
            false
        }
        async fn get_object(
            &self,
            _key: &str,
            _version_id: Option<String>,
            _checksum_mode: Option<ChecksumMode>,
            _range: Option<String>,
            _sse_c: Option<String>,
            _sse_c_key: SseCustomerKey,
            _sse_c_key_md5: Option<String>,
        ) -> Result<GetObjectOutput> {
            unimplemented!()
        }
        async fn get_object_tagging(
            &self,
            _key: &str,
            _version_id: Option<String>,
        ) -> Result<GetObjectTaggingOutput> {
            unimplemented!()
        }
        async fn head_object(
            &self,
            _key: &str,
            _version_id: Option<String>,
            _checksum_mode: Option<ChecksumMode>,
            _range: Option<String>,
            _sse_c: Option<String>,
            _sse_c_key: SseCustomerKey,
            _sse_c_key_md5: Option<String>,
        ) -> Result<HeadObjectOutput> {
            unimplemented!()
        }
        async fn head_object_first_part(
            &self,
            _key: &str,
            _version_id: Option<String>,
            _checksum_mode: Option<ChecksumMode>,
            _sse_c: Option<String>,
            _sse_c_key: SseCustomerKey,
            _sse_c_key_md5: Option<String>,
        ) -> Result<HeadObjectOutput> {
            unimplemented!()
        }
        async fn get_object_parts(
            &self,
            _key: &str,
            _version_id: Option<String>,
            _sse_c: Option<String>,
            _sse_c_key: SseCustomerKey,
            _sse_c_key_md5: Option<String>,
        ) -> Result<Vec<ObjectPart>> {
            unimplemented!()
        }
        async fn get_object_parts_attributes(
            &self,
            _key: &str,
            _version_id: Option<String>,
            _max_parts: i32,
            _sse_c: Option<String>,
            _sse_c_key: SseCustomerKey,
            _sse_c_key_md5: Option<String>,
        ) -> Result<Vec<ObjectPart>> {
            unimplemented!()
        }
        async fn put_object(
            &self,
            _key: &str,
            _source: Storage,
            _source_key: &str,
            _source_size: u64,
            _source_additional_checksum: Option<String>,
            _get_object_output_first_chunk: GetObjectOutput,
            _tagging: Option<String>,
            _object_checksum: Option<ObjectChecksum>,
            _if_none_match: Option<String>,
        ) -> Result<PutObjectOutput> {
            unimplemented!()
        }
        async fn put_object_tagging(
            &self,
            _key: &str,
            _version_id: Option<String>,
            _tagging: Tagging,
        ) -> Result<PutObjectTaggingOutput> {
            unimplemented!()
        }
        async fn delete_object(
            &self,
            key: &str,
            version_id: Option<String>,
        ) -> Result<DeleteObjectOutput> {
            self.delete_calls
                .lock()
                .unwrap()
                .push((key.to_string(), version_id));
            match &*self.delete_result.lock().unwrap() {
                DeleteResult::Ok => Ok(DeleteObjectOutput::builder().build()),
                DeleteResult::Err(msg) => Err(anyhow!(msg.clone())),
            }
        }
        fn get_client(&self) -> Option<Arc<Client>> {
            None
        }
        fn get_stats_sender(&self) -> Sender<SyncStatistics> {
            async_channel::unbounded().0
        }
        async fn send_stats(&self, _stats: SyncStatistics) {}
        fn get_local_path(&self) -> PathBuf {
            PathBuf::new()
        }
        fn get_rate_limit_bandwidth(&self) -> Option<Arc<RateLimiter>> {
            None
        }
        fn generate_copy_source_key(&self, _key: &str, _version_id: Option<String>) -> String {
            unimplemented!()
        }
        fn set_warning(&self) {}
    }

    /// Build a minimal Config sufficient for `apply_mv_decision_tree`. Only
    /// `no_fail_on_verify_error`, `version_id`, and `dry_run` are read by
    /// the gate logic; every other field stays at a default.
    fn minimal_config() -> Config {
        Config {
            source: StoragePath::S3 {
                bucket: "src".to_string(),
                prefix: "k".to_string(),
            },
            target: StoragePath::S3 {
                bucket: "dst".to_string(),
                prefix: "k".to_string(),
            },
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
            sse_kms_key_id: s3util_rs::types::SseKmsKeyId { id: None },
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
            metadata: None,
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
            max_parallel_uploads: 1,
            rate_limit_bandwidth: None,
            version_id: None,
            is_stdio_source: false,
            is_stdio_target: false,
            no_fail_on_verify_error: false,
            skip_existing: false,
            dry_run: false,
            // Added in s3util-rs 1.6.0: object-annotation sync toggles for
            // cp/mv. Off by default, matching the upstream test fixture.
            enable_sync_object_annotations: false,
            disable_check_annotation_etag: false,
        }
    }

    fn synth_phase(
        transfer_result: Result<TransferOutcome>,
        has_warning: bool,
        cancelled: bool,
        fake_source: Storage,
        token: PipelineCancellationToken,
    ) -> CopyPhase {
        CopyPhase {
            transfer_result,
            source_storage: fake_source,
            source_key: "key".to_string(),
            cancellation_token: token,
            cancelled,
            has_warning,
        }
    }

    #[tokio::test]
    async fn gate_1_cancelled_returns_cancelled_no_delete() {
        let config = minimal_config();
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let calls = fake.delete_calls.clone();
        let token = create_pipeline_cancellation_token();
        let phase = synth_phase(
            Ok(TransferOutcome::default()),
            false,
            true,
            Box::new(fake),
            token,
        );

        let result = apply_mv_decision_tree(config, phase).await.unwrap();
        assert!(matches!(result, ExitStatus::Cancelled));
        assert_eq!(calls.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn gate_2_transfer_err_returns_err_no_delete() {
        let config = minimal_config();
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let calls = fake.delete_calls.clone();
        let token = create_pipeline_cancellation_token();
        let phase = synth_phase(
            Err(anyhow!("transfer failed")),
            false,
            false,
            Box::new(fake),
            token,
        );

        let err = apply_mv_decision_tree(config, phase).await.unwrap_err();
        assert!(err.to_string().contains("transfer failed"));
        assert_eq!(calls.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn gate_3_warning_without_flag_returns_err_no_delete() {
        let config = minimal_config();
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let calls = fake.delete_calls.clone();
        let token = create_pipeline_cancellation_token();
        let phase = synth_phase(
            Ok(TransferOutcome::default()),
            true,
            false,
            Box::new(fake),
            token,
        );

        let err = apply_mv_decision_tree(config, phase).await.unwrap_err();
        assert!(err.to_string().contains("verification failed"));
        assert_eq!(calls.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn gate_3_warning_with_flag_proceeds_to_delete() {
        let mut config = minimal_config();
        config.no_fail_on_verify_error = true;
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let calls = fake.delete_calls.clone();
        let token = create_pipeline_cancellation_token();
        let phase = synth_phase(
            Ok(TransferOutcome::default()),
            true,
            false,
            Box::new(fake),
            token,
        );

        let result = apply_mv_decision_tree(config, phase).await.unwrap();
        assert!(matches!(result, ExitStatus::Success));
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn gate_4_late_cancellation_returns_cancelled_no_delete() {
        let config = minimal_config();
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let calls = fake.delete_calls.clone();
        let token = create_pipeline_cancellation_token();
        // Trip the token but leave phase.cancelled = false so only gate 4 catches it.
        token.cancel();
        let phase = synth_phase(
            Ok(TransferOutcome::default()),
            false,
            false,
            Box::new(fake),
            token,
        );

        let result = apply_mv_decision_tree(config, phase).await.unwrap();
        assert!(matches!(result, ExitStatus::Cancelled));
        assert_eq!(calls.lock().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn delete_ok_returns_success() {
        let config = minimal_config();
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let calls = fake.delete_calls.clone();
        let token = create_pipeline_cancellation_token();
        let phase = synth_phase(
            Ok(TransferOutcome::default()),
            false,
            false,
            Box::new(fake),
            token,
        );

        let result = apply_mv_decision_tree(config, phase).await.unwrap();
        assert!(matches!(result, ExitStatus::Success));
        let recorded = calls.lock().unwrap().clone();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "key");
    }

    #[tokio::test]
    async fn delete_err_returns_err() {
        let config = minimal_config();
        let fake = FakeSourceStorage::new(DeleteResult::Err("delete boom".to_string()));
        let calls = fake.delete_calls.clone();
        let token = create_pipeline_cancellation_token();
        let phase = synth_phase(
            Ok(TransferOutcome::default()),
            false,
            false,
            Box::new(fake),
            token,
        );

        let err = apply_mv_decision_tree(config, phase).await.unwrap_err();
        assert!(err.to_string().contains("delete boom"));
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn version_id_explicit_wins_over_captured() {
        let mut config = minimal_config();
        config.version_id = Some("USER".to_string());
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let recorded = fake.delete_calls.clone();
        let token = create_pipeline_cancellation_token();
        let outcome = TransferOutcome {
            source_version_id: Some("CAPTURED".to_string()),
        };
        let phase = synth_phase(Ok(outcome), false, false, Box::new(fake), token);

        let _ = apply_mv_decision_tree(config, phase).await.unwrap();
        let calls = recorded.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, Some("USER".to_string()));
    }

    #[tokio::test]
    async fn version_id_falls_back_to_captured() {
        let config = minimal_config();
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let recorded = fake.delete_calls.clone();
        let token = create_pipeline_cancellation_token();
        let outcome = TransferOutcome {
            source_version_id: Some("CAPTURED".to_string()),
        };
        let phase = synth_phase(Ok(outcome), false, false, Box::new(fake), token);

        let _ = apply_mv_decision_tree(config, phase).await.unwrap();
        let calls = recorded.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, Some("CAPTURED".to_string()));
    }

    #[tokio::test]
    async fn dry_run_skips_source_delete() {
        let mut config = minimal_config();
        config.dry_run = true;
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let calls = fake.delete_calls.clone();
        let token = create_pipeline_cancellation_token();
        let phase = synth_phase(
            Ok(TransferOutcome::default()),
            false,
            false,
            Box::new(fake),
            token,
        );

        let result = apply_mv_decision_tree(config, phase).await.unwrap();
        assert!(matches!(result, ExitStatus::Success));
        assert_eq!(
            calls.lock().unwrap().len(),
            0,
            "dry-run must NOT call delete_object on source"
        );
    }

    // ── FakeSourceStorage coverage ──────────────────────────────────────
    //
    // These tests exercise the trait-impl bodies directly. The decision-tree
    // tests above only invoke `delete_object`, so without these tests every
    // other method (getters returning defaults, `unimplemented!` panics on the
    // unused methods) shows up as uncovered. Mirrored from the upstream
    // s3util-rs mv.rs test module.

    #[test]
    fn fake_source_storage_storage_type_flags_are_false() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        assert!(!fake.is_local_storage());
        assert!(!fake.is_express_onezone_storage());
    }

    #[test]
    fn fake_source_storage_simple_getters_return_defaults() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        assert!(fake.get_client().is_none());
        assert!(fake.get_rate_limit_bandwidth().is_none());
        assert_eq!(fake.get_local_path(), PathBuf::new());
        // set_warning is a no-op on the fake; just confirm it doesn't panic.
        fake.set_warning();
    }

    #[tokio::test]
    async fn fake_source_storage_send_stats_does_not_panic() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        fake.send_stats(SyncStatistics::SyncBytes(0)).await;
        // get_stats_sender returns a fresh unbounded channel — must not panic.
        let _sender = fake.get_stats_sender();
    }

    #[tokio::test]
    #[should_panic(expected = "not implemented")]
    async fn fake_source_storage_get_object_panics_unimplemented() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let _ = fake
            .get_object(
                "k",
                None,
                None,
                None,
                None,
                SseCustomerKey { key: None },
                None,
            )
            .await;
    }

    #[tokio::test]
    #[should_panic(expected = "not implemented")]
    async fn fake_source_storage_get_object_tagging_panics_unimplemented() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let _ = fake.get_object_tagging("k", None).await;
    }

    #[tokio::test]
    #[should_panic(expected = "not implemented")]
    async fn fake_source_storage_head_object_panics_unimplemented() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let _ = fake
            .head_object(
                "k",
                None,
                None,
                None,
                None,
                SseCustomerKey { key: None },
                None,
            )
            .await;
    }

    #[tokio::test]
    #[should_panic(expected = "not implemented")]
    async fn fake_source_storage_head_object_first_part_panics_unimplemented() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let _ = fake
            .head_object_first_part("k", None, None, None, SseCustomerKey { key: None }, None)
            .await;
    }

    #[tokio::test]
    #[should_panic(expected = "not implemented")]
    async fn fake_source_storage_get_object_parts_panics_unimplemented() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let _ = fake
            .get_object_parts("k", None, None, SseCustomerKey { key: None }, None)
            .await;
    }

    #[tokio::test]
    #[should_panic(expected = "not implemented")]
    async fn fake_source_storage_get_object_parts_attributes_panics_unimplemented() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let _ = fake
            .get_object_parts_attributes("k", None, 0, None, SseCustomerKey { key: None }, None)
            .await;
    }

    #[tokio::test]
    #[should_panic(expected = "not implemented")]
    async fn fake_source_storage_put_object_tagging_panics_unimplemented() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let tagging = Tagging::builder()
            .set_tag_set(Some(vec![]))
            .build()
            .unwrap();
        let _ = fake.put_object_tagging("k", None, tagging).await;
    }

    #[tokio::test]
    #[should_panic(expected = "not implemented")]
    async fn fake_source_storage_put_object_panics_unimplemented() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let inner = FakeSourceStorage::new(DeleteResult::Ok);
        let storage: Storage = Box::new(inner);
        let _ = fake
            .put_object(
                "k",
                storage,
                "src_k",
                0,
                None,
                GetObjectOutput::builder().build(),
                None,
                None,
                None,
            )
            .await;
    }

    #[test]
    #[should_panic(expected = "not implemented")]
    fn fake_source_storage_generate_copy_source_key_panics_unimplemented() {
        let fake = FakeSourceStorage::new(DeleteResult::Ok);
        let _ = fake.generate_copy_source_key("k", None);
    }
}
