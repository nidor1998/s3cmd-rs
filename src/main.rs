// s7cmd entry point. The per-subcommand dispatch table lives in
// `dispatch.rs` so it can be reused by `batch_run`.

use std::io::Write;
use std::process::ExitCode;

use clap::FromArgMatches;
use clap_complete::generate;

mod batch_run;
mod clean_bin;
mod cli;
mod dispatch;
mod ls_bin;
mod pipe_safe;
mod sync_bin;
mod util_bin;

use cli::{Cli, Cmd, cli_command};

#[tokio::main]
async fn main() -> ExitCode {
    let cli_args = match Cli::from_arg_matches(&cli_command().get_matches()) {
        Ok(c) => c,
        Err(e) => e.exit(),
    };

    // --auto-complete-shell only works at the top level: generate completions
    // for the whole s7cmd CLI (all subcommands) and exit. The per-subcommand
    // form is stripped in `cli_command()` (each upstream args struct still
    // declares the field, but we clear the long name so the parser rejects
    // `s7cmd <sub> --auto-complete-shell ...`).
    if let Some(shell) = cli_args.auto_complete_shell {
        return print_completion_script(shell);
    }

    // arg_required_else_help on the Cli ensures we always have a command
    // by this point — but the type is Option<Cmd>, so unwrap explicitly.
    let command = cli_args
        .command
        .expect("clap's arg_required_else_help should have prevented this");

    init_tracing_for(&command);

    let exit_code = dispatch::dispatch(command).await;
    ExitCode::from(exit_code as u8)
}

/// Render the shell-completion script for `shell` and print it pipe-safely.
///
/// clap_complete's generators panic on writer errors ("failed to write
/// completion file"), so they never touch stdout directly: the script is
/// rendered into an infallible in-memory buffer and written in one
/// pipe-safe pass. A reader that exits early
/// (`s7cmd --auto-complete-shell bash | head 1`) yields exit 0; any other
/// stdout write error is reported on stderr with exit 1.
fn print_completion_script(shell: clap_complete::shells::Shell) -> ExitCode {
    match pipe_safe::write_all_pipe_safe(&render_completion_script(shell)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Tracing is never initialized on this path; best-effort stderr.
            let _ = writeln!(
                std::io::stderr(),
                "error: failed to write completion script: {e}"
            );
            ExitCode::FAILURE
        }
    }
}

/// Render the completion script for the whole s7cmd CLI into memory.
/// Infallible: clap_complete only fails through its writer, and writes to
/// a `Vec<u8>` cannot fail.
fn render_completion_script(shell: clap_complete::shells::Shell) -> Vec<u8> {
    let mut script = Vec::new();
    generate(shell, &mut cli_command(), "s7cmd", &mut script);
    script
}

