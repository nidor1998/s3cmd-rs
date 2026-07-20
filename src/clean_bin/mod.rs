// Vendored from s3rm-rs@1.3.3
//   src/bin/s3rm/main.rs
// Adjustments: removed #[tokio::main] async fn main() (s7cmd has its own);
//              stripped #[cfg(test)] indicator_properties module declaration
//              and any test blocks; helpers (start_tracing_if_necessary,
//              run, EXIT_CODE_*) made pub for dispatch from s7cmd::main.
//              The upstream `load_config_exit_if_err` helper was dropped:
//              it called `clap::Error::exit()` (i.e. `std::process::exit(2)`)
//              on bad config, which would kill batch-run mid-batch.
//              dispatch.rs calls `Config::try_from` directly and returns
//              exit code 2 on Err instead. run() now returns Result<i32>
//              instead of calling std::process::exit (so it can be invoked
//              from batch-run without killing the process mid-batch).
//              "Deletion cancelled." made EPIPE-safe (a bare eprintln!
//              panics when stderr is a closed pipe).

use std::io::Write;

use anyhow::Result;
use tracing::{debug, error};

use s3rm_rs::config::Config;
use s3rm_rs::{
    DeletionPipeline, create_pipeline_cancellation_token, exit_code_from_error, is_cancelled_error,
};

pub mod ctrl_c_handler;
pub mod indicator;
mod tracing_init;
pub mod ui_config;

pub const EXIT_CODE_SUCCESS: i32 = 0;
pub const EXIT_CODE_WARNING: i32 = 3;
pub const EXIT_CODE_ABNORMAL_TERMINATION: i32 = 101;

pub fn start_tracing_if_necessary(config: &Config) -> bool {
    if let Some(tracing_config) = config.tracing_config.as_ref() {
        tracing_init::init_tracing(tracing_config);
        true
    } else {
        false
    }
}

pub async fn run(config: Config) -> Result<i32> {
    #[allow(unused_assignments)]
    let mut has_warning = false;

    {
        let cancellation_token = create_pipeline_cancellation_token();

        let start_time = tokio::time::Instant::now();
        debug!("deletion pipeline start.");

        let mut pipeline = DeletionPipeline::new(config.clone(), cancellation_token.clone()).await;

        // Check prerequisites (confirmation prompt) before starting the indicator,
        // so the progress bar doesn't interfere with the prompt.
        // The Ctrl+C handler is spawned AFTER this so that the default OS
        // SIGINT handler remains active during the blocking stdin read,
        // allowing Ctrl+C to terminate the process immediately at the prompt.
        if let Err(e) = pipeline.check_prerequisites().await {
            pipeline.close_stats_sender();
            if is_cancelled_error(&e) {
                // Ignore write errors (EPIPE): a closed stderr must not
                // panic what is otherwise a clean, successful cancellation.
                let _ = writeln!(std::io::stderr(), "Deletion cancelled.");
                debug!("deletion cancelled by user.");
                return Ok(EXIT_CODE_SUCCESS);
            }
            let code = exit_code_from_error(&e);
            error!("{}", e);
            return Ok(code);
        }

        // Now that the blocking prompt is done, install the async Ctrl+C
        // handler for graceful pipeline shutdown.
        ctrl_c_handler::spawn_ctrl_c_handler(cancellation_token);

        let indicator_join_handle = indicator::show_indicator(
            pipeline.get_stats_receiver(),
            ui_config::is_progress_indicator_needed(&config),
            ui_config::is_show_result_needed(&config),
            config.dry_run,
        );

        pipeline.run().await;
        match indicator_join_handle.await {
            Ok(_summary) => {}
            Err(e) => {
                error!("indicator task panicked: {}", e);
                return Ok(EXIT_CODE_ABNORMAL_TERMINATION);
            }
        }

        let duration_sec = format!("{:.3}", start_time.elapsed().as_secs_f32());

        if pipeline.has_error() {
            if pipeline.has_panic() {
                error!(
                    duration_sec = duration_sec,
                    "s7cmd clean abnormal termination."
                );
                return Ok(EXIT_CODE_ABNORMAL_TERMINATION);
            }
            let Some(errors) = pipeline.get_errors_and_consume() else {
                // has_error() was true but no errors found — should not happen.
                error!(duration_sec = duration_sec, "s7cmd clean failed.");
                return Ok(1);
            };
            // Use the highest exit code across all errors so that a severe
            // status (e.g. 3 for PartialFailure) is not downgraded by a
            // subsequent generic error (code 1).
            let mut code = 1;
            for err in &errors {
                if is_cancelled_error(err) {
                    debug!("deletion cancelled by user.");
                    return Ok(EXIT_CODE_SUCCESS);
                }
                code = code.max(exit_code_from_error(err));
                error!("{}", err);
            }
            error!(duration_sec = duration_sec, "s7cmd clean failed.");
            return Ok(code);
        }

        has_warning = pipeline.has_warning();

        debug!(
            duration_sec = duration_sec,
            "s7cmd clean has been completed."
        );
    }

    if has_warning {
        return Ok(EXIT_CODE_WARNING);
    }

    Ok(EXIT_CODE_SUCCESS)
}

#[cfg(test)]
mod tests {
    use s3rm_rs::parse_from_args;

    use super::*;

    fn config_from_args(args: &[&str]) -> Config {
        Config::try_from(parse_from_args(args).unwrap()).unwrap()
    }

    #[test]
    fn start_tracing_if_necessary_returns_false_when_no_tracing_config() {
        // -qqq drops below all tracing levels → tracing_config is None.
        let config = config_from_args(&[
            "s3rm",
            "-qqq",
            "--target-profile",
            "p",
            "--force",
            "s3://test-bucket",
        ]);
        assert!(config.tracing_config.is_none());
        assert!(!start_tracing_if_necessary(&config));
    }

    // NOTE: a "no --force, non-TTY → SafetyChecker errors out" test would be
    // useful for `pipeline.check_prerequisites()` Err coverage, but in
    // practice s3rm's safety prompt blocks on `stdin.read_line()` instead of
    // short-circuiting on non-TTY — so under interactive `cargo test` the
    // call hangs. Process-level e2e coverage is the right place for this.

    /// With `--force` and an unreachable endpoint, the pipeline runs (skips
    /// confirmation), records listing errors, and `run()` traverses the
    /// `pipeline.has_error()` arm. Returns a non-zero exit code derived from
    /// the recorded errors.
    #[tokio::test]
    async fn run_with_force_against_fake_endpoint_returns_error_code() {
        let config = config_from_args(&[
            "s3rm",
            "--force",
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
        let code = run(config).await.unwrap();
        // Pipeline records listing errors → has_error() true → non-zero exit.
        assert_ne!(code, EXIT_CODE_SUCCESS);
        assert_ne!(code, EXIT_CODE_ABNORMAL_TERMINATION);
    }

    /// `--dry-run` against an unreachable endpoint: SafetyChecker
    /// short-circuits on dry-run, but the listing stage still records errors
    /// when the endpoint is unreachable. Confirms the error-arm path.
    /// (`--force` is mutually exclusive with `--dry-run` as of s3rm-rs
    /// 1.3.6 — dry-run already implies skipping the confirmation prompt.)
    #[tokio::test]
    async fn run_dry_run_against_fake_endpoint_returns_error_code() {
        let config = config_from_args(&[
            "s3rm",
            "--dry-run",
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
        let code = run(config).await.unwrap();
        assert_ne!(code, EXIT_CODE_ABNORMAL_TERMINATION);
    }
}
