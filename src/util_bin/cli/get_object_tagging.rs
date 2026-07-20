// Vendored from s3util-rs@0.2.0
//   src/bin/s3util/cli/get_object_tagging.rs
// Adjustments: no tests stripped; rewrote crate::cli → super

use anyhow::Result;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::get_object_tagging::GetObjectTaggingArgs;
use s3util_rs::output::json::get_object_tagging_to_json;
use s3util_rs::storage::s3::api::{self, HeadError};

use super::ExitStatus;

/// Runtime entry for `s3util get-object-tagging s3://<BUCKET>/<KEY>`.
///
/// Builds the SDK client from `client_config`, issues `GetObjectTagging`,
/// and prints the response as AWS-CLI-shape pretty-printed JSON
/// followed by a newline. Returns `ExitStatus::NotFound` (exit code 4) when
/// S3 reports the object, bucket, or version does not exist.
pub async fn run_get_object_tagging(
    args: GetObjectTaggingArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let (bucket, key) = args
        .bucket_key()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;

    let client = client_config.create_client().await;

    match api::get_object_tagging(
        &client,
        &bucket,
        &key,
        args.source_version_id.as_deref(),
        client_config.request_payer.clone(),
    )
    .await
    {
        Ok(out) => {
            let json = get_object_tagging_to_json(&out);
            let pretty = serde_json::to_string_pretty(&json)?;
            println!("{pretty}");
            Ok(ExitStatus::Success)
        }
        Err(HeadError::BucketNotFound) => {
            tracing::warn!("bucket s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::NotFound) => {
            match args.source_version_id.as_deref() {
                Some(v) => {
                    tracing::warn!("s3://{bucket}/{key} (versionId={v}) not found");
                }
                None => {
                    tracing::warn!("s3://{bucket}/{key} not found");
                }
            }
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::Other(e)) => Err(e),
    }
}
