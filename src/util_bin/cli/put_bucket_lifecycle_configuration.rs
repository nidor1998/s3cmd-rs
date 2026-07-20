// Vendored from s3util-rs@1.1.0
//   src/bin/s3util/cli/put_bucket_lifecycle_configuration.rs
// Adjustments: no tests stripped; rewrote crate::cli → super
// Includes an adapted fix from s3util-rs PR#25 (post-1.7.1): a top-level
// `TransitionDefaultMinimumObjectSize` in the input JSON is rejected with
// guidance instead of being silently dropped. Upstream implements this via
// `deny_unknown_fields` on the input structs plus a
// `--transition-default-minimum-object-size` flag; neither exists in the
// released 1.7.1 library, so the vendored runner checks the raw JSON itself
// (the flag pointer in the upstream hint is omitted until the library ships).
use anyhow::{Context, Result};
use tracing::info;

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
    api::put_bucket_lifecycle_configuration(&client, &bucket, cfg).await?;
    info!(bucket = %bucket, "Bucket lifecycle configuration set.");
    Ok(())
}

/// Parses the lifecycle configuration JSON body.
///
/// `get-bucket-lifecycle-configuration` reports `TransitionDefaultMinimumObjectSize`
/// at the top level (matching `aws s3api`), but S3 accepts it only as a request
/// parameter, never inside the configuration document — so feeding the get
/// output back into put must not carry it along. The 1.7.1 library's
/// `LifecycleConfigurationJson` silently ignores unknown fields, which would
/// drop the value and reset the bucket to S3's default of
/// `all_storage_classes_128K`. Reject it up front instead.
fn parse_lifecycle_configuration(body: &str) -> Result<LifecycleConfigurationJson> {
    let value: serde_json::Value = serde_json::from_str(body)?;
    if value
        .as_object()
        .is_some_and(|o| o.contains_key("TransitionDefaultMinimumObjectSize"))
    {
        anyhow::bail!(
            "`TransitionDefaultMinimumObjectSize` is a request parameter, not part of the \
             lifecycle configuration document: remove it from the JSON (S3 then applies its \
             default of `all_storage_classes_128K`)"
        );
    }
    serde_json::from_value(value).map_err(anyhow::Error::from)
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
    /// fail with guidance, not silently drop the value — dropping it would
    /// reset a bucket set to `varies_by_storage_class` back to S3's default.
    #[test]
    fn get_output_fed_back_into_put_is_rejected_with_guidance() {
        let body = r#"{"Rules": [{"ID": "r1", "Status": "Enabled",
            "Expiration": {"Days": 30}, "Filter": {"Prefix": ""}}],
            "TransitionDefaultMinimumObjectSize": "all_storage_classes_128K"}"#;
        let err = parse_lifecycle_configuration(body).unwrap_err();
        assert!(
            format!("{err:#}").contains("request parameter"),
            "error must explain the field is a request parameter; got: {err:#}"
        );
    }

    /// Other unknown fields keep the 1.7.1 library behavior (ignored by
    /// serde), without the unrelated guidance firing.
    #[test]
    fn other_unknown_fields_do_not_trigger_the_guidance() {
        let body = r#"{"Rules": [], "Bogus": 1}"#;
        let result = parse_lifecycle_configuration(body);
        assert!(
            result.is_ok(),
            "unknown fields other than TransitionDefaultMinimumObjectSize keep \
             the 1.7.1 ignore behavior; got: {:?}",
            result.err()
        );
    }

    /// Invalid JSON still surfaces as a serde error.
    #[test]
    fn invalid_json_is_a_parse_error() {
        assert!(parse_lifecycle_configuration("{not json").is_err());
    }
}
