// Vendored from s3util-rs@0.2.0
//   src/bin/s3util/tracing_init/mod.rs
// Adjustments: stripped #[cfg(test)] mod tests; added s7cmd=<level> to
//              filter strings (vendored tracing events now emit from
//              s7cmd::util_bin::* targets, not s3util/s3util_rs)

use std::env;
use std::io::Write;

use tracing_subscriber::fmt::format::FmtSpan;

use s3util_rs::config::TracingConfig;

const EVENT_FILTER_ENV_VAR: &str = "RUST_LOG";

/// A writer that silently ignores BrokenPipe errors, so piping to
/// head/tail does not produce noisy tracing-subscriber diagnostics.
struct PipeSafeWriter;

impl Write for PipeSafeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match std::io::stderr().write(buf) {
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(buf.len()),
            other => other,
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match std::io::stderr().flush() {
            Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            other => other,
        }
    }
}

pub fn init_tracing(config: &TracingConfig) {
    let fmt_span = if config.span_events_tracing {
        FmtSpan::NEW | FmtSpan::CLOSE
    } else {
        FmtSpan::NONE
    };

    let subscriber_builder = tracing_subscriber::fmt()
        .with_writer(|| PipeSafeWriter)
        .compact()
        .with_target(false)
        .with_ansi(!config.disable_color_tracing)
        .with_span_events(fmt_span);

    let mut show_target = true;
    let tracing_level = config.tracing_level;
    // batch-run installs this subscriber once at the top, then per-line
    // dispatch into clean / ls / sync no-ops (try_init is already set).
    // The filter must therefore include every subcommand crate's target,
    // or per-line INFO events from those crates would be dropped.
    let event_filter = if config.aws_sdk_tracing {
        format!(
            "s7cmd={tracing_level},s3util={tracing_level},s3util_rs={tracing_level},s3rm={tracing_level},s3rm_rs={tracing_level},s3ls={tracing_level},s3ls_rs={tracing_level},s3sync={tracing_level},aws_smithy_runtime={tracing_level},aws_config={tracing_level},aws_sigv4={tracing_level}"
        )
    } else if env::var(EVENT_FILTER_ENV_VAR).is_ok() {
        env::var(EVENT_FILTER_ENV_VAR).unwrap()
    } else {
        show_target = false;
        format!(
            "s7cmd={tracing_level},s3util={tracing_level},s3util_rs={tracing_level},s3rm={tracing_level},s3rm_rs={tracing_level},s3ls={tracing_level},s3ls_rs={tracing_level},s3sync={tracing_level}"
        )
    };

    let subscriber_builder = subscriber_builder
        .with_env_filter(event_filter)
        .with_target(show_target);
    // try_init keeps init_tracing idempotent: batch-run installs the
    // subscriber once at the top, then per-line subcommand dispatch
    // arms call this again (harmlessly).
    let _ = if config.json_tracing {
        subscriber_builder.json().try_init()
    } else {
        subscriber_builder.try_init()
    };
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use s3util_rs::config::TracingConfig;
    use tracing_subscriber::fmt::format::FmtSpan;

    use super::{EVENT_FILTER_ENV_VAR, PipeSafeWriter};

    /// Mirror of `init_tracing` that uses `try_init` so it never panics on
    /// repeated calls during tests where another subscriber may already be set.
    fn try_init_tracing(config: &TracingConfig) {
        let fmt_span = if config.span_events_tracing {
            FmtSpan::NEW | FmtSpan::CLOSE
        } else {
            FmtSpan::NONE
        };

        let subscriber_builder = tracing_subscriber::fmt()
            .with_writer(|| PipeSafeWriter)
            .compact()
            .with_target(false)
            .with_ansi(!config.disable_color_tracing)
            .with_span_events(fmt_span);

        let mut show_target = true;
        let tracing_level = config.tracing_level;
        let event_filter = if config.aws_sdk_tracing {
            format!(
                "s7cmd={tracing_level},s3util={tracing_level},s3util_rs={tracing_level},s3rm={tracing_level},s3rm_rs={tracing_level},s3ls={tracing_level},s3ls_rs={tracing_level},s3sync={tracing_level},aws_smithy_runtime={tracing_level},aws_config={tracing_level},aws_sigv4={tracing_level}"
            )
        } else if std::env::var(EVENT_FILTER_ENV_VAR).is_ok() {
            std::env::var(EVENT_FILTER_ENV_VAR).unwrap()
        } else {
            show_target = false;
            format!(
                "s7cmd={tracing_level},s3util={tracing_level},s3util_rs={tracing_level},s3rm={tracing_level},s3rm_rs={tracing_level},s3ls={tracing_level},s3ls_rs={tracing_level},s3sync={tracing_level}"
            )
        };

        let subscriber_builder = subscriber_builder
            .with_env_filter(event_filter)
            .with_target(show_target);
        if config.json_tracing {
            let _ = subscriber_builder.json().try_init();
        } else {
            let _ = subscriber_builder.try_init();
        }
    }

    #[test]
    fn init_json_tracing() {
        try_init_tracing(&TracingConfig {
            tracing_level: log::Level::Info,
            json_tracing: true,
            aws_sdk_tracing: false,
            span_events_tracing: false,
            disable_color_tracing: false,
        });
    }

    #[test]
    fn init_aws_sdk_tracing() {
        try_init_tracing(&TracingConfig {
            tracing_level: log::Level::Info,
            json_tracing: false,
            aws_sdk_tracing: true,
            span_events_tracing: false,
            disable_color_tracing: false,
        });
    }

    #[test]
    fn init_normal_tracing() {
        unsafe { std::env::remove_var(EVENT_FILTER_ENV_VAR) };

        try_init_tracing(&TracingConfig {
            tracing_level: log::Level::Info,
            json_tracing: false,
            aws_sdk_tracing: false,
            span_events_tracing: false,
            disable_color_tracing: false,
        });
    }

    #[test]
    fn init_span_events_tracing() {
        try_init_tracing(&TracingConfig {
            tracing_level: log::Level::Info,
            json_tracing: false,
            aws_sdk_tracing: true,
            span_events_tracing: true,
            disable_color_tracing: false,
        });
    }

    #[test]
    fn init_disable_color_tracing() {
        try_init_tracing(&TracingConfig {
            tracing_level: log::Level::Info,
            json_tracing: false,
            aws_sdk_tracing: false,
            span_events_tracing: false,
            disable_color_tracing: true,
        });
    }

    #[test]
    fn init_with_env() {
        unsafe { std::env::set_var(EVENT_FILTER_ENV_VAR, "trace") };

        try_init_tracing(&TracingConfig {
            tracing_level: log::Level::Info,
            json_tracing: false,
            aws_sdk_tracing: false,
            span_events_tracing: false,
            disable_color_tracing: true,
        });

        unsafe { std::env::remove_var(EVENT_FILTER_ENV_VAR) };
    }

    #[test]
    fn pipe_safe_writer_writes_normally() {
        let mut writer = PipeSafeWriter;
        // Write to a real (open) stderr — should succeed and return the
        // length of the buffer or a transient OS-level error. Either way,
        // exercises the non-BrokenPipe path of the Write impl.
        let n = writer.write(b"").expect("empty write must succeed");
        assert_eq!(n, 0);
    }

    #[test]
    fn pipe_safe_writer_flush_normally() {
        let mut writer = PipeSafeWriter;
        // Should succeed since stderr is open in test runner.
        writer.flush().expect("flush must succeed");
    }

    /// Tests that exercise the *production* `init_tracing` body. The mirror
    /// helper above intentionally duplicates the production logic so the
    /// global subscriber install can fail safely on repeat calls — but as a
    /// side effect, the production function's own branches (`FmtSpan::NEW |
    /// CLOSE`, the AWS-SDK filter format, the `RUST_LOG` env-var branch)
    /// would otherwise never be exercised in coverage. Calling
    /// `super::init_tracing` here runs the body to completion; only the
    /// `try_init` install at the tail short-circuits if a subscriber is
    /// already set, so these are safe to interleave with the mirror tests.

    #[test]
    fn production_init_span_events_tracing() {
        super::init_tracing(&TracingConfig {
            tracing_level: log::Level::Info,
            json_tracing: false,
            aws_sdk_tracing: false,
            span_events_tracing: true,
            disable_color_tracing: false,
        });
    }

    #[test]
    fn production_init_aws_sdk_tracing() {
        super::init_tracing(&TracingConfig {
            tracing_level: log::Level::Info,
            json_tracing: false,
            aws_sdk_tracing: true,
            span_events_tracing: false,
            disable_color_tracing: false,
        });
    }

    #[test]
    fn production_init_disable_color_tracing() {
        super::init_tracing(&TracingConfig {
            tracing_level: log::Level::Info,
            json_tracing: false,
            aws_sdk_tracing: false,
            span_events_tracing: false,
            disable_color_tracing: true,
        });
    }

    #[test]
    fn production_init_json_tracing() {
        super::init_tracing(&TracingConfig {
            tracing_level: log::Level::Info,
            json_tracing: true,
            aws_sdk_tracing: false,
            span_events_tracing: false,
            disable_color_tracing: true,
        });
    }

    /// Drive the real `PipeSafeWriter` through its non-BrokenPipe arms. The
    /// BrokenPipe arms need stderr to be an actually-closed pipe and are
    /// exercised at process level by the e2e suites. Ported from s3util-rs
    /// PR#25 (post-1.7.1).
    #[test]
    fn pipe_safe_writer_write_and_flush_pass_through() {
        use std::io::Write;

        let mut writer = PipeSafeWriter;
        let written = writer
            .write(b"")
            .expect("empty write to stderr must pass through");
        assert_eq!(written, 0);
        writer.flush().expect("flush to stderr must pass through");
    }
}
