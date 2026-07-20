// Vendored from s3util-rs@1.1.0
//   src/bin/s3util/cli/rm.rs
// Adjustments: stripped #[cfg(test)] mod tests; rewrote crate::cli → super

use anyhow::Result;
use tracing::info;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::rm::RmArgs;
use s3util_rs::storage::s3::api;

/// Runtime entry for `s3util rm s3://<BUCKET>/<KEY>`.
///
/// Builds the SDK client from `client_config`, issues `DeleteObject`,
/// and returns `Ok(())` on success (silent — no output).
pub async fn run_rm(args: RmArgs, client_config: ClientConfig) -> Result<()> {
    let (bucket, key) = args
        .bucket_key()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;

    let client = client_config.create_client().await;

    if args.dry_run {
        info!(
            bucket = %bucket,
            key = %key,
            version_id = %args.source_version_id.as_deref().unwrap_or_default(),
            "[dry-run] would delete object."
        );
        return Ok(());
    }

    api::delete_object(
        &client,
        &bucket,
        &key,
        args.source_version_id.as_deref(),
        client_config.request_payer.clone(),
    )
    .await?;
    info!(
        bucket = %bucket,
        key = %key,
        version_id = %args.source_version_id.as_deref().unwrap_or_default(),
        "Object deleted."
    );
    Ok(())
}
