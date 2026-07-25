use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::shells::Shell;
use clap_verbosity_flag::{Verbosity, WarnLevel};
use std::sync::LazyLock;

#[cfg(feature = "version")]
use shadow_rs::shadow;

#[cfg(feature = "version")]
shadow!(build);

#[derive(Parser, Clone, Debug)]
#[cfg_attr(
    feature = "version",
    command(version = format!(
        "{} ({} {}), {}",
        build::PKG_VERSION,
        build::SHORT_COMMIT,
        build::BUILD_TARGET,
        build::RUST_VERSION
    ))
)]
#[cfg_attr(not(feature = "version"), command(version))]
#[command(
    name = "s7cmd",
    about = "Reliable, flexible, and fast command-line tool for Amazon S3",
    arg_required_else_help = true,
    // clap 4.6 has no per-subcommand help_heading in derive, so the grouped
    // listing below is static. The order/names must match the `Cmd` enum
    // variants — `tests/cli_help.rs::top_level_help_*` exercise the basics,
    // but adding/renaming a subcommand requires a manual edit here.
    help_template = "\
{about-with-newline}
{usage-heading} {usage}

Object Operations:
  ls                                    List S3 objects
  cp                                    Copy objects from/to S3 (or S3 to S3)
  mv                                    Move objects from/to S3 (copy then delete source)
  rename                                Rename an S3 object within an Express One Zone bucket
  rm                                    Delete a single S3 object
  restore-object                        Restore an archived S3 object
  presign                               Generate a pre-signed URL for an S3 object (GET only)
  sync                                  Synchronize files between local and S3 (or S3 to S3)
  clean                                 Bulk-delete S3 objects

Object Metadata:
  head-object                           Head an S3 object
  get-object-tagging                    Get an S3 object's tagging
  put-object-tagging                    Put tagging on an S3 object
  delete-object-tagging                 Delete tagging from an S3 object

Object Annotation:
  get-object-annotation                 Download a named annotation payload from an S3 object
  put-object-annotation                 Attach a named annotation payload to an S3 object
  delete-object-annotation              Delete a named annotation from an S3 object
  list-object-annotations               List the annotations of an S3 object and print them as JSON

Bucket Operations:
  create-bucket                         Create an S3 bucket
  delete-bucket                         Delete an S3 bucket
  head-bucket                           Head an S3 bucket

Bucket Tagging:
  get-bucket-tagging                    Get a bucket's tagging
  put-bucket-tagging                    Put tagging on a bucket
  delete-bucket-tagging                 Delete tagging from a bucket

Bucket Policy:
  get-bucket-policy                     Get a bucket's policy
  put-bucket-policy                     Put a bucket policy
  delete-bucket-policy                  Delete a bucket's policy

Bucket Versioning:
  get-bucket-versioning                 Get a bucket's versioning configuration
  put-bucket-versioning                 Put a bucket versioning configuration

Bucket Lifecycle:
  get-bucket-lifecycle-configuration    Get a bucket's lifecycle configuration
  put-bucket-lifecycle-configuration    Put a bucket lifecycle configuration
  delete-bucket-lifecycle-configuration Delete a bucket's lifecycle configuration

Bucket Encryption:
  get-bucket-encryption                 Get a bucket's encryption configuration
  put-bucket-encryption                 Put a bucket encryption configuration
  delete-bucket-encryption              Delete a bucket's encryption configuration

Bucket CORS:
  get-bucket-cors                       Get a bucket's CORS configuration
  put-bucket-cors                       Put a bucket CORS configuration
  delete-bucket-cors                    Delete a bucket's CORS configuration

Bucket Public Access Block:
  get-public-access-block               Get a bucket's public access block configuration
  put-public-access-block               Put a bucket public access block configuration
  delete-public-access-block            Delete a bucket's public access block configuration

Bucket Website:
  get-bucket-website                    Get a bucket's website configuration
  put-bucket-website                    Put a bucket website configuration
  delete-bucket-website                 Delete a bucket's website configuration

Bucket Logging:
  get-bucket-logging                    Get a bucket's logging configuration
  put-bucket-logging                    Put a bucket logging configuration

