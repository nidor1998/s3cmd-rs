//! Implementation of methods on `BatchRunArgs` (defined in `src/cli.rs`).
//! The struct itself lives in `cli.rs` so the integration test that
//! `#[path]`-includes `cli.rs` does not need to also pull in `batch_run`.

use s3util_rs::config::TracingConfig;

use crate::cli::BatchRunArgs;

impl BatchRunArgs {
    /// Mirror of `CommonClientArgs::build_tracing_config()`.
    /// Returns `None` when verbosity is below the lowest tracing level
    /// (e.g. `-qqq`), matching every other subcommand's behaviour.
    ///
    /// `--check-format` forces the level to at least `Info` so the
    /// "format OK" success message is visible at the default
    /// `WarnLevel` — the same pattern `--dry-run` uses for s3util
    /// commands.
    pub fn build_tracing_config(&self) -> Option<TracingConfig> {
        let tracing_level = if self.check_format {
            Some(
                self.verbosity
                    .log_level()
                    .map_or(log::Level::Info, |l| l.max(log::Level::Info)),
            )
        } else {
            self.verbosity.log_level()
        }?;
        Some(TracingConfig {
            tracing_level,
            json_tracing: self.json_tracing,
            aws_sdk_tracing: self.aws_sdk_tracing,
            span_events_tracing: self.span_events_tracing,
            disable_color_tracing: self.disable_color_tracing,
        })
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{BatchRunArgs, Cli, Cmd};

    fn parse_batch_run_args(args: &[&str]) -> BatchRunArgs {
        let cli = Cli::try_parse_from(args).unwrap();
        match cli.command {
            Some(Cmd::BatchRun(a)) => a,
            _ => panic!("expected the batch-run subcommand"),
        }
    }

    #[test]
    fn build_tracing_config_without_check_format_uses_plain_verbosity() {
        let args = parse_batch_run_args(&["s7cmd", "batch-run", "script.txt"]);
        assert!(!args.check_format);
        let config = args
            .build_tracing_config()
            .expect("default verbosity maps to a tracing level");
        assert_eq!(config.tracing_level, log::Level::Warn);
    }

    #[test]
    fn build_tracing_config_below_all_levels_returns_none() {
        // -qqq drops below all tracing levels → None, matching every other
        // subcommand's behaviour.
        let args = parse_batch_run_args(&["s7cmd", "batch-run", "-qqq", "script.txt"]);
        assert!(args.build_tracing_config().is_none());
    }

    #[test]
    fn build_tracing_config_check_format_floors_at_info() {
        // --check-format must lift the default Warn level to Info so the
        // "format OK" success message is visible.
        let args = parse_batch_run_args(&["s7cmd", "batch-run", "--check-format", "script.txt"]);
        let config = args
            .build_tracing_config()
            .expect("check-format always yields a tracing config");
        assert_eq!(config.tracing_level, log::Level::Info);
    }
}
