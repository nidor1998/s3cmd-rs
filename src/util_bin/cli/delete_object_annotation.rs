// Vendored from s3util-rs@1.6.0
//   src/bin/s3util/cli/delete_object_annotation.rs
// Adjustments: none — the upstream file already targets `super::ExitStatus`.

use anyhow::Result;
use tracing::info;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::delete_object_annotation::DeleteObjectAnnotationArgs;
use s3util_rs::storage::annotation;
use s3util_rs::storage::s3::api::{self, DeleteObjectAnnotationParams, ObjectAnnotationError};

use super::ExitStatus;

/// Runtime entry for
/// `s3util delete-object-annotation s3://<BUCKET>/<KEY> --annotation-name N`.
///
/// Sends `DeleteObjectAnnotation`. Prints nothing to stdout on success (emits a
/// single `info!` line on stderr). Returns `ExitStatus::NotFound` (exit 4) when
/// the bucket, object, or version does not exist, or when the object exists but
/// has no annotation under the requested name (`NoSuchAnnotation`, logged as
/// "annotation … not found"); any other S3 error returns `Err` (exit 1).
pub async fn run_delete_object_annotation(
    args: DeleteObjectAnnotationArgs,
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

    let request_payer = client_config.request_payer.clone();
    let client = client_config.create_client().await;

    if args.dry_run {
        info!(
            bucket = %bucket,
            key = %key,
            annotation_name = %annotation_name,
            version_id = %args.target_version_id.as_deref().unwrap_or_default(),
            "[dry-run] would delete object annotation."
        );
        return Ok(ExitStatus::Success);
    }

    let params = DeleteObjectAnnotationParams {
        bucket: &bucket,
        key: &key,
        annotation_name,
        version_id: args.target_version_id.as_deref(),
        request_payer,
    };

    match api::delete_object_annotation(&client, params).await {
        Ok(_) => {
            info!(
                bucket = %bucket,
                key = %key,
                annotation_name = %annotation_name,
                "Object annotation deleted."
            );
            Ok(ExitStatus::Success)
        }
        Err(ObjectAnnotationError::BucketNotFound) => {
            tracing::error!("bucket s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(ObjectAnnotationError::NotFound) => {
            match args.target_version_id.as_deref() {
                Some(v) => tracing::error!("s3://{bucket}/{key} (versionId={v}) not found"),
                None => tracing::error!("object s3://{bucket}/{key} not found"),
            }
            Ok(ExitStatus::NotFound)
        }
        Err(ObjectAnnotationError::AnnotationNotFound) => {
            match args.target_version_id.as_deref() {
                Some(v) => tracing::error!(
                    "annotation {annotation_name} not found for s3://{bucket}/{key} (versionId={v})"
                ),
                None => {
                    tracing::error!(
                        "annotation {annotation_name} not found for s3://{bucket}/{key}"
                    )
                }
            }
            Ok(ExitStatus::NotFound)
        }
        Err(ObjectAnnotationError::Other(e)) => Err(e),
    }
}