Bucket Notification:
  get-bucket-notification-configuration Get a bucket's notification configuration
  put-bucket-notification-configuration Put a bucket notification configuration

Bucket Replication:
  get-bucket-replication                Get a bucket's replication configuration
  put-bucket-replication                Put a bucket replication configuration
  delete-bucket-replication             Delete a bucket's replication configuration

Bucket Transfer Acceleration:
  get-bucket-accelerate-configuration   Get a bucket's transfer acceleration configuration
  put-bucket-accelerate-configuration   Put a bucket transfer acceleration configuration

Bucket Request Payment:
  get-bucket-request-payment            Get a bucket's request payment configuration
  put-bucket-request-payment            Put a bucket request payment configuration

Bucket Policy Status:
  get-bucket-policy-status              Get a bucket's policy status (whether it is public)

Batch:
  batch-run                             Run s7cmd commands from a file (or - for stdin)

Other:
  help                                  Print this message or the help of the given subcommand(s)

Options:
{options}{after-help}",
)]
pub struct Cli {
    /// Generate shell completions for s7cmd (all subcommands) and exit.
    ///
    /// Equivalent to `<subcommand> --auto-complete-shell <SHELL>`,
    /// but does not require picking a subcommand.
    #[arg(long, value_enum, value_name = "SHELL", global = false)]
    pub auto_complete_shell: Option<Shell>,

    #[command(subcommand)]
    pub command: Option<Cmd>,
}

/// Upper bound on `batch-run --parallel`. An unbounded worker count has two
/// failure modes: `tokio::sync::Semaphore::new` panics — aborting the whole
/// process with exit 101 — once the value exceeds `usize::MAX >> 3`, and even
/// a large-but-valid value would spawn that many concurrent dispatch tasks,
/// exhausting file descriptors / connections. 1024 concurrent whole-subcommand
/// executions is already far beyond any practical batch workload (each line
/// may itself be a multi-worker transfer), so the cap stays generous while
/// keeping the knob safe. `0` (use all logical CPUs) is always accepted.
pub(crate) const MAX_PARALLEL: usize = 1024;

/// Value parser for `--parallel`. Accepts `0..=MAX_PARALLEL`, rejecting an
/// oversized or non-numeric value at clap parse time (clean exit 2) rather
/// than letting it reach `Semaphore::new` and panic (exit 101). Mirrors the
/// custom-parser style used for `presign --expires-in` upstream.
fn parse_parallel(s: &str) -> Result<usize, String> {
    let n: usize = s
        .parse()
        .map_err(|_| format!("invalid value '{s}': expected an integer in 0..={MAX_PARALLEL}"))?;
    if n > MAX_PARALLEL {
        return Err(format!("--parallel must be no more than {MAX_PARALLEL}"));
    }
    Ok(n)
}

#[derive(clap::Args, Clone, Debug)]
pub struct BatchRunArgs {
    /// Path to a script file with s7cmd commands, or `-` to read from stdin.
    #[arg(value_name = "FILE")]
    pub script: String,

    /// Number of commands to run concurrently. 1 = sequential (default).
    /// 0 = use all logical CPUs. Must be no more than 1024;
    /// an oversized value is rejected at parse time rather than panicking the
    /// tokio semaphore.
    #[arg(long, default_value_t = 1, value_name = "N", value_parser = parse_parallel)]
    pub parallel: usize,

    /// Execute commands as they are read from stdin (no progress bar).
    /// By default, all commands are read first, then executed.
    #[arg(long)]
    pub streaming: bool,

    /// Continue executing remaining commands after any non-zero exit
    /// (failure or warning). By default, the first non-zero exit stops
    /// execution (sequential) or prevents new commands from starting
    /// (parallel). Mutually exclusive with `--max-errors` and
    /// `--continue-on-warning`.
    #[arg(long, conflicts_with_all = ["max_errors", "continue_on_warning"])]
    pub continue_on_error: bool,

