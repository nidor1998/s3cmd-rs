// Vendored from s3ls-rs@0.4.1
//   src/bin/s3ls/main.rs
// Adjustments: removed #[tokio::main] async fn main() (s7cmd has its own);
//              helpers made pub for dispatch from s7cmd::main; stripped
//              any #[cfg(test)] blocks. The upstream
//              `load_config_exit_if_err` helper was dropped: it called
//              `clap::Error::exit()` (i.e. `std::process::exit(2)`) on bad
//              config, which would kill batch-run mid-batch. dispatch.rs
//              calls `Config::try_from` directly and returns exit code 2
//              on Err instead. run() now returns Result<i32> instead of
//              calling std::process::exit (so it can be invoked from
//              batch-run without killing the process mid-batch).
//              SIGINT exit code 130 ported from nidor1998/s3ls-rs#34:
//              after the pipeline stops, a run interrupted by Ctrl+C
//              returns 130 instead of falling through to the
//              success/error mapping — returned rather than upstream's
//              std::process::exit so batch-run survives.

use anyhow::Result;
use tracing::{debug, error};

use s3ls_rs::bucket_lister;
use s3ls_rs::config::Config;
use s3ls_rs::{
    ListingPipeline, create_pipeline_cancellation_token, exit_code_from_error, is_cancelled_error,
};

mod ctrl_c_handler;
mod tracing_init;

/// Conventional exit code for termination by Ctrl+C: 128 + SIGINT(2), the
/// shell encoding that lets scripts distinguish user interruption from
/// real failures.
const SIGINT_EXIT_CODE: i32 = 130;

pub fn start_tracing_if_necessary(config: &Config) -> bool {
    if let Some(tracing_config) = config.tracing_config.as_ref() {
        tracing_init::init_tracing(tracing_config);
        true
    } else {
        false
    }
}

pub async fn run(config: Config) -> Result<i32> {
    // Bucket listing mode: no target specified
    if config.target.bucket.is_empty() {
        return match bucket_lister::list_buckets(&config).await {
            Ok(()) => Ok(0),
            Err(e) => {
                if let Some(io_err) = e.downcast_ref::<std::io::Error>()
                    && io_err.kind() == std::io::ErrorKind::BrokenPipe
                {
                    return Ok(0);
                }
                error!("{}", e);
                Ok(1)
            }
        };
    }

    let cancellation_token = create_pipeline_cancellation_token();

    ctrl_c_handler::spawn_ctrl_c_handler(cancellation_token.clone());

    let start_time = tokio::time::Instant::now();
    debug!("listing pipeline start.");

    let pipeline = ListingPipeline::new(config, cancellation_token);

    let result = pipeline.run().await;

    // Ctrl+C takes precedence over whatever the pipeline returned: once
    // SIGINT is received the run is "interrupted", even if the shutdown
    // also surfaced an error. Returned (not process::exit as upstream
    // does) so batch-run survives and buckets the line as skipped.
    if ctrl_c_handler::is_ctrl_c_received() {
        let duration_sec = format!("{:.3}", start_time.elapsed().as_secs_f32());
        debug!(duration_sec = duration_sec, "listing cancelled by user.");
        return Ok(SIGINT_EXIT_CODE);
    }

    match result {
        Ok(()) => {
            let duration_sec = format!("{:.3}", start_time.elapsed().as_secs_f32());
            debug!(duration_sec = duration_sec, "s7cmd ls has been completed.");
            Ok(0)
        }
        Err(e) => {
            // Broken pipe is expected when piped to head/tail — exit silently.
            if let Some(io_err) = e.downcast_ref::<std::io::Error>()
                && io_err.kind() == std::io::ErrorKind::BrokenPipe
            {
                return Ok(0);
            }
            let duration_sec = format!("{:.3}", start_time.elapsed().as_secs_f32());
            if is_cancelled_error(&e) {
                debug!("listing cancelled by user.");
                return Ok(0);
            }
            let code = exit_code_from_error(&e);
            error!(duration_sec = duration_sec, "s7cmd ls failed.");
            error!("{}", e);
            Ok(code)
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use s3ls_rs::CLIArgs;

    use super::*;

    fn parse(args: &[&str]) -> CLIArgs {
        CLIArgs::try_parse_from(args).unwrap()
    }

    fn build_config(args: &[&str]) -> Config {
        Config::try_from(parse(args)).unwrap()
    }

    #[test]
    fn start_tracing_if_necessary_returns_false_when_no_tracing_config() {
        // -qqq drops below all tracing levels → tracing_config is None.
        let config = build_config(&["s3ls", "-qqq", "--target-profile", "p", "s3://test-bucket"]);
        assert!(config.tracing_config.is_none());
        assert!(!start_tracing_if_necessary(&config));
    }

    #[test]
    fn sigint_exit_code_is_130() {
        assert_eq!(SIGINT_EXIT_CODE, 130);
    }

    #[test]
    #[cfg(target_family = "unix")]
    fn sigint_exit_code_follows_128_plus_signal_number_convention() {
        assert_eq!(
            SIGINT_EXIT_CODE,
            128 + nix::sys::signal::Signal::SIGINT as i32
        );
    }

    /// Bucket-listing mode (no target prefix) against an unreachable endpoint.
    /// Exercises the `bucket_lister::list_buckets` Err arm — should return Ok(1).
    #[tokio::test]
    async fn run_bucket_listing_against_fake_endpoint_returns_one() {
        let config = build_config(&[
            "s3ls",
            "--target-access-key",
            "dummy",
            "--target-secret-access-key",
            "dummy",
            "--target-endpoint-url",
            "http://127.0.0.1:1",
            "--target-region",
            "us-east-1",
            "--connect-timeout-milliseconds",
            "1",
            "--aws-max-attempts",
            "0",
        ]);
        // Bucket listing mode — target.bucket is empty.
        assert!(config.target.bucket.is_empty());
        let code = run(config).await.unwrap();
        // Connection refused → list_buckets returns Err, run() maps to Ok(1).
        assert_eq!(code, 1);
    }

    /// Listing-pipeline mode with a target prefix against an unreachable
    /// endpoint. Exercises the `pipeline.run()` Err arm — should return a
    /// non-zero exit code (the SDK error is mapped via `exit_code_from_error`).
    #[tokio::test]
    async fn run_listing_pipeline_against_fake_endpoint_returns_nonzero() {
        let config = build_config(&[
            "s3ls",
            "--target-access-key",
            "dummy",
            "--target-secret-access-key",
            "dummy",
            "--target-endpoint-url",
            "http://127.0.0.1:1",
            "--target-region",
            "us-east-1",
            "--connect-timeout-milliseconds",
            "1",
            "--aws-max-attempts",
            "0",
            "s3://nonexistent-bucket-for-s7cmd-tests/prefix/",
        ]);
        assert!(!config.target.bucket.is_empty());
        let code = run(config).await.unwrap();
        // Connection refused → pipeline.run returns Err, run() returns non-zero.
        assert_ne!(code, 0);
    }
}
