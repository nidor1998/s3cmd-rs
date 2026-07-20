// Vendored from s3util-rs@1.8.0
//   src/bin/s3util/cli/put_bucket_lifecycle_configuration.rs
// Adjustments: no tests stripped; rewrote crate::cli → super
// The 1.8.0 library rejects unknown JSON fields (`deny_unknown_fields`), so a
// top-level `TransitionDefaultMinimumObjectSize` — as produced by
// `get-bucket-lifecycle-configuration` — fails the parse; the runner maps that
// error to a hint pointing at `--transition-default-minimum-object-size`,
// which sends the value as the request parameter S3 actually accepts.
use anyhow::{Context, Result};
use tracing::info;

use aws_sdk_s3::types::TransitionDefaultMinimumObjectSize;
use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::put_bucket_lifecycle_configuration::PutBucketLifecycleConfigurationArgs;
use s3util_rs::input::json::LifecycleConfigurationJson;
use s3util_rs::storage::s3::api;

/// Runtime entry for
/// `s3util put-bucket-lifecycle-configuration s3://<BUCKET> <CONFIG_FILE|->`.
///
/// Reads the configuration JSON from a file path or stdin (`-`), parses it
/// into a `LifecycleConfigurationJson` mirror struct (AWS-CLI input shape),
/// converts to the SDK type, and issues `PutBucketLifecycleConfiguration`.
/// Exits silently on success.
pub async fn run_put_bucket_lifecycle_configuration(
    args: PutBucketLifecycleConfigurationArgs,
    client_config: ClientConfig,
) -> Result<()> {
    let bucket = args
        .bucket_name()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;

    let config_arg = args
        .lifecycle_configuration
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("lifecycle-configuration source required"))?;

    let body = if config_arg == "-" {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
        buf
    } else {
        std::fs::read_to_string(config_arg)
            .with_context(|| format!("reading lifecycle configuration from {config_arg}"))?
    };

    let parsed = parse_lifecycle_configuration(&body)
        .with_context(|| format!("parsing JSON from {config_arg}"))?;
    let cfg = parsed.into_sdk()?;

    let client = client_config.create_client().await;
    if args.dry_run {
        info!(bucket = %bucket, "[dry-run] would put bucket lifecycle configuration.");
        return Ok(());
    }
    let transition_default_minimum_object_size = args
        .transition_default_minimum_object_size
        .as_deref()
        .map(TransitionDefaultMinimumObjectSize::from);
    api::put_bucket_lifecycle_configuration(
        &client,
        &bucket,
        cfg,
        transition_default_minimum_object_size,
    )
    .await?;
    info!(bucket = %bucket, "Bucket lifecycle configuration set.");
    Ok(())
}

/// Parses the lifecycle configuration JSON body.
///
/// `get-bucket-lifecycle-configuration` reports `TransitionDefaultMinimumObjectSize`
/// at the top level (matching `aws s3api`), but S3 accepts it only as a request
/// parameter, never inside the configuration document — so feeding the get
/// output back into put is rejected by `deny_unknown_fields`. That rejection is
/// correct (silently dropping it would reset the bucket to S3's default), but
/// the bare serde error is a dead end; point at the flag that sends it.
fn parse_lifecycle_configuration(body: &str) -> Result<LifecycleConfigurationJson> {
    serde_json::from_str(body).map_err(|e| {
        if e.to_string()
            .contains("unknown field `TransitionDefaultMinimumObjectSize`")
        {
            anyhow::anyhow!(e).context(
                "`TransitionDefaultMinimumObjectSize` is a request parameter, not part of the \
                 lifecycle configuration document: remove it from the JSON and pass it with \
                 --transition-default-minimum-object-size instead",
            )
        } else {
            anyhow::anyhow!(e)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_rules_document_parses() {
        let body = r#"{"Rules": [{"ID": "r1", "Status": "Enabled",
            "Expiration": {"Days": 30}, "Filter": {"Prefix": ""}}]}"#;
        assert!(parse_lifecycle_configuration(body).is_ok());
    }

    /// Feeding `get-bucket-lifecycle-configuration` output (which carries
    /// `TransitionDefaultMinimumObjectSize` at the top level) into put must
    /// fail with a hint at the flag, not a bare unknown-field error.
    #[test]
    fn get_output_fed_back_into_put_hints_at_the_flag() {
        let body = r#"{"Rules": [{"ID": "r1", "Status": "Enabled",
            "Expiration": {"Days": 30}, "Filter": {"Prefix": ""}}],
            "TransitionDefaultMinimumObjectSize": "all_storage_classes_128K"}"#;
        let err = parse_lifecycle_configuration(body).unwrap_err();
        assert!(
            format!("{err:#}").contains("--transition-default-minimum-object-size"),
            "error must point at the flag; got: {err:#}"
        );
    }

    /// Any other unknown field keeps the plain serde error, without the
    /// unrelated hint.
    #[test]
    fn other_unknown_fields_do_not_get_the_hint() {
        let body = r#"{"Rules": [], "Bogus": 1}"#;
        let err = parse_lifecycle_configuration(body).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unknown field `Bogus`"), "got: {msg}");
        assert!(
            !msg.contains("--transition-default-minimum-object-size"),
            "hint must only fire for TransitionDefaultMinimumObjectSize; got: {msg}"
        );
    }

    /// Invalid JSON still surfaces as a serde error.
    #[test]
    fn invalid_json_is_a_parse_error() {
        assert!(parse_lifecycle_configuration("{not json").is_err());
    }
}