    /// Continue executing remaining commands after a per-line warning
    /// (exit codes 3 and 4 — `EXIT_CODE_WARNING` and `EXIT_CODE_NOT_FOUND`).
    /// True failures (any other non-zero exit) still stop the run
    /// according to `--max-errors` (or the default first-failure stop).
    /// Mutually exclusive with `--continue-on-error`.
    #[arg(long, conflicts_with = "continue_on_error")]
    pub continue_on_warning: bool,

    /// Stop spawning new commands once `N` failures have been recorded
    /// (graceful: in-flight commands complete). Must be >= 1. When
    /// omitted, the run stops on the first failure — the same behavior
    /// as no flag at all. Mutually exclusive with `--continue-on-error`.
    /// When combined with `--continue-on-warning`, only true failures
    /// (non-warning non-zero exits) count toward `N`.
    ///
    /// In parallel mode this only stops NEW spawns; in-flight commands
    /// run to completion. When `--parallel` is close to or exceeds the
    /// total line count, every line may already be in flight by the
    /// time the threshold trips, so the threshold has no visible effect.
    /// Send SIGINT to cancel in-flight work.
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(u64).range(1..),
    )]
    pub max_errors: Option<u64>,

    /// Suppress the end-of-run summary line on stderr.
    #[arg(long)]
    pub no_summary: bool,

    /// Suppress the live progress bar on stderr (the end-of-run summary
    /// is still printed unless `--no-summary` is also set). Useful when
    /// stderr is a TTY but you want machine-readable log output —
    /// terminal multiplexers, `script(1)`, some CI runners. Has no
    /// effect with `--streaming` or non-TTY stderr, which already
    /// suppress the bar.
    #[arg(long)]
    pub no_progress: bool,

    /// Only validate the script's format. Stops at the first
    /// problematic line, reports it at error level, and exits 1 — no
    /// command is executed. On success an info-level message is logged.
    /// Verbosity is forced to at least info while this flag is set so
    /// the success message is visible at the default warn level.
    #[arg(long)]
    pub check_format: bool,

    // Tracing flags — same names as every other subcommand's tracing
    // block. AWS auth/endpoint flags are intentionally NOT included
    // (each per-line subcommand brings its own).
    /// Show trace as json format.
    #[arg(long, default_value_t = false, help_heading = "Tracing/Logging")]
    pub json_tracing: bool,

    /// Enable aws sdk tracing.
    #[arg(long, default_value_t = false, help_heading = "Tracing/Logging")]
    pub aws_sdk_tracing: bool,

    /// Show span event tracing.
    #[arg(long, default_value_t = false, help_heading = "Tracing/Logging")]
    pub span_events_tracing: bool,

    /// Disable ANSI terminal colors.
    #[arg(long, default_value_t = false, help_heading = "Tracing/Logging")]
    pub disable_color_tracing: bool,

    #[command(flatten)]
    pub verbosity: Verbosity<WarnLevel>,
}

#[derive(Subcommand, Clone, Debug)]
pub enum Cmd {
    // Object Operations
    /// List S3 objects
    Ls(Box<s3ls_rs::CLIArgs>),
    /// Copy objects from/to S3 (or S3 to S3)
    Cp(s3util_rs::config::args::CpArgs),
    /// Move objects from/to S3 (copy then delete source)
    Mv(s3util_rs::config::args::MvArgs),
    /// Rename an S3 object within an Express One Zone bucket (atomic server-side rename)
    Rename(s3util_rs::config::args::RenameArgs),
    /// Delete a single S3 object
    Rm(s3util_rs::config::args::RmArgs),
    /// Restore an archived S3 object
    RestoreObject(s3util_rs::config::args::RestoreObjectArgs),
    /// Generate a pre-signed URL for an S3 object (GET only)
    Presign(s3util_rs::config::args::PresignArgs),
    /// Synchronize files between local and S3 (or S3 to S3)
    Sync(Box<s3sync::CLIArgs>),
    /// Bulk-delete S3 objects
    Clean(Box<s3rm_rs::CLIArgs>),

