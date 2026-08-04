// Vendored from s3util-rs@0.2.2
//   src/bin/s3util/cli/get_public_access_block.rs
// Adjustments: no tests stripped; rewrote crate::cli → super
//              Report println made pipe-safe (ported from s3util-rs 1.9.2).
use anyhow::Result;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::get_public_access_block::GetPublicAccessBlockArgs;
use s3util_rs::output::json::get_public_access_block_to_json;
use s3util_rs::storage::s3::api::{self, HeadError};

use crate::pipe_safe::println_pipe_safe;

use super::ExitStatus;

/// Runtime entry for `s3util get-public-access-block s3://<BUCKET>`.
///
/// Builds the SDK client from `client_config`, issues `GetPublicAccessBlock`,
/// and prints the response as AWS-CLI-shape pretty-printed JSON followed by
/// a newline. Returns `ExitStatus::NotFound` (exit code 4) when S3 reports
/// `NoSuchBucket` (logged as "bucket … not found") or
/// `NoSuchPublicAccessBlockConfiguration` (logged as
/// "public access block configuration for … not found").
pub async fn run_get_public_access_block(
    args: GetPublicAccessBlockArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let bucket = args
        .bucket_name()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;
    let client = client_config.create_client().await;
    match api::get_public_access_block(&client, &bucket).await {
        Ok(out) => {
            let json = get_public_access_block_to_json(&out);
            let pretty = serde_json::to_string_pretty(&json)?;
            println_pipe_safe(&pretty)?;
            Ok(ExitStatus::Success)
        }
        Err(HeadError::BucketNotFound) => {
            tracing::warn!("bucket s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::NotFound) => {
            tracing::warn!("public access block configuration for s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::Other(e)) => Err(e),
    }
}
