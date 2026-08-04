// Vendored from s3util-rs@0.2.0
//   src/bin/s3util/cli/head_bucket.rs
// Adjustments: no tests stripped; rewrote crate::cli → super
//              Report println made pipe-safe (ported from s3util-rs 1.9.2).

use anyhow::Result;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::head_bucket::HeadBucketArgs;
use s3util_rs::output::json::head_bucket_to_json;
use s3util_rs::storage::s3::api::{self, HeadError};

use crate::pipe_safe::println_pipe_safe;

use super::ExitStatus;

/// Runtime entry for `s3util head-bucket s3://<BUCKET>`.
///
/// Builds the SDK client from `client_config`, issues `HeadBucket`, prints
/// the response as AWS-CLI-shape pretty-printed JSON, and returns the exit
/// status. Returns `ExitStatus::NotFound` (exit code 4) when the bucket
/// does not exist; bubbles up any other error via `anyhow`.
pub async fn run_head_bucket(
    args: HeadBucketArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let bucket = args
        .bucket_name()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;
    let client = client_config.create_client().await;

    match api::head_bucket(&client, &bucket).await {
        Ok(out) => {
            let json = head_bucket_to_json(&out);
            let pretty = serde_json::to_string_pretty(&json)?;
            println_pipe_safe(&pretty)?;
            Ok(ExitStatus::Success)
        }
        Err(HeadError::BucketNotFound) | Err(HeadError::NotFound) => {
            tracing::warn!("bucket s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::Other(e)) => Err(e),
    }
}