    // Object Metadata
    /// Head an S3 object
    HeadObject(s3util_rs::config::args::HeadObjectArgs),
    /// Get an S3 object's tagging
    GetObjectTagging(s3util_rs::config::args::GetObjectTaggingArgs),
    /// Put tagging on an S3 object
    PutObjectTagging(s3util_rs::config::args::PutObjectTaggingArgs),
    /// Delete tagging from an S3 object
    DeleteObjectTagging(s3util_rs::config::args::DeleteObjectTaggingArgs),

    // Object Annotation
    /// Download a named annotation payload from an S3 object
    GetObjectAnnotation(s3util_rs::config::args::GetObjectAnnotationArgs),
    /// Attach a named annotation payload to an S3 object
    PutObjectAnnotation(s3util_rs::config::args::PutObjectAnnotationArgs),
    /// Delete a named annotation from an S3 object
    DeleteObjectAnnotation(s3util_rs::config::args::DeleteObjectAnnotationArgs),
    /// List the annotations of an S3 object and print them as JSON
    ListObjectAnnotations(s3util_rs::config::args::ListObjectAnnotationsArgs),

    // Bucket Operations
    /// Create an S3 bucket
    CreateBucket(s3util_rs::config::args::CreateBucketArgs),
    /// Delete an S3 bucket
    DeleteBucket(s3util_rs::config::args::DeleteBucketArgs),
    /// Head an S3 bucket
    HeadBucket(s3util_rs::config::args::HeadBucketArgs),

    // Bucket Tagging
    /// Get a bucket's tagging
    GetBucketTagging(s3util_rs::config::args::GetBucketTaggingArgs),
    /// Put tagging on a bucket
    PutBucketTagging(s3util_rs::config::args::PutBucketTaggingArgs),
    /// Delete tagging from a bucket
    DeleteBucketTagging(s3util_rs::config::args::DeleteBucketTaggingArgs),

    // Bucket Policy
    /// Get a bucket's policy
    GetBucketPolicy(s3util_rs::config::args::GetBucketPolicyArgs),
    /// Put a bucket policy
    PutBucketPolicy(s3util_rs::config::args::PutBucketPolicyArgs),
    /// Delete a bucket's policy
    DeleteBucketPolicy(s3util_rs::config::args::DeleteBucketPolicyArgs),

    // Bucket Versioning
    /// Get a bucket's versioning configuration
    GetBucketVersioning(s3util_rs::config::args::GetBucketVersioningArgs),
    /// Put a bucket versioning configuration
    PutBucketVersioning(s3util_rs::config::args::PutBucketVersioningArgs),

    // Bucket Lifecycle
    /// Get a bucket's lifecycle configuration
    GetBucketLifecycleConfiguration(s3util_rs::config::args::GetBucketLifecycleConfigurationArgs),
    /// Put a bucket lifecycle configuration
    PutBucketLifecycleConfiguration(s3util_rs::config::args::PutBucketLifecycleConfigurationArgs),
    /// Delete a bucket's lifecycle configuration
    DeleteBucketLifecycleConfiguration(
        s3util_rs::config::args::DeleteBucketLifecycleConfigurationArgs,
    ),

    // Bucket Encryption
    /// Get a bucket's encryption configuration
    GetBucketEncryption(s3util_rs::config::args::GetBucketEncryptionArgs),
    /// Put a bucket encryption configuration
    PutBucketEncryption(s3util_rs::config::args::PutBucketEncryptionArgs),
    /// Delete a bucket's encryption configuration
    DeleteBucketEncryption(s3util_rs::config::args::DeleteBucketEncryptionArgs),

    // Bucket CORS
    /// Get a bucket's CORS configuration
    GetBucketCors(s3util_rs::config::args::GetBucketCorsArgs),
    /// Put a bucket CORS configuration
    PutBucketCors(s3util_rs::config::args::PutBucketCorsArgs),
    /// Delete a bucket's CORS configuration
    DeleteBucketCors(s3util_rs::config::args::DeleteBucketCorsArgs),

    // Bucket Public Access Block
    /// Get a bucket's public access block configuration
    GetPublicAccessBlock(s3util_rs::config::args::GetPublicAccessBlockArgs),
    /// Put a bucket public access block configuration
    PutPublicAccessBlock(s3util_rs::config::args::PutPublicAccessBlockArgs),
    /// Delete a bucket's public access block configuration
    DeletePublicAccessBlock(s3util_rs::config::args::DeletePublicAccessBlockArgs),

