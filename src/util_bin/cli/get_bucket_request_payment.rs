// Vendored from s3util-rs@1.3.0
//   src/bin/s3util/cli/get_bucket_request_payment.rs
// Adjustments: no tests stripped; rewrote crate::cli → super
//              Report println made pipe-safe (ported from s3util-rs 1.9.2).
use anyhow::Result;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::get_bucket_request_payment::GetBucketRequestPaymentArgs;
use s3util_rs::output::json::get_bucket_request_payment_to_json;
use s3util_rs::storage::s3::api::{self, HeadError};

use crate::pipe_safe::println_pipe_safe;

use super::ExitStatus;

/// Runtime entry for `s3util get-bucket-request-payment s3://<BUCKET>`.
///
/// Builds the SDK client from `client_config`, issues `GetBucketRequestPayment`,
/// and prints the response as AWS-CLI-shape pretty-printed JSON
/// (`{"Payer": "Requester"|"BucketOwner"}`) followed by a newline.
/// Returns `ExitStatus::NotFound` (exit code 4) when S3 reports `NoSuchBucket`.
pub async fn run_get_bucket_request_payment(
    args: GetBucketRequestPaymentArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let bucket = args
        .bucket_name()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;
    let client = client_config.create_client().await;
    match api::get_bucket_request_payment(&client, &bucket).await {
        Ok(out) => {
            let json = get_bucket_request_payment_to_json(&out);
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
