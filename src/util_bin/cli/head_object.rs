// Vendored from s3util-rs@0.2.0
//   src/bin/s3util/cli/head_object.rs
// Adjustments: no tests stripped; rewrote crate::cli → super;
//              merged the unreachable `HeadError::BucketNotFound` arm into
//              `HeadError::NotFound` (api::head_object only checks
//              `is_not_found()`, so any 404 is `NotFound` regardless of
//              whether the bucket or the key is missing).
//              Report println made pipe-safe (ported from s3util-rs 1.9.2).

use anyhow::Result;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::head_object::HeadObjectArgs;
use s3util_rs::output::json::head_object_to_json;
use s3util_rs::storage::s3::api::{self, HeadError, HeadObjectOpts};

use crate::pipe_safe::println_pipe_safe;

use super::ExitStatus;

/// Runtime entry for `s3util head-object s3://<BUCKET>/<KEY>`.
///
/// Builds the SDK client from `client_config`, issues `HeadObject`, prints
/// the response as AWS-CLI-shape pretty-printed JSON, and returns the exit
/// status. Returns `ExitStatus::NotFound` (exit code 4) when the object
/// (or its bucket / version) does not exist; bubbles up any other error
/// via `anyhow`.
pub async fn run_head_object(
    args: HeadObjectArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let (bucket, key) = args
        .bucket_key()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;

    let client = client_config.create_client().await;

    let opts = HeadObjectOpts {
        version_id: args.source_version_id.clone(),
        sse_c: args.source_sse_c.clone(),
        sse_c_key: args.source_sse_c_key.clone(),
        sse_c_key_md5: args.source_sse_c_key_md5.clone(),
        enable_additional_checksum: args.enable_additional_checksum,
        request_payer: client_config.request_payer.clone(),
    };

    match api::head_object(&client, &bucket, &key, opts).await {
        Ok(out) => {
            let json = head_object_to_json(&out);
            let pretty = serde_json::to_string_pretty(&json)?;
            println_pipe_safe(&pretty)?;
            Ok(ExitStatus::Success)
        }
        Err(HeadError::BucketNotFound | HeadError::NotFound) => {
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