    // Bucket Website
    /// Get a bucket's website configuration
    GetBucketWebsite(s3util_rs::config::args::GetBucketWebsiteArgs),
    /// Put a bucket website configuration
    PutBucketWebsite(s3util_rs::config::args::PutBucketWebsiteArgs),
    /// Delete a bucket's website configuration
    DeleteBucketWebsite(s3util_rs::config::args::DeleteBucketWebsiteArgs),

    // Bucket Logging
    /// Get a bucket's logging configuration
    GetBucketLogging(s3util_rs::config::args::GetBucketLoggingArgs),
    /// Put a bucket logging configuration
    PutBucketLogging(s3util_rs::config::args::PutBucketLoggingArgs),

    // Bucket Notification
    /// Get a bucket's notification configuration
    GetBucketNotificationConfiguration(
        s3util_rs::config::args::GetBucketNotificationConfigurationArgs,
    ),
    /// Put a bucket notification configuration
    PutBucketNotificationConfiguration(
        s3util_rs::config::args::PutBucketNotificationConfigurationArgs,
    ),

    // Bucket Replication
    /// Get a bucket's replication configuration
    GetBucketReplication(s3util_rs::config::args::GetBucketReplicationArgs),
    /// Put a bucket replication configuration
    PutBucketReplication(s3util_rs::config::args::PutBucketReplicationArgs),
    /// Delete a bucket's replication configuration
    DeleteBucketReplication(s3util_rs::config::args::DeleteBucketReplicationArgs),

    // Bucket Transfer Acceleration
    /// Get a bucket's transfer acceleration configuration
    GetBucketAccelerateConfiguration(s3util_rs::config::args::GetBucketAccelerateConfigurationArgs),
    /// Put a bucket transfer acceleration configuration
    PutBucketAccelerateConfiguration(s3util_rs::config::args::PutBucketAccelerateConfigurationArgs),

    // Bucket Request Payment
    /// Get a bucket's request payment configuration
    GetBucketRequestPayment(s3util_rs::config::args::GetBucketRequestPaymentArgs),
    /// Put a bucket request payment configuration
    PutBucketRequestPayment(s3util_rs::config::args::PutBucketRequestPaymentArgs),

    // Bucket Policy Status
    /// Get a bucket's policy status (whether the policy makes the bucket public)
    GetBucketPolicyStatus(s3util_rs::config::args::GetBucketPolicyStatusArgs),

    // Batch
    /// Run s7cmd commands from a file (or - for stdin)
    BatchRun(BatchRunArgs),
}

/// Build the s7cmd `Command` with `--auto-complete-shell` hidden on every
/// subcommand. The flag is inherited from each upstream args struct (we
/// cannot remove it), but it's redundant with the top-level
/// `--auto-complete-shell` — both produce the same full-tree completion.
///
/// `hide(true)` alone only suppresses the flag from `--help`. clap_complete
/// does not honor `hide(true)` for args, so we additionally clear the long
/// name to keep it out of generated completion scripts, and clear the env
/// source: upstream declares the arg with `env`, so an exported
/// `AUTO_COMPLETE_SHELL` would otherwise still populate it and fire its
/// clap side effects (`default_value_if` defaulting sync source/target to
/// `s3://ignored`, `required_unless_present` lifting required util targets)
/// even though dispatch ignores the parsed value. With both cleared, the
/// arg id stays and `FromArgMatches` derives `auto_complete_shell: None`;
/// users who want completions use the top-level `--auto-complete-shell`.
#[allow(dead_code)] // used from main.rs; cli_routing integration test includes this file directly
pub fn cli_command() -> clap::Command {
    // The clap `Command` tree is large (~40 subcommands embedding sizable
    // upstream arg structs from s3sync, s3util_rs, etc.) and rebuilding
    // it on every call dominates `batch-run` startup at scale: phase 2 of
    // `run_read_all` calls this once per script line on a single thread,
    // serially, before any AWS work begins. Cache and clone instead — the
    // mutations below are idempotent and don't depend on inputs, so the
    // cached tree is reusable across all callers.
    static CACHED: LazyLock<clap::Command> = LazyLock::new(build_cli_command);
    CACHED.clone()
}

