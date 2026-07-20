// Vendored from s3util-rs@1.4.0
//   src/bin/s3util/cli/presign.rs
// Adjustments: rewrote crate::cli → super.

use std::time::Duration;

use anyhow::Result;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::presign::PresignArgs;
use s3util_rs::storage::s3::api;

use super::ExitStatus;

/// Runtime entry for `s7cmd presign s3://<BUCKET>/<KEY> [--expires-in N]`.
///
/// Builds the SDK client from `client_config`, generates a pre-signed URL
/// for `GetObject`, and prints it on stdout. The URL is signed locally —
/// no S3 API call is made — so the existence of the bucket or key is not
/// verified.
pub async fn run_presign(args: PresignArgs, client_config: ClientConfig) -> Result<ExitStatus> {
    let (bucket, key) = args
        .bucket_key()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;

    let client = client_config.create_client().await;
    let url = api::presign_get_object(
        &client,
        &bucket,
        &key,
        Duration::from_secs(args.expires_in),
        client_config.request_payer.clone(),
    )
    .await?;
    println!("{url}");
    Ok(ExitStatus::Success)
}
