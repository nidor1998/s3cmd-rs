// Vendored from s3util-rs@0.2.2
//   src/bin/s3util/cli/get_bucket_logging.rs
// Adjustments: no tests stripped; rewrote crate::cli → super
//              Report println made pipe-safe (ported from s3util-rs 1.9.2).
use anyhow::Result;
use tracing::info;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::get_bucket_logging::GetBucketLoggingArgs;
use s3util_rs::output::json::get_bucket_logging_to_json;
use s3util_rs::storage::s3::api::{self, HeadError};

use crate::pipe_safe::println_pipe_safe;

use super::ExitStatus;

/// Runtime entry for `s3util get-bucket-logging s3://<BUCKET>`.
///
/// Builds the SDK client from `client_config`, issues `GetBucketLogging`,
/// and prints the response as AWS-CLI-shape pretty-printed JSON followed
/// by a newline. When the bucket has no logging configured, S3 returns
/// success with an empty body (there is no `NoSuchLoggingConfiguration`
/// error code) and an empty JSON object `{}` is logged at info level —
/// matching the empty-body semantics of `get-bucket-versioning`. Returns
/// `ExitStatus::NotFound` (exit code 4) when S3 reports `NoSuchBucket`.
pub async fn run_get_bucket_logging(
    args: GetBucketLoggingArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let bucket = args
        .bucket_name()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;
    let client = client_config.create_client().await;
    match api::get_bucket_logging(&client, &bucket).await {
        Ok(out) => {
            let json = get_bucket_logging_to_json(&out);
            if json.as_object().is_some_and(|m| m.is_empty()) {
                info!(bucket = %bucket, "Bucket logging not configured.");
            } else {
                let pretty = serde_json::to_string_pretty(&json)?;
                println_pipe_safe(&pretty)?;
            }
            Ok(ExitStatus::Success)
        }
        Err(HeadError::BucketNotFound) | Err(HeadError::NotFound) => {
            // `get-bucket-logging` never returns `NoSuchLoggingConfiguration`
            // — the NotFound arm is unreachable in practice but kept here
            // for symmetry with the other `get_*` runtimes and to satisfy
            // exhaustiveness on the shared `HeadError` type.
            tracing::warn!("bucket s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::Other(e)) => Err(e),
    }
}
