// Vendored from s3util-rs@1.3.0
//   src/bin/s3util/cli/get_bucket_policy_status.rs
// Adjustments: no tests stripped; rewrote crate::cli → super
//              Report println made pipe-safe (ported from s3util-rs 1.9.2).
use anyhow::Result;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::get_bucket_policy_status::GetBucketPolicyStatusArgs;
use s3util_rs::output::json::get_bucket_policy_status_to_json;
use s3util_rs::storage::s3::api::{self, HeadError};

use crate::pipe_safe::println_pipe_safe;

use super::ExitStatus;

/// Runtime entry for `s3util get-bucket-policy-status s3://<BUCKET>`.
///
/// Builds the SDK client from `client_config`, issues `GetBucketPolicyStatus`,
/// and prints the response as AWS-CLI-shape pretty-printed JSON
/// (`{"PolicyStatus": {"IsPublic": true|false}}`) followed by a newline.
/// Returns `ExitStatus::NotFound` (exit code 4) when S3 reports `NoSuchBucket`
/// (logged as "bucket … not found") or `NoSuchBucketPolicy` (logged as
/// "policy for … not found"; without a policy there is no policy status).
pub async fn run_get_bucket_policy_status(
    args: GetBucketPolicyStatusArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let bucket = args
        .bucket_name()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;
    let client = client_config.create_client().await;
    match api::get_bucket_policy_status(&client, &bucket).await {
        Ok(out) => {
            let json = get_bucket_policy_status_to_json(&out);
            let pretty = serde_json::to_string_pretty(&json)?;
            println_pipe_safe(&pretty)?;
            Ok(ExitStatus::Success)
        }
        Err(HeadError::BucketNotFound) => {
            tracing::warn!("bucket s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::NotFound) => {
            tracing::warn!("policy for s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::Other(e)) => Err(e),
    }
}
