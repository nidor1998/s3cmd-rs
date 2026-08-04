//! Process-level broken-pipe regression tests. No AWS access is needed:
//! completions, help, and version never leave the process, presign signs
//! locally with dummy static credentials, the batch-run script is empty,
//! and the get-bucket-versioning report comes from a loopback
//! [`MockS3Server`].
//!
//! Each test hands the child a stdout (or stderr) whose read end is already
//! closed, so every write to it fails with `BrokenPipe` from the first
//! byte — the worst case of piping to a consumer that exits without
//! reading, such as `s7cmd --auto-complete-shell bash | head 1` (`head`
//! treats `1` as a file name, fails to open it, and never reads its input)
//! or `head -1`/`grep -q`/a pager closed early. The binary must exit
//! through its normal paths: never panic ("failed printing to stdout:
//! Broken pipe" / "failed to write completion file") and never die of
//! SIGPIPE (which would surface here as `status.code() == None`).
//!
//! The tracing-writer counterpart (`PipeSafeWriter` on a closed stderr
//! during offline-failing commands) is pinned by `cli_closed_stderr.rs`.

mod common;

use common::{MockResponse, MockS3Server, mock_target_args, s7cmd_cmd_clean_env};
use std::process::{Command, Stdio};

/// Run `cmd` with a pre-closed stdout pipe and return (exit_code, stderr).
fn run_with_closed_stdout(cmd: &mut Command) -> (Option<i32>, String) {
    let (reader, writer) = std::io::pipe().expect("failed to create pipe");
    // Close the read end before the child even starts: with no readers left,
    // every stdout write in the child fails with EPIPE immediately.
    drop(reader);

    let output = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::from(writer))
        .stderr(Stdio::piped())
        .output()
        .expect("failed to spawn s7cmd binary");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// Run `cmd` with pre-closed stdout AND stderr pipes; only the exit code is
/// observable. This is the `s7cmd ... 2>&1 | head 1` case, where both
/// streams share one pipe whose reader is already gone.
fn run_with_closed_stdout_and_stderr(cmd: &mut Command) -> Option<i32> {
    let (out_reader, out_writer) = std::io::pipe().expect("failed to create pipe");
    let (err_reader, err_writer) = std::io::pipe().expect("failed to create pipe");
    drop(out_reader);
    drop(err_reader);

    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(out_writer))
        .stderr(Stdio::from(err_writer))
        .status()
        .expect("failed to spawn s7cmd binary")
        .code()
}

fn assert_exits_zero_without_panic(code: Option<i32>, stderr: &str, what: &str) {
    assert!(
        !stderr.contains("panicked"),
        "{what} must not panic on a closed stdout pipe; stderr: {stderr}"
    );
    assert_eq!(
        code,
        Some(0),
        "{what} must exit 0 on a closed stdout pipe (None = killed by \
         SIGPIPE); stderr: {stderr}"
    );
}

/// Completion scripts are rendered to a buffer and written pipe-safely —
/// clap_complete itself would panic ("failed to write completion file") if
/// its generator wrote straight into a closed stdout.
#[test]
fn completion_script_with_closed_stdout_exits_zero() {
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let (code, stderr) =
            run_with_closed_stdout(s7cmd_cmd_clean_env().args(["--auto-complete-shell", shell]));
        assert_exits_zero_without_panic(code, &stderr, &format!("--auto-complete-shell {shell}"));
    }
}

/// The completion script must still be emitted in full when stdout is
/// actually read — the pipe-safe path only swallows a vanished reader, it
/// must not swallow the output itself.
#[test]
fn completion_script_still_prints_when_read() {
    let output = s7cmd_cmd_clean_env()
        .args(["--auto-complete-shell", "bash"])
        .output()
        .expect("failed to spawn s7cmd binary");

    assert!(output.status.success(), "expected exit 0");
    let script = String::from_utf8_lossy(&output.stdout);
    assert!(
        script.contains("_s7cmd"),
        "expected bash completion function in output, got: {script:?}"
    );
}

/// `presign` prints its URL through `pipe_safe::println_pipe_safe` — the
/// same helper every JSON-report subcommand (get-bucket-versioning,
/// head-object, ...) uses — so this pins the fix for the reported
/// `get-bucket-versioning ... | head 1` panic without needing AWS.
/// Credentials are the AWS documentation example values passed as flags;
/// presign resolves them locally and signs without network I/O.
#[test]
fn presign_with_closed_stdout_exits_zero() {
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args([
        "presign",
        "--target-access-key",
        "AKIAIOSFODNN7EXAMPLE",
        "--target-secret-access-key",
        "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        "--target-region",
        "us-east-1",
        "s3://closed-stdout-test-bucket/test-key",
    ])
    // Keep the developer's real AWS setup out of the credential chain.
    .env("AWS_EC2_METADATA_DISABLED", "true")
    .env("AWS_CONFIG_FILE", "/nonexistent/aws-config")
    .env(
        "AWS_SHARED_CREDENTIALS_FILE",
        "/nonexistent/aws-credentials",
    )
    .env_remove("AWS_ACCESS_KEY_ID")
    .env_remove("AWS_SECRET_ACCESS_KEY")
    .env_remove("AWS_SESSION_TOKEN");
    let (code, stderr) = run_with_closed_stdout(&mut cmd);
    assert_exits_zero_without_panic(code, &stderr, "presign");
}

