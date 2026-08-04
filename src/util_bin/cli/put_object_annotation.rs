// Vendored from s3util-rs@1.6.0
//   src/bin/s3util/cli/put_object_annotation.rs
// Adjustments: none beyond the note below — the upstream file already
//              targets `super::ExitStatus`.
//              Report println made pipe-safe (ported from s3util-rs 1.9.2).

use anyhow::{Context, Result};
use aws_sdk_s3::primitives::ByteStream;
use tracing::info;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::put_object_annotation::PutObjectAnnotationArgs;
use s3util_rs::output::json::put_object_annotation_to_json;
use s3util_rs::storage::annotation;
use s3util_rs::storage::s3::api::{self, HeadError, PutObjectAnnotationParams};

use crate::pipe_safe::println_pipe_safe;

use super::ExitStatus;

/// Runtime entry for
/// `s3util put-object-annotation s3://<BUCKET>/<KEY> --annotation-name N --annotation-payload P`.
///
/// Reads the payload (file or stdin `-`), enforces the 1-byte..=1-MiB size
/// limit locally, sends `PutObjectAnnotation` with Content-MD5 and an explicit
/// CRC64NVME, verifies the returned CRC64NVME matches the local value, and
/// prints the response as AWS-CLI-shape JSON. Returns `ExitStatus::NotFound`
/// (exit 4) when the bucket, object, or version does not exist; a verification
/// mismatch returns `Err` (exit 1).
pub async fn run_put_object_annotation(
    args: PutObjectAnnotationArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let (bucket, key) = args
        .bucket_key()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;

    let annotation_name = args
        .annotation_name
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--annotation-name is required"))?;
    annotation::validate_annotation_name(annotation_name)?;
    let payload_arg = args
        .annotation_payload
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--annotation-payload is required"))?;

    let cap = (annotation::MAX_ANNOTATION_PAYLOAD_LEN as u64) + 1;
    let payload: Vec<u8> = if payload_arg == "-" {
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(std::io::stdin(), cap), &mut buf)
            .context("reading annotation payload from stdin")?;
        annotation::validate_payload_len(buf.len())?;
        buf
    } else {
        // Bounded read over the actual bytes (not the stat size): correct for
        // FIFOs/special files and not TOCTOU-racy, matching the stdin path.
        let file = std::fs::File::open(payload_arg)
            .with_context(|| format!("reading annotation payload from {payload_arg}"))?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut std::io::Read::take(file, cap), &mut buf)
            .with_context(|| format!("reading annotation payload from {payload_arg}"))?;
        annotation::validate_payload_len(buf.len())?;
        buf
    };

    let content_md5 = annotation::content_md5_base64(&payload);
    let crc64 = annotation::crc64nvme_base64(&payload);
    let request_payer = client_config.request_payer.clone();

    let client = client_config.create_client().await;

    if args.dry_run {
        info!(
            bucket = %bucket,
            key = %key,
            annotation_name = %annotation_name,
            version_id = %args.target_version_id.as_deref().unwrap_or_default(),
            "[dry-run] would put object annotation."
        );
        return Ok(ExitStatus::Success);
    }

    let params = PutObjectAnnotationParams {
        bucket: &bucket,
        key: &key,
        annotation_name,
        version_id: args.target_version_id.as_deref(),
        content_md5: &content_md5,
        checksum_crc64_nvme: &crc64,
        request_payer,
    };

    match api::put_object_annotation(&client, params, ByteStream::from(payload)).await {
        Ok(out) => {
            annotation::verify_crc64nvme(&crc64, out.checksum_crc64_nvme())?;
            let json = put_object_annotation_to_json(&out);
            println_pipe_safe(&serde_json::to_string_pretty(&json)?)?;
            info!(
                bucket = %bucket,
                key = %key,
                annotation_name = %annotation_name,
                "Object annotation set and verified (Content-MD5 + CRC64NVME)."
            );
            Ok(ExitStatus::Success)
        }
        Err(HeadError::BucketNotFound) => {
            tracing::error!("bucket s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::NotFound) => {
            match args.target_version_id.as_deref() {
                Some(v) => tracing::error!("s3://{bucket}/{key} (versionId={v}) not found"),
                None => tracing::error!("object s3://{bucket}/{key} not found"),
            }
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::Other(e)) => Err(e),
    }
}
