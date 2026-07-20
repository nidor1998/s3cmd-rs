// Vendored from s3util-rs@1.1.0
//   src/bin/s3util/cli/put_object_tagging.rs
// Adjustments: no tests stripped; rewrote crate::cli → super

use anyhow::Result;
use tracing::info;

use aws_sdk_s3::types::Tagging;
use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::put_object_tagging::PutObjectTaggingArgs;
use s3util_rs::storage::s3::api;

use super::tagging::parse_tagging_to_tags;

/// Runtime entry for `s3util put-object-tagging s3://<BUCKET>/<KEY>`.
///
/// Builds the SDK client from `client_config`, issues `PutObjectTagging`,
/// replacing all existing tags with the supplied ones. Silent on success.
pub async fn run_put_object_tagging(
    args: PutObjectTaggingArgs,
    client_config: ClientConfig,
) -> Result<()> {
    let (bucket, key) = args
        .bucket_key()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;

    let tagging_str = args.tagging.as_deref().unwrap_or("");
    let tags = parse_tagging_to_tags(tagging_str)?;
    let tagging = Tagging::builder().set_tag_set(Some(tags)).build()?;

    let client = client_config.create_client().await;

    if args.dry_run {
        info!(
            bucket = %bucket,
            key = %key,
            version_id = %args.source_version_id.as_deref().unwrap_or_default(),
            "[dry-run] would put object tagging."
        );
        return Ok(());
    }

    api::put_object_tagging(
        &client,
        &bucket,
        &key,
        args.source_version_id.as_deref(),
        tagging,
        client_config.request_payer.clone(),
    )
    .await?;
    info!(
        bucket = %bucket,
        key = %key,
        version_id = %args.source_version_id.as_deref().unwrap_or_default(),
        "Object tagging set."
    );
    Ok(())
}
