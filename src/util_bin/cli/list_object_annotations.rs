// Vendored from s3util-rs@1.6.0
//   src/bin/s3util/cli/list_object_annotations.rs
// Adjustments: none — the upstream file already targets `super::ExitStatus`.

use anyhow::Result;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::list_object_annotations::ListObjectAnnotationsArgs;
use s3util_rs::output::json::list_object_annotations_to_json;
use s3util_rs::storage::s3::api::{self, HeadError, ListObjectAnnotationsParams};

use super::ExitStatus;

/// Runtime entry for
/// `s3util list-object-annotations s3://<BUCKET>/<KEY>`.
///
/// Builds the SDK client from `client_config`, issues `ListObjectAnnotations`
/// (`max-annotation-results=1000`, single request), and prints the response as
/// AWS-CLI-shape pretty-printed JSON followed by a newline. Returns
/// `ExitStatus::NotFound` (exit code 4) when S3 reports the object, bucket, or
/// version does not exist.
pub async fn run_list_object_annotations(
    args: ListObjectAnnotationsArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let (bucket, key) = args
        .bucket_key()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;

    let request_payer = client_config.request_payer.clone();
    let client = client_config.create_client().await;

    let params = ListObjectAnnotationsParams {
        bucket: &bucket,
        key: &key,
        annotation_prefix: args.annotation_prefix.as_deref(),
        version_id: args.target_version_id.as_deref(),
        request_payer,
    };

    match api::list_object_annotations(&client, params).await {
        Ok(out) => {
            let json = list_object_annotations_to_json(&out);
            let pretty = serde_json::to_string_pretty(&json)?;
            println!("{pretty}");
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
