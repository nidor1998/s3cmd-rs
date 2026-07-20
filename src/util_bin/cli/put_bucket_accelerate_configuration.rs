// Vendored from s3util-rs@1.3.0
//   src/bin/s3util/cli/put_bucket_accelerate_configuration.rs
// Adjustments: no tests stripped; rewrote crate::cli → super;
//              replaced the process-exiting `validate_state_flag()` call
//              with the non-exiting `check_state_flag` invoked from
//              dispatch.rs (a `process::exit(2)` would kill a whole
//              batch-run instead of failing one line).
use anyhow::Result;
use aws_sdk_s3::types::AccelerateConfiguration;
use clap::CommandFactory;
use tracing::info;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::put_bucket_accelerate_configuration::PutBucketAccelerateConfigurationArgs;
use s3util_rs::storage::s3::api;

/// Runtime entry for
/// `s3util put-bucket-accelerate-configuration s3://<BUCKET> --enabled|--suspended`.
///
/// Builds the SDK client from `client_config`, issues
/// `PutBucketAccelerateConfiguration` with `Status=Enabled` or
/// `Status=Suspended` (determined by the mutually-exclusive `--enabled` /
/// `--suspended` flags), and exits silently on success.
pub async fn run_put_bucket_accelerate_configuration(
    args: PutBucketAccelerateConfigurationArgs,
    client_config: ClientConfig,
) -> Result<()> {
    // --enabled/--suspended presence is enforced by `check_state_flag`,
    // which dispatch.rs runs before invoking this runner.
    let bucket = args
        .bucket_name()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;
    let status = args.accelerate_status();
    let cfg = AccelerateConfiguration::builder()
        .status(status.clone())
        .build();
    let client = client_config.create_client().await;
    if args.dry_run {
        info!(
            bucket = %bucket,
            status = %status.as_str(),
            "[dry-run] would put bucket accelerate configuration."
        );
        return Ok(());
    }
    api::put_bucket_accelerate_configuration(&client, &bucket, cfg).await?;
    info!(
        bucket = %bucket,
        status = %status.as_str(),
        "Bucket Transfer Acceleration set."
    );
    Ok(())
}

/// Non-exiting replacement for upstream's `validate_state_flag`, whose
/// `clap::Error::exit()` (`std::process::exit(2)`) cannot be contained by
/// batch-run's per-line error handling. dispatch.rs calls this before the
/// runner, prints the error, and returns exit code 2 — same message and
/// code as upstream, without terminating the process.
pub fn check_state_flag(args: &PutBucketAccelerateConfigurationArgs) -> Result<(), clap::Error> {
    if !args.enabled && !args.suspended {
        let mut cmd = PutBucketAccelerateConfigurationArgs::command();
        return Err(cmd.error(
            clap::error::ErrorKind::MissingRequiredArgument,
            "one of --enabled or --suspended must be specified",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(argv: &[&str]) -> PutBucketAccelerateConfigurationArgs {
        PutBucketAccelerateConfigurationArgs::try_parse_from(argv).expect("args should parse")
    }

    #[test]
    fn check_state_flag_rejects_neither_flag() {
        let args = parse(&["put-bucket-accelerate-configuration", "s3://b"]);
        let err = check_state_flag(&args).expect_err("neither flag must be rejected");
        assert_eq!(err.kind(), clap::error::ErrorKind::MissingRequiredArgument);
        assert!(
            err.to_string()
                .contains("one of --enabled or --suspended must be specified")
        );
    }

    #[test]
    fn check_state_flag_accepts_enabled() {
        let args = parse(&["put-bucket-accelerate-configuration", "s3://b", "--enabled"]);
        assert!(check_state_flag(&args).is_ok());
    }

    #[test]
    fn check_state_flag_accepts_suspended() {
        let args = parse(&[
            "put-bucket-accelerate-configuration",
            "s3://b",
            "--suspended",
        ]);
        assert!(check_state_flag(&args).is_ok());
    }
}