fn build_cli_command() -> clap::Command {
    // Cap help-text wrap width so long flag descriptions stay readable on
    // wide terminals (otherwise clap wraps at the detected terminal width
    // and produces 200+ char lines). Requires the clap `wrap_help`
    // feature for terminal-size detection. Propagates to subcommands.
    let mut cmd = Cli::command().max_term_width(100);
    let names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    for name in names {
        cmd = cmd.mut_subcommand(name, |sub| {
            let has_flag = sub
                .get_arguments()
                .any(|a| a.get_id().as_str() == "auto_complete_shell");
            let sub = if has_flag {
                sub.mut_arg("auto_complete_shell", |a| {
                    a.hide(true)
                        .long(None::<&'static str>)
                        .env(None::<&'static str>)
                })
            } else {
                sub
            };
            hide_credential_env_values(sub)
        });
    }
    cmd
}

/// Mark every env-backed credential argument `hide_env_values` so a secret
/// exported through the environment is not echoed into `--help` output
/// (CI logs, terminal scrollback, pasted bug reports). clap renders the
/// current value of an arg's env var into help text by default; the env var
/// *name* stays visible, only the value is suppressed.
///
/// Upstream fixes this at the derive level: s3sync 1.61.0, s3rm-rs 1.5.0,
/// s3ls-rs 1.2.0, and s3util-rs 1.9.0 (the releases s7cmd pins) all mark
/// their credential args `hide_env_values = true`, so this pass is an
/// idempotent no-op today. It is kept as a guard: a future upstream release
/// that adds a credential arg without the marking (or drops it) is re-hidden
/// here rather than leaking secrets into `--help` output.
///
/// The id predicate matches the exact set upstream hides: access keys,
/// secret access keys, session tokens (`*access_key*`, `*session_token*`)
/// and SSE-C key material (`*sse_c_key*`, which also covers the
/// `*_sse_c_key_md5` digests).
fn hide_credential_env_values(mut sub: clap::Command) -> clap::Command {
    let sensitive_ids: Vec<clap::Id> = sub
        .get_arguments()
        .filter(|a| {
            let id = a.get_id().as_str();
            (id.contains("access_key") || id.contains("session_token") || id.contains("sse_c_key"))
                && a.get_env().is_some()
        })
        .map(|a| a.get_id().clone())
        .collect();
    for id in sensitive_ids {
        sub = sub.mut_arg(id, |a| a.hide_env_values(true));
    }
    sub
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `cli_command()` is now backed by a `LazyLock` cache that returns a
    /// clone per call. This test guards the most basic correctness
    /// invariant after that change: parsing the same argv twice through
    /// successive calls must succeed and produce the same matched
    /// subcommand. A regression here (e.g. accidentally returning a
    /// shared `Arc<Command>` and consuming it) would surface as the
    /// second call failing or giving stale state.
    #[test]
    fn cli_command_repeat_calls_parse_identically() {
        let argv = ["s7cmd", "head-bucket", "s3://b"];
        let m1 = cli_command().try_get_matches_from(argv).unwrap();
        let m2 = cli_command().try_get_matches_from(argv).unwrap();
        assert_eq!(m1.subcommand_name(), Some("head-bucket"));
        assert_eq!(m2.subcommand_name(), Some("head-bucket"));
    }

    /// Each `cli_command()` call returns an independent clone of the
    /// cached tree. `try_get_matches_from` consumes the `Command`, so if
    /// the cache were ever reused as a borrow rather than cloned, the
    /// second call would see a half-consumed tree. Drive both happy and
    /// error paths in close succession to catch a stateful regression.
    #[test]
    fn cli_command_clones_are_independent_across_calls() {
        // First call: a parse error (unknown subcommand). Must not poison
        // the cache for the next caller.
        let err = cli_command()
            .try_get_matches_from(["s7cmd", "no-such-command"])
            .unwrap_err();
        assert!(err.kind() == clap::error::ErrorKind::InvalidSubcommand);
        // Second call: a clean parse, fresh clone.
        let m = cli_command()
            .try_get_matches_from(["s7cmd", "head-bucket", "s3://b"])
            .unwrap();
        assert_eq!(m.subcommand_name(), Some("head-bucket"));
    }

    /// `parse_and_validate`'s `thread::scope` fan-out calls
    /// `cli_command()` from many worker threads concurrently. `LazyLock`
    /// guarantees synchronized initialization, but the test fixes the
    /// invariant in code so accidental migration to a non-thread-safe
    /// primitive (e.g. `OnceCell`) breaks here, not at runtime under load.
    #[test]
    fn cli_command_concurrent_calls_succeed() {
        let argv: [&str; 3] = ["s7cmd", "head-bucket", "s3://b"];
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..16)
                .map(|_| {
                    s.spawn(move || {
                        let m = cli_command().try_get_matches_from(argv).unwrap();
                        assert_eq!(m.subcommand_name(), Some("head-bucket"));
                    })
                })
                .collect();
            for h in handles {
                h.join().expect("worker panicked");
            }
        });
    }

    /// The post-build mutations applied inside `build_cli_command` strip
    /// the long form of `--auto-complete-shell` on every subcommand and
    /// hide it from `--help`. With the `LazyLock` cache those mutations
    /// happen exactly once and must persist on every clone — verify
    /// directly via clap introspection so a regression surfaces here
    /// rather than as an unexpected acceptance of `cp
    /// --auto-complete-shell <SHELL>` at runtime. Spot-check a few
    /// representative subcommands rather than every one — the build is
    /// uniform so coverage of the loop body is what matters.
    #[test]
    fn cli_command_subcommand_auto_complete_shell_long_form_is_cleared() {
        let cmd = cli_command();
        for name in ["cp", "mv", "sync", "head-bucket"] {
            let sub = cmd
                .find_subcommand(name)
                .unwrap_or_else(|| panic!("subcommand `{name}` missing"));
            let arg = sub
                .get_arguments()
                .find(|a| a.get_id().as_str() == "auto_complete_shell")
                .unwrap_or_else(|| panic!("`{name}` lost its `auto_complete_shell` arg"));
            assert!(
                arg.get_long().is_none(),
                "`{name}` still exposes a long form for --auto-complete-shell",
            );
            assert!(
                arg.is_hide_set(),
                "`{name}` does not hide --auto-complete-shell from --help",
            );
        }
    }

    /// The top-level `--auto-complete-shell` is intentionally left alone
    /// (only the per-subcommand inherited copies are stripped). Guard
    /// that the mutation loop didn't accidentally affect the Cli
    /// root-level arg of the same id.
    #[test]
    fn cli_command_top_level_auto_complete_shell_long_form_preserved() {
        let cmd = cli_command();
        let arg = cmd
            .get_arguments()
            .find(|a| a.get_id().as_str() == "auto_complete_shell")
            .expect("top-level --auto-complete-shell is missing");
        assert_eq!(arg.get_long(), Some("auto-complete-shell"));
    }

    /// Every env-backed credential argument across every subcommand must be
    /// `hide_env_values` so `--help` never echoes a secret exported through
    /// the environment. Mirrors the upstream guard test added by s3util-rs
    /// PR#25. The pinned upstream releases (s3sync 1.61.0, s3rm-rs 1.5.0,
    /// s3ls-rs 1.2.0, s3util-rs 1.9.0) now carry their own
    /// `hide_env_values = true` markings; `hide_credential_env_values` backs
    /// them up so a future upstream credential arg that misses the marking
    /// still cannot leak.
    #[test]
    fn credential_args_hide_env_values_in_every_subcommand() {
        fn check(cmd: &clap::Command, path: &str, checked: &mut usize) {
            for arg in cmd.get_arguments() {
                let id = arg.get_id().as_str();
                let sensitive = id.contains("access_key")
                    || id.contains("session_token")
                    || id.contains("sse_c_key");
                if sensitive && arg.get_env().is_some() {
                    *checked += 1;
                    assert!(
                        arg.is_hide_env_values_set() || arg.is_hide_env_set(),
                        "{path} --{id}: env-backed credential arg must hide its env value in help output"
                    );
                }
            }
            for sub in cmd.get_subcommands() {
                check(sub, &format!("{path} {}", sub.get_name()), checked);
            }
        }

        let cmd = cli_command();
        let mut checked = 0;
        check(&cmd, cmd.get_name(), &mut checked);
        // sync and cp/mv alone contribute 10 credential args each, and the
        // ~40 CommonClientArgs-based subcommands 3 each; a low count means
        // the walk (or the mutation loop) went wrong.
        assert!(checked >= 100, "only {checked} credential args found");
    }

    /// The mutation must reach the args embedded from every upstream crate,
    /// not only s3util-rs. Spot-check one representative credential arg per
    /// upstream parser: sync (s3sync), clean (s3rm-rs), ls (s3ls-rs), and
    /// cp (s3util-rs) — including the SSE-C key material on sync/cp.
    #[test]
    fn credential_args_hidden_for_each_upstream_parser() {
        let cmd = cli_command();
        for (sub_name, arg_id) in [
            ("sync", "source_access_key"),
            ("sync", "target_sse_c_key"),
            ("sync", "target_sse_c_key_md5"),
            ("clean", "target_secret_access_key"),
            ("ls", "target_session_token"),
            ("cp", "source_sse_c_key"),
            ("mv", "target_access_key"),
            ("rename", "source_session_token"),
            ("head-object", "source_sse_c_key_md5"),
        ] {
            let sub = cmd
                .find_subcommand(sub_name)
                .unwrap_or_else(|| panic!("subcommand `{sub_name}` missing"));
            let arg = sub
                .get_arguments()
                .find(|a| a.get_id().as_str() == arg_id)
                .unwrap_or_else(|| panic!("`{sub_name}` lost its `{arg_id}` arg"));
            assert!(
                arg.is_hide_env_values_set(),
                "`{sub_name} --{arg_id}` must hide its env value in help output",
            );
        }
    }

    // ---- --parallel value parser (MAX_PARALLEL cap) ----
    //
    // An unbounded `--parallel` reaches `tokio::sync::Semaphore::new`, which
    // panics (aborting the process, exit 101) once the value exceeds
    // `usize::MAX >> 3`. `parse_parallel` rejects out-of-range input at clap
    // parse time (exit 2) instead.

    #[test]
    fn parse_parallel_accepts_zero_one_and_max() {
        // 0 = "all logical CPUs", 1 = sequential, MAX_PARALLEL = the ceiling.
        assert_eq!(parse_parallel("0"), Ok(0));
        assert_eq!(parse_parallel("1"), Ok(1));
        assert_eq!(parse_parallel(&MAX_PARALLEL.to_string()), Ok(MAX_PARALLEL));
    }

    #[test]
    fn parse_parallel_rejects_just_over_max() {
        let err = parse_parallel(&(MAX_PARALLEL + 1).to_string()).unwrap_err();
        assert!(err.contains("no more than"), "got: {err}");
    }

    #[test]
    fn parse_parallel_rejects_semaphore_panic_range() {
        // tokio's Semaphore::new panics above `usize::MAX >> 3`; the value one
        // past that boundary — and `usize::MAX` itself — must be rejected here
        // rather than ever reaching `Semaphore::new`.
        let boundary = (usize::MAX >> 3).wrapping_add(1);
        assert!(parse_parallel(&boundary.to_string()).is_err());
        assert!(parse_parallel(&usize::MAX.to_string()).is_err());
    }

    #[test]
    fn parse_parallel_rejects_overflow_and_non_numeric() {
        // Beyond usize::MAX → clean parse error (no overflow/panic).
        assert!(parse_parallel("99999999999999999999999999").is_err());
        assert!(parse_parallel("abc").is_err());
        assert!(parse_parallel("").is_err());
        assert!(parse_parallel("-1").is_err());
    }
}