/// The versioning XML the AWS SDK parses into a non-empty report, so the
/// subcommand reaches its `println_pipe_safe` call (an empty config is
/// logged, not printed).
const VERSIONING_ENABLED_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
     <VersioningConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
     <Status>Enabled</Status></VersioningConfiguration>";

fn get_bucket_versioning_cmd(server: &MockS3Server) -> Command {
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.arg("get-bucket-versioning")
        .args(mock_target_args(&server.endpoint_url()))
        .arg("s3://mock-bucket");
    cmd
}

/// The originally reported panic, end to end without AWS:
/// `s7cmd get-bucket-versioning s3://bucket | head 1` — the mock endpoint
/// returns a real versioning config, and the JSON report is printed into a
/// stdout whose reader is already gone. The S3 call has succeeded by then,
/// so the command must exit 0, not panic with
/// `failed printing to stdout: Broken pipe`.
#[test]
fn get_bucket_versioning_report_with_closed_stdout_exits_zero() {
    let server = MockS3Server::start(vec![MockResponse::new(200, VERSIONING_ENABLED_XML)]);
    let (code, stderr) = run_with_closed_stdout(&mut get_bucket_versioning_cmd(&server));
    assert_exits_zero_without_panic(code, &stderr, "get-bucket-versioning");
}

/// Counterpart with a read stdout: the report must still be emitted in
/// full — the pipe-safe path only swallows a vanished reader, never the
/// report itself.
#[test]
fn get_bucket_versioning_report_still_prints_when_read() {
    let server = MockS3Server::start(vec![MockResponse::new(200, VERSIONING_ENABLED_XML)]);
    let (code, stdout, stderr) = common::run(&mut get_bucket_versioning_cmd(&server));
    assert_eq!(code, Some(0), "expected exit 0; stderr: {stderr}");
    assert!(
        stdout.contains("\"Status\"") && stdout.contains("Enabled"),
        "expected the versioning JSON report on stdout, got: {stdout:?}"
    );
}

/// Help and --version are printed by clap, whose `Error::exit` swallows
/// write errors ("Swallow broken pipe errors" in clap itself). Pinned here
/// so a clap regression would be caught.
#[test]
fn help_and_version_with_closed_stdout_exit_zero() {
    let (code, stderr) = run_with_closed_stdout(s7cmd_cmd_clean_env().arg("--help"));
    assert_exits_zero_without_panic(code, &stderr, "--help");

    let (code, stderr) =
        run_with_closed_stdout(s7cmd_cmd_clean_env().args(["get-bucket-versioning", "--help"]));
    assert_exits_zero_without_panic(code, &stderr, "get-bucket-versioning --help");

    let (code, stderr) = run_with_closed_stdout(s7cmd_cmd_clean_env().arg("--version"));
    assert_exits_zero_without_panic(code, &stderr, "--version");
}

/// The batch-run summary is written to stderr after the run completes. With
/// stderr a closed pipe that `eprintln!` panicked, turning an otherwise
/// clean run into exit 101 — even though every line had already executed.
/// The summary is now best-effort: an empty script (stdin at EOF) must
/// still exit 0 with both output pipes closed.
#[test]
fn batch_run_summary_with_closed_stderr_exits_zero() {
    let code = run_with_closed_stdout_and_stderr(s7cmd_cmd_clean_env().args(["batch-run", "-"]));
    assert_eq!(
        code,
        Some(0),
        "an empty batch-run with closed output pipes must exit 0 \
         (101 = the summary line panicked, None = killed by SIGPIPE)"
    );
}

/// Same as above with `--json-tracing`, which routes the summary through
/// `format_json` but down the identical stderr write.
#[test]
fn batch_run_json_summary_with_closed_stderr_exits_zero() {
    let code = run_with_closed_stdout_and_stderr(s7cmd_cmd_clean_env().args([
        "batch-run",
        "--json-tracing",
        "-",
    ]));
    assert_eq!(
        code,
        Some(0),
        "an empty batch-run --json-tracing with closed output pipes must \
         exit 0 (101 = the summary line panicked, None = killed by SIGPIPE)"
    );
}

/// Config errors are reported via `clap::Error::raw(...).print()` with the
/// result ignored, so a closed stderr must not change the exit code: an
/// invalid clean target must still exit 2, not panic or die of SIGPIPE.
#[test]
fn clean_config_error_with_closed_stderr_still_exits_two() {
    let code =
        run_with_closed_stdout_and_stderr(s7cmd_cmd_clean_env().args(["clean", "s3:///prefix"]));
    assert_eq!(
        code,
        Some(2),
        "an invalid clean target with closed output pipes must still exit 2 \
         (None = killed by SIGPIPE, 101 = panicked)"
    );
}