/// Initialize the global tracing subscriber from whichever subcommand
/// owns the tracing flags. Called exactly once.
///
/// `BatchRun` brings its own tracing flags. Other `CommonClientArgs`-based
/// subcommands use `args.common.build_tracing_config()` for read-only
/// commands (`get-*`, `head-*`) and `build_tracing_config_dry_run(args.dry_run)`
/// for mutating commands (`rm`, `create-bucket`, all `put-*`, all `delete-*`)
/// so that `--dry-run` forces verbosity to at least info — making the
/// `[dry-run]` log line visible at default `WarnLevel`. `Sync`, `Ls`,
/// `Clean`, `Cp`, and `Mv` initialize their own subscribers from inside
/// `dispatch` (Sync/Ls/Clean read from their own Config; Cp/Mv use
/// `CommonTransferArgs` which has no `build_tracing_config` — the
/// subscriber is installed only after the args are converted into a
/// `s3util_rs::Config`). This function returns early for those variants.
fn init_tracing_for(cmd: &Cmd) {
    use s3util_rs::config::TracingConfig;

    let tc: Option<TracingConfig> = match cmd {
        Cmd::BatchRun(args) => args.build_tracing_config(),
        // sync_bin / ls_bin / clean_bin / cp / mv handle their own subscriber
        // init inside their dispatch arms.
        Cmd::Sync(_) | Cmd::Ls(_) | Cmd::Clean(_) | Cmd::Cp(_) | Cmd::Mv(_) => return,
        Cmd::Rename(args) => args.build_tracing_config_dry_run(args.dry_run),
        Cmd::Rm(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::CreateBucket(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::DeleteBucket(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::HeadBucket(args) => args.common.build_tracing_config(),
        Cmd::HeadObject(args) => args.common.build_tracing_config(),
        Cmd::GetObjectTagging(args) => args.common.build_tracing_config(),
        Cmd::PutObjectTagging(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::DeleteObjectTagging(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::GetObjectAnnotation(args) => args.common.build_tracing_config(),
        Cmd::PutObjectAnnotation(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::DeleteObjectAnnotation(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::ListObjectAnnotations(args) => args.common.build_tracing_config(),
        Cmd::GetBucketTagging(args) => args.common.build_tracing_config(),
        Cmd::PutBucketTagging(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::DeleteBucketTagging(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::GetBucketPolicy(args) => args.common.build_tracing_config(),
        Cmd::PutBucketPolicy(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::DeleteBucketPolicy(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::GetBucketVersioning(args) => args.common.build_tracing_config(),
        Cmd::PutBucketVersioning(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::GetBucketLifecycleConfiguration(args) => args.common.build_tracing_config(),
        Cmd::PutBucketLifecycleConfiguration(args) => {
            args.common.build_tracing_config_dry_run(args.dry_run)
        }
        Cmd::DeleteBucketLifecycleConfiguration(args) => {
            args.common.build_tracing_config_dry_run(args.dry_run)
        }
        Cmd::GetBucketEncryption(args) => args.common.build_tracing_config(),
        Cmd::PutBucketEncryption(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::DeleteBucketEncryption(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::GetBucketCors(args) => args.common.build_tracing_config(),
        Cmd::PutBucketCors(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::DeleteBucketCors(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::GetPublicAccessBlock(args) => args.common.build_tracing_config(),
        Cmd::PutPublicAccessBlock(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::DeletePublicAccessBlock(args) => {
            args.common.build_tracing_config_dry_run(args.dry_run)
        }
        Cmd::GetBucketWebsite(args) => args.common.build_tracing_config(),
        Cmd::PutBucketWebsite(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::DeleteBucketWebsite(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::GetBucketLogging(args) => args.common.build_tracing_config(),
        Cmd::PutBucketLogging(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::GetBucketNotificationConfiguration(args) => args.common.build_tracing_config(),
        Cmd::PutBucketNotificationConfiguration(args) => {
            args.common.build_tracing_config_dry_run(args.dry_run)
        }
        Cmd::GetBucketReplication(args) => args.common.build_tracing_config(),
        Cmd::PutBucketReplication(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::DeleteBucketReplication(args) => {
            args.common.build_tracing_config_dry_run(args.dry_run)
        }
        Cmd::GetBucketAccelerateConfiguration(args) => args.common.build_tracing_config(),
        Cmd::PutBucketAccelerateConfiguration(args) => {
            args.common.build_tracing_config_dry_run(args.dry_run)
        }
        Cmd::GetBucketRequestPayment(args) => args.common.build_tracing_config(),
        Cmd::PutBucketRequestPayment(args) => {
            args.common.build_tracing_config_dry_run(args.dry_run)
        }
        Cmd::GetBucketPolicyStatus(args) => args.common.build_tracing_config(),
        Cmd::RestoreObject(args) => args.common.build_tracing_config_dry_run(args.dry_run),
        Cmd::Presign(args) => args.common.build_tracing_config(),
    };

    if let Some(tc) = tc {
        util_bin::tracing_init::init_tracing(&tc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap_complete::shells::Shell;

    /// The in-memory rendering must produce a real s7cmd completion script
    /// for every supported shell. Rendering to a buffer is what keeps
    /// clap_complete's panicking writer path away from stdout; the
    /// closed-pipe behavior itself is pinned process-level in
    /// `tests/cli_broken_pipe.rs`.
    #[test]
    fn render_completion_script_produces_s7cmd_completions() {
        for shell in [
            Shell::Bash,
            Shell::Zsh,
            Shell::Fish,
            Shell::PowerShell,
            Shell::Elvish,
        ] {
            let script = render_completion_script(shell);
            let text = String::from_utf8(script).expect("completion script must be UTF-8");
            assert!(
                text.contains("s7cmd"),
                "{shell:?} completion script must mention s7cmd"
            );
        }
    }
}
