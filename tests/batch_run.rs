use assert_cmd::Command;
use predicates::prelude::*;
use std::io::Write;

/// Empty stdin → exits 0, prints summary "0 succeeded, 0 failed".
#[test]
fn batch_run_empty_stdin_succeeds() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin("")
        .assert()
        .success()
        .stderr(predicate::str::contains("0 succeeded, 0 failed"));
}

#[test]
fn batch_run_no_summary_suppresses_summary() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--no-summary", "-"])
        .write_stdin("")
        .assert()
        .success()
        .stderr(predicate::str::contains("batch-run:").not());
}

/// `--json-tracing` switches the trailing summary line to a JSON object
/// (and, separately, suppresses the live progress bar — covered by
/// `Progress::should_show` unit tests in src). Empty-stdin is enough to
/// exercise the summary path without needing an S3 endpoint.
#[test]
fn batch_run_json_tracing_emits_json_summary() {
    let assert = Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--json-tracing", "-"])
        .write_stdin("")
        .assert()
        .success();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    let summary_line = stderr
        .lines()
        .find(|l| l.contains("\"summary\":\"batch-run\""))
        .unwrap_or_else(|| panic!("no JSON summary line in stderr: {stderr}"));
    let v: serde_json::Value = serde_json::from_str(summary_line).unwrap();
    assert_eq!(v["summary"], "batch-run");
    assert_eq!(v["succeeded"], 0);
    assert_eq!(v["failed"], 0);
    assert_eq!(v["skipped"], 0);
    assert!(v["elapsed_seconds"].is_number());
}

/// `--json-tracing --no-summary` still suppresses the trailing summary —
/// `--no-summary` wins, no JSON summary line is emitted.
#[test]
fn batch_run_json_tracing_with_no_summary_emits_nothing() {
    let assert = Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--json-tracing", "--no-summary", "-"])
        .write_stdin("")
        .assert()
        .success();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        !stderr.contains("\"summary\":\"batch-run\""),
        "JSON summary should be suppressed; stderr={stderr}"
    );
    assert!(
        !stderr.contains("batch-run:"),
        "human summary should be suppressed; stderr={stderr}"
    );
}

#[test]
fn batch_run_rejects_nested_batch_run_line() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin("batch-run -\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("nested batch-run"));
}

#[test]
fn batch_run_rejects_per_line_tracing_flag() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin("head-bucket --aws-sdk-tracing s3://b\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("tracing flags are not allowed"));
}

#[test]
fn batch_run_rejects_stdio_cp_target() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin("cp s3://bucket/key -\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("stdin/stdout"));
}

#[test]
fn batch_run_parses_blank_and_comment_lines() {
    // The lines below all skip (blank, comment) — net result: 0 commands run.
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin("\n# this is a comment\n   \n# another\n")
        .assert()
        .success()
        .stderr(predicate::str::contains("0 succeeded, 0 failed"));
}

#[test]
fn batch_run_parse_error_includes_line_number() {
    // Malformed quoting → parse error mentions line 2 (structured field line=2).
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--disable-color-tracing", "-"])
        .write_stdin("# ok\ncp \"unterminated\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("line=2"));
}

#[test]
fn batch_run_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--parallel"))
        .stdout(predicate::str::contains("--streaming"))
        .stdout(predicate::str::contains("--continue-on-error"))
        .stdout(predicate::str::contains("--max-errors"))
        .stdout(predicate::str::contains("--check-format"));
}

#[test]
fn top_level_help_lists_batch_run() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("batch-run"));
}

// ---- streaming-mode coverage ----

#[test]
fn batch_run_streaming_empty_stdin_succeeds() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--streaming", "-"])
        .write_stdin("")
        .assert()
        .success()
        .stderr(predicate::str::contains("0 succeeded, 0 failed"));
}

#[test]
fn batch_run_streaming_rejects_per_line_tracing_flag() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--streaming", "-"])
        .write_stdin("head-bucket --aws-sdk-tracing s3://b\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("tracing flags are not allowed"));
}

#[test]
fn batch_run_streaming_parses_blank_and_comment_lines() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--streaming", "-"])
        .write_stdin("\n# comment\n   \n")
        .assert()
        .success()
        .stderr(predicate::str::contains("0 succeeded, 0 failed"));
}

// ---- invalid-config handling: validate-time errors become per-line
// failures (exit 2) that count toward `--max-errors` ----

/// A malformed `sync` line (local-to-local without
/// `--allow-both-local-storage`) is caught at validate time and surfaced
/// as a per-line `Invalid` failure (exit 2). Earlier lines run normally;
/// the failure trips the default `--max-errors=1`, so trailing lines are
/// skipped. This lets `--max-errors N` and `--continue-on-error` apply
/// to validate failures the same way they apply to runtime failures.
/// (Previous behavior: validate failure aborted the whole run before any
/// dispatch — even earlier valid lines never ran.)
#[test]
fn batch_run_invalid_sync_config_counts_as_per_line_failure() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--disable-color-tracing", "-"])
        .write_stdin(concat!(
            "create-bucket --dry-run s3://b1\n",
            "sync /tmp/nonexistent-src /tmp/nonexistent-dst\n",
            "create-bucket --dry-run s3://b3\n",
        ))
        .assert()
        .failure()
        // Line 1 ran (dry-run create-bucket succeeds), line 2's
        // validate-time error becomes an Invalid failure logged at
        // error level, line 3 is skipped due to default --max-errors=1.
        .stderr(predicate::str::contains("line=2"))
        .stderr(predicate::str::contains(
            "1 succeeded, 1 failed, 0 warnings, 1 skipped",
        ));
}

// ---- 16 KiB per-line cap ----

const MAX_LINE_LEN: usize = 16 * 1024;

/// A line at exactly the 16 KiB cap (a comment of MAX_LINE_LEN bytes plus
/// `\n`) must be accepted. Comments are skipped, so the run finishes with
/// `0 succeeded, 0 failed`.
#[test]
fn batch_run_accepts_line_at_16kib_cap() {
    let mut input = String::with_capacity(MAX_LINE_LEN + 1);
    input.push('#');
    input.extend(std::iter::repeat_n('x', MAX_LINE_LEN - 1));
    input.push('\n');
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin(input)
        .assert()
        .success()
        .stderr(predicate::str::contains("0 succeeded, 0 failed"));
}

/// A line one byte over the cap must be rejected with a read error
/// pointing at the offending line. Read-all mode bails before any
/// dispatch, so no summary is printed.
#[test]
fn batch_run_rejects_line_over_16kib_cap_read_all() {
    let mut input = String::with_capacity(MAX_LINE_LEN + 3);
    input.push('#');
    input.extend(std::iter::repeat_n('x', MAX_LINE_LEN));
    input.push('\n');
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin(input)
        .assert()
        .failure()
        .stderr(predicate::str::contains("line 1"))
        .stderr(predicate::str::contains("exceeds"));
}

/// Same in streaming mode. The reader returns `Err`, the executor
/// drains, and the summary line is still printed.
#[test]
fn batch_run_rejects_line_over_16kib_cap_streaming() {
    let mut input = String::with_capacity(MAX_LINE_LEN + 3);
    input.push('#');
    input.extend(std::iter::repeat_n('x', MAX_LINE_LEN));
    input.push('\n');
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--streaming", "-"])
        .write_stdin(input)
        .assert()
        .failure()
        .stderr(predicate::str::contains("line 1"))
        .stderr(predicate::str::contains("exceeds"));
}

/// A malformed `ls` config (e.g. `--recursive` in bucket-listing mode)
/// must NOT call `std::process::exit` mid-batch. dispatch must convert it
/// to exit code 2 so the executor records the failure and prints a
/// summary. Regression test for the previous behavior where Ls/Clean
/// went through `load_config_exit_if_err`, killing the entire batch
/// process and bypassing the summary.
#[test]
fn batch_run_invalid_ls_config_does_not_kill_process() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--continue-on-error", "-"])
        .write_stdin(concat!(
            "create-bucket --dry-run s3://b1\n",
            "ls --recursive\n",
            "create-bucket --dry-run s3://b3\n",
        ))
        .assert()
        .failure()
        // The summary line is the load-bearing assertion: its presence
        // proves the batch finished cleanly instead of being killed.
        .stderr(predicate::str::contains("2 succeeded, 1 failed"));
}

// ---- file-source coverage ----

/// Missing positional → clap parse error (mirrors put-bucket-policy).
#[test]
fn batch_run_requires_script_positional() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

/// A non-existent file path is rejected with the path in the error and
/// no batch is started. Same shape as `put-bucket-policy` reading from a
/// missing file.
#[test]
fn batch_run_missing_file_errors() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "/nonexistent/s7cmd-batch-run-script.txt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "/nonexistent/s7cmd-batch-run-script.txt",
        ));
}

#[test]
fn batch_run_reads_from_file_in_read_all_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("script.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "# comment").unwrap();
    writeln!(f, "create-bucket --dry-run s3://b1").unwrap();
    writeln!(f, "create-bucket --dry-run s3://b2").unwrap();
    drop(f);

    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", path.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("2 succeeded, 0 failed"));
}

#[test]
fn batch_run_reads_from_file_in_streaming_mode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("script.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "create-bucket --dry-run s3://b1").unwrap();
    writeln!(f, "create-bucket --dry-run s3://b2").unwrap();
    drop(f);

    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--streaming", path.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("2 succeeded, 0 failed"));
}

// ---- --check-format coverage ----

/// A clean script reports "format OK (<path>)" at info level
/// (verbosity bumped to info by --check-format itself), exits 0,
/// and runs no commands — the absence of a `[dry-run]` line confirms
/// no dispatch happened. The source label echoes the file path the
/// user passed so the message is unambiguous when several scripts
/// are checked in a row.
#[test]
fn batch_run_check_format_reports_ok_for_valid_script() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("script.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "# this is a comment").unwrap();
    writeln!(f, "head-bucket s3://b1").unwrap();
    writeln!(f, "create-bucket --dry-run s3://b2").unwrap();
    drop(f);

    let path_str = path.to_str().unwrap();
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "--disable-color-tracing",
            path_str,
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("batch-run format OK"))
        .stderr(predicate::str::contains(format!(
            "source=\"{}\"",
            path_str.replace('\\', "\\\\")
        )))
        // No execution: no [dry-run] log, no run summary.
        .stderr(predicate::str::contains("[dry-run]").not())
        .stderr(predicate::str::contains("ok, ").not());
}

/// Stdin success path uses the literal label `stdin` (not `-`) so the
/// log line reads naturally. Regression test for the trivial-but-easy-
/// to-break source-label substitution in `run_check_format`.
#[test]
fn batch_run_check_format_reports_ok_for_stdin_uses_stdin_label() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "--disable-color-tracing",
            "-",
        ])
        .write_stdin("head-bucket s3://b1\n")
        .assert()
        .success()
        .stderr(predicate::str::contains("batch-run format OK"))
        .stderr(predicate::str::contains("source=\"stdin\""))
        // The literal `-` should not leak into the source label.
        .stderr(predicate::str::contains("source=\"-\"").not());
}

/// Reproduces the `s7cmd batch-run /etc/hosts ... | <piped-data>`
/// situation: the user piped commands into stdin but passed a file
/// path as the script positional, so batch-run validated the file
/// (not stdin). The per-line error must include the file path so the
/// user immediately sees which source is being read — that's the
/// signal that points back at "I forgot `-`". Also asserts that
/// clap's verbose `Usage:` / `For more information…` trailers and
/// the doubled `parse error: error:` artefact have been stripped.
#[test]
fn batch_run_check_format_error_includes_source_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hosts.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    // Mimic /etc/hosts — first non-comment line begins with an IP.
    writeln!(f, "# the loopback address").unwrap();
    writeln!(f, "127.0.0.1\tlocalhost").unwrap();
    drop(f);

    let path_str = path.to_str().unwrap();
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "--disable-color-tracing",
            path_str,
        ])
        .assert()
        .failure()
        // Structured fields: source, line=2, and the clap error reason.
        .stderr(predicate::str::contains(format!(
            "source=\"{}\"",
            path_str.replace('\\', "\\\\")
        )))
        .stderr(predicate::str::contains("line=2"))
        .stderr(predicate::str::contains(
            "unrecognized subcommand '127.0.0.1'",
        ))
        // clap's verbose tail must not leak into the error log.
        .stderr(predicate::str::contains("Usage:").not())
        .stderr(predicate::str::contains("For more information").not())
        // And the doubled `parse error: error:` would-be artefact is gone.
        .stderr(predicate::str::contains("parse error: error:").not());
}

/// Stdin input prefixes per-line errors with the literal `stdin`
/// label (not `-`).
#[test]
fn batch_run_check_format_error_uses_stdin_label_for_dash() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "--disable-color-tracing",
            "-",
        ])
        .write_stdin("127.0.0.1\tlocalhost\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("source=\"stdin\""))
        .stderr(predicate::str::contains("line=1"))
        .stderr(predicate::str::contains(
            "unrecognized subcommand '127.0.0.1'",
        ));
}

/// Stops at the first problematic line — only that line's error is
/// logged, later bad lines are not reported, and no "format OK"
/// message is emitted. The error line must include the script path.
#[test]
fn batch_run_check_format_stops_at_first_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("script.txt");
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "head-bucket s3://b1").unwrap(); // valid
    writeln!(f, "batch-run -").unwrap(); // invalid (nested batch-run)
    writeln!(f, "cp s3://b/k -").unwrap(); // invalid (stdio)
    writeln!(f, "another-bad-line").unwrap(); // invalid (unknown subcommand)
    drop(f);

    let path_str = path.to_str().unwrap();
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "--disable-color-tracing",
            path_str,
        ])
        .assert()
        .failure()
        // Structured fields: source, line=2, and the validate reason.
        .stderr(predicate::str::contains(format!(
            "source=\"{}\"",
            path_str.replace('\\', "\\\\")
        )))
        .stderr(predicate::str::contains("line=2"))
        .stderr(predicate::str::contains("nested batch-run"))
        // Walk stopped at line 2: line 3 / line 4 must NOT appear.
        .stderr(predicate::str::contains("line=3").not())
        .stderr(predicate::str::contains("line=4").not())
        .stderr(predicate::str::contains("stdin/stdout").not())
        .stderr(predicate::str::contains("format OK").not());
}

/// `--check-format -` reads the script from stdin and behaves the same.
/// Per-line errors are prefixed with `stdin:`.
#[test]
fn batch_run_check_format_reads_from_stdin() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "--disable-color-tracing",
            "-",
        ])
        .write_stdin("head-bucket s3://b1\nbatch-run -\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("source=\"stdin\""))
        .stderr(predicate::str::contains("line=2"))
        .stderr(predicate::str::contains("nested batch-run"))
        .stderr(predicate::str::contains("format OK").not());
}

/// On a check-format error exit, stdin is drained to EOF so an
/// upstream producer (xargs, a shell loop, etc.) can finish writing
/// without getting SIGPIPE on its next write. We can't directly
/// observe the absence of SIGPIPE from outside, so the assertion is
/// behavioural: pipe more bytes than fit in any reasonable kernel
/// pipe buffer (default ~64 KiB on Linux/macOS), trigger a check-
/// format error, and require the run to exit cleanly with the
/// expected exit code instead of hanging on a back-pressured write.
#[test]
fn batch_run_check_format_error_drains_unread_stdin() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hosts.txt");
    // Trigger an immediate parse error on line 1 so check-format
    // bails before doing anything else.
    std::fs::write(&path, "127.0.0.1\tlocalhost\n").unwrap();

    // 1 MiB of "well-formed but never read" piped data — well past
    // the kernel pipe buffer, so without a drain the producer's
    // write would block (and assert_cmd's 30-second default would
    // surface as a hang/timeout).
    let payload: String =
        std::iter::repeat_n("create-bucket --dry-run s3://x\n", 32 * 1024).collect();

    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--check-format", path.to_str().unwrap()])
        .write_stdin(payload)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unrecognized subcommand '127.0.0.1'",
        ));
}

/// Same scenario via a missing-file error: the open failure is
/// reported, stdin is drained, and the run exits cleanly.
#[test]
fn batch_run_check_format_missing_file_drains_unread_stdin() {
    let payload: String =
        std::iter::repeat_n("create-bucket --dry-run s3://x\n", 32 * 1024).collect();

    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "/nonexistent/s7cmd-drain-test.txt",
        ])
        .write_stdin(payload)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "/nonexistent/s7cmd-drain-test.txt",
        ));
}

/// A missing file is reported at error level and exits non-zero.
#[test]
fn batch_run_check_format_missing_file_errors() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "/nonexistent/s7cmd-check-format.txt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "/nonexistent/s7cmd-check-format.txt",
        ));
}

// ---- --max-errors coverage ----

/// `--max-errors 2` keeps running past the first failure and stops only
/// after the second. With the input below: `ls --recursive` fails at
/// dispatch (line 2), `create-bucket --dry-run` succeeds (line 3),
/// `ls --recursive` fails again (line 4) → threshold reached, line 5
/// is skipped.
#[test]
fn batch_run_max_errors_two_stops_after_second_failure() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--max-errors", "2", "-"])
        .write_stdin(concat!(
            "create-bucket --dry-run s3://b1\n",
            "ls --recursive\n",
            "create-bucket --dry-run s3://b2\n",
            "ls --recursive\n",
            "create-bucket --dry-run s3://b3\n",
        ))
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "2 succeeded, 2 failed, 0 warnings, 1 skipped",
        ));
}

/// `--max-errors` is mutually exclusive with `--continue-on-error` —
/// clap rejects the combination at parse time.
#[test]
fn batch_run_max_errors_conflicts_with_continue_on_error() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--continue-on-error", "--max-errors", "3", "-"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

/// `--max-errors 0` is rejected by clap's value-parser range.
#[test]
fn batch_run_max_errors_zero_rejected() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--max-errors", "0", "-"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

/// `--max-errors N` covers parse-time failures too — not just runtime
/// failures from `dispatch()`. Reproduces the original user report: a
/// script of 5 lines all with a typo'd flag (`--server-side-copy2`).
/// Each line fails clap parsing → exit 2 → counts toward the threshold.
/// Threshold of 3 should let the first 3 lines fail and skip the
/// remaining 2.
#[test]
fn batch_run_max_errors_covers_clap_parse_failures() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--max-errors", "3", "-"])
        .write_stdin(concat!(
            "cp --server-side-copy2 s3://b/a s3://b/backup/\n",
            "cp --server-side-copy2 s3://b/b s3://b/backup/\n",
            "cp --server-side-copy2 s3://b/c s3://b/backup/\n",
            "cp --server-side-copy2 s3://b/d s3://b/backup/\n",
            "cp --server-side-copy2 s3://b/e s3://b/backup/\n",
        ))
        .assert()
        .failure()
        // Three Invalid lines hit before the threshold trips; the last
        // two are skipped without being parsed.
        .stderr(predicate::str::contains(
            "0 succeeded, 3 failed, 0 warnings, 2 skipped",
        ))
        .stderr(predicate::str::contains("server-side-copy2"));
}

/// Without `--max-errors`, the historical fail-fast behaviour is
/// unchanged: the first failure stops the run and the rest are
/// skipped (covered already by `batch_run_invalid_sync_config_*`,
/// but assert it explicitly here so any future regression in the
/// default-mapping helper trips this test).
#[test]
fn batch_run_default_is_fail_fast() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin(concat!(
            "create-bucket --dry-run s3://b1\n",
            "ls --recursive\n",
            "create-bucket --dry-run s3://b2\n",
        ))
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "1 succeeded, 1 failed, 0 warnings, 1 skipped",
        ));
}

// ---- per-line start/end info logs ----

/// With `-v` (verbosity bumped to info), every dispatched line emits a
/// `start` and matching outcome event. The line number and raw text
/// identify which subcommand each event belongs to.
#[test]
fn batch_run_logs_per_line_start_and_end_at_info() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        // RUST_LOG would override the verbosity flag we're testing —
        // strip it so an ambient `RUST_LOG=s7cmd=trace` etc. doesn't
        // make this test pass for the wrong reason.
        .env_remove("RUST_LOG")
        .args(["batch-run", "-v", "-"])
        .write_stdin(concat!(
            "create-bucket --dry-run s3://b1\n",
            "create-bucket --dry-run s3://b2\n",
        ))
        .assert()
        .success()
        .stderr(predicate::str::contains("line started"))
        .stderr(predicate::str::contains("line completed"));
}

/// A failing line is logged with `failure (exit N)` outcome, not
/// `success`. Verifies the exit-code → outcome-word mapping for the
/// error case.
#[test]
fn batch_run_logs_per_line_failure_outcome_with_exit_code() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        // See note in the previous test about RUST_LOG.
        .env_remove("RUST_LOG")
        // `--continue-on-error` so the run reaches both lines and we
        // can assert both events.
        .args(["batch-run", "-v", "--continue-on-error", "-"])
        .write_stdin(concat!(
            "create-bucket --dry-run s3://b1\n",
            // `ls --recursive` (no target) is a config-validation
            // failure → dispatch returns exit 2.
            "ls --recursive\n",
        ))
        .assert()
        .failure()
        .stderr(predicate::str::contains("line completed"))
        .stderr(predicate::str::contains("line started"))
        .stderr(predicate::str::contains("line failed"));
}

/// Without `-v`, info logs are suppressed (default verbosity is warn).
/// Confirms the new logs don't leak at the default level.
#[test]
fn batch_run_per_line_logs_silent_at_default_verbosity() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        // tracing_init reads RUST_LOG and uses it verbatim if set,
        // overriding the default-warn filter this test is asserting.
        // Drop it so an ambient `RUST_LOG=s7cmd=trace` doesn't break us.
        .env_remove("RUST_LOG")
        .args(["batch-run", "-"])
        .write_stdin("create-bucket --dry-run s3://b1\n")
        .assert()
        .success()
        .stderr(predicate::str::contains("line started").not())
        .stderr(predicate::str::contains("line completed").not())
        // The summary line must still appear — it goes to plain stderr,
        // not via tracing.
        .stderr(predicate::str::contains("1 succeeded, 0 failed"));
}

/// A failing line is emitted at error level, so it's visible at the
/// default `warn` verbosity without `-v`. The corresponding `start`
/// (info) is still suppressed — only the failure surfaces. Regression
/// test for the previous behavior where failures were emitted at info
/// and silently swallowed at the default level.
#[test]
fn batch_run_failure_log_visible_at_default_verbosity() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .env_remove("RUST_LOG")
        .args(["batch-run", "--continue-on-error", "-"])
        .write_stdin(concat!(
            "create-bucket --dry-run s3://b1\n",
            "ls --recursive\n",
        ))
        .assert()
        .failure()
        .stderr(predicate::str::contains("line started").not())
        .stderr(predicate::str::contains("line completed").not())
        .stderr(predicate::str::contains("line failed"));
}

// ---- --continue-on-warning surface ----

/// `--continue-on-warning` appears in `--help`. Cheap CLI-surface
/// regression test for the flag's existence and wiring.
#[test]
fn batch_run_continue_on_warning_listed_in_help() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--continue-on-warning"));
}

/// `--continue-on-warning` is mutually exclusive with `--continue-on-error`
/// — clap rejects the combination at parse time.
#[test]
fn batch_run_continue_on_warning_conflicts_with_continue_on_error() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--continue-on-error",
            "--continue-on-warning",
            "-",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

/// `--continue-on-warning` plus `--max-errors` is a permitted combination
/// (warnings ignored, true failures bounded). With empty stdin the run
/// just succeeds and prints a normal summary — what we're testing here
/// is that clap accepts the combination.
#[test]
fn batch_run_continue_on_warning_combines_with_max_errors() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--continue-on-warning",
            "--max-errors",
            "3",
            "-",
        ])
        .write_stdin("")
        .assert()
        .success()
        .stderr(predicate::str::contains("0 succeeded, 0 failed"));
}

// ---- parallel-mode coverage ----

/// `--parallel 2` exercises `executor::run_parallel` (the workers != 1
/// arm in `run_read_all`). Two dry-run create-bucket lines are enough.
#[test]
fn batch_run_parallel_two_workers_succeeds() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--parallel", "2", "-"])
        .write_stdin(concat!(
            "create-bucket --dry-run s3://b1\n",
            "create-bucket --dry-run s3://b2\n",
            "create-bucket --dry-run s3://b3\n",
            "create-bucket --dry-run s3://b4\n",
        ))
        .assert()
        .success()
        .stderr(predicate::str::contains("4 succeeded, 0 failed"));
}

/// `--parallel 2 --streaming` exercises `executor::run_parallel_streaming`.
#[test]
fn batch_run_streaming_parallel_two_workers_succeeds() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--streaming", "--parallel", "2", "-"])
        .write_stdin(concat!(
            "create-bucket --dry-run s3://b1\n",
            "create-bucket --dry-run s3://b2\n",
            "create-bucket --dry-run s3://b3\n",
        ))
        .assert()
        .success()
        .stderr(predicate::str::contains("3 succeeded, 0 failed"));
}

// ---- mv stdio rejection (mirrors the existing cp tests) ----

/// `mv` with `-` as the target is rejected by validate.
#[test]
fn batch_run_rejects_stdio_mv_target() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin("mv s3://bucket/key -\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("stdin/stdout"));
}

/// `mv` with `-` as the source is rejected by validate.
#[test]
fn batch_run_rejects_stdio_mv_source() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin("mv - s3://bucket/key\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("stdin/stdout"));
}

// ---- file-source error branches ----

/// 16 KiB cap rejection from a FILE source (the stdin variant exists
/// already). Exercises the file-branch read-all error path.
#[test]
fn batch_run_rejects_line_over_16kib_cap_file_read_all() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("script.txt");
    let mut content = String::with_capacity(MAX_LINE_LEN + 3);
    content.push('#');
    content.extend(std::iter::repeat_n('x', MAX_LINE_LEN));
    content.push('\n');
    std::fs::write(&path, content).unwrap();

    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", path.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("line 1"))
        .stderr(predicate::str::contains("exceeds"));
}

/// Streaming mode + missing file → opens fails before any reader spawns
/// and the error mentions the path.
#[test]
fn batch_run_streaming_missing_file_errors() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--streaming",
            "/nonexistent/s7cmd-streaming-missing.txt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "/nonexistent/s7cmd-streaming-missing.txt",
        ));
}

// ---- streaming reader error branches ----
//
// Each of these exercises a distinct error arm in `streaming_reader`:
// tokenize error, clap parse error, empty command, respectively.

/// Streaming mode tokenize error (unbalanced quote) is reported with the
/// line number and the offending text.
#[test]
fn batch_run_streaming_parse_error_includes_line_number() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--disable-color-tracing", "--streaming", "-"])
        .write_stdin("# blank ok\ncp \"unterminated\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("line=2"))
        .stderr(predicate::str::contains("malformed"));
}

/// Streaming mode unknown subcommand → clap parse error path.
#[test]
fn batch_run_streaming_clap_parse_error_includes_line_number() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--disable-color-tracing", "--streaming", "-"])
        .write_stdin("no-such-command\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("line=1"))
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

/// Streaming mode `--auto-complete-shell` → top-level flag with no
/// subcommand → empty-command branch in `streaming_reader`.
#[test]
fn batch_run_streaming_empty_command_errors() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--disable-color-tracing", "--streaming", "-"])
        .write_stdin("--auto-complete-shell bash\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("line=1"))
        .stderr(predicate::str::contains("empty command"));
}

// ---- check_format error branches ----

/// `--check-format` + a top-level-only line (`--auto-complete-shell`)
/// hits the empty-command branch in `check_format_lines`.
#[test]
fn batch_run_check_format_rejects_empty_command() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--check-format", "-"])
        .write_stdin("--auto-complete-shell bash\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("empty command"));
}

/// `--check-format` + an unbalanced quote hits the tokenize-error branch
/// in `check_format_lines` (distinct from the clap parse-error branch
/// already covered by the `127.0.0.1` test).
#[test]
fn batch_run_check_format_rejects_tokenize_error() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "--disable-color-tracing",
            "-",
        ])
        .write_stdin("cp \"unterminated\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("malformed"))
        .stderr(predicate::str::contains("line=1"));
}

// ---- empty-command in read-all mode ----

/// `--auto-complete-shell bash` parses cleanly as a top-level CLI with
/// `command: None`. `parse_and_validate` surfaces this as an
/// `empty command` error from the read-all path.
#[test]
fn batch_run_read_all_empty_command_errors() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--disable-color-tracing", "-"])
        .write_stdin("--auto-complete-shell bash\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("empty command"))
        .stderr(predicate::str::contains("line=1"));
}

/// Regression: per-line `cp --auto-complete-shell <SHELL>` previously
/// panicked inside `s3util_rs::Config::try_from` (the flag is inherited
/// from upstream `CommonTransferArgs`, leaving `source` / `target` as
/// `None`, which `parse_storage_path("")` unwrapped on). batch-run now
/// parses every line through `cli_command()` — same as the top-level
/// binary — which clears the per-subcommand long name so clap rejects
/// the flag at parse time with a clean error.
#[test]
fn batch_run_per_line_auto_complete_shell_does_not_panic() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--disable-color-tracing", "-"])
        .write_stdin("cp --auto-complete-shell fish\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("line=1"))
        .stderr(predicate::str::contains("auto-complete-shell"))
        // Must NOT panic — the old behavior surfaced as
        // `thread 'main' ... panicked at .../storage_path.rs`.
        .stderr(predicate::str::contains("panicked").not());
}

/// Same regression in streaming mode, which has its own parse path
/// (`streaming_reader`).
#[test]
fn batch_run_streaming_per_line_auto_complete_shell_does_not_panic() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--disable-color-tracing", "--streaming", "-"])
        .write_stdin("cp --auto-complete-shell fish\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("line=1"))
        .stderr(predicate::str::contains("auto-complete-shell"))
        .stderr(predicate::str::contains("panicked").not());
}

/// And in `--check-format` mode.
#[test]
fn batch_run_check_format_per_line_auto_complete_shell_does_not_panic() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "--disable-color-tracing",
            "-",
        ])
        .write_stdin("cp --auto-complete-shell fish\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("line=1"))
        .stderr(predicate::str::contains("auto-complete-shell"))
        .stderr(predicate::str::contains("panicked").not());
}

// ---- --continue-on-error in streaming mode ----
//
// Existing tests cover `--continue-on-error` only in read-all mode. Add
// a streaming variant to exercise the streaming sequential executor's
// continue-on-error path.

#[test]
fn batch_run_streaming_continue_on_error_runs_all_lines() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--streaming", "--continue-on-error", "-"])
        .write_stdin(concat!(
            "create-bucket --dry-run s3://b1\n",
            // ls --recursive without target → dispatch returns exit 2.
            "ls --recursive\n",
            "create-bucket --dry-run s3://b2\n",
        ))
        .assert()
        .failure()
        .stderr(predicate::str::contains("2 succeeded, 1 failed"));
}

/// `--json-tracing` per-line events emit structured fields (line, event,
/// exit_code, command, raw) instead of packing everything into the message.
/// Verified against `create-bucket --dry-run` so no S3 endpoint is needed.
#[test]
fn batch_run_json_tracing_per_line_fields() {
    let assert = Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--json-tracing", "-v", "-"])
        .write_stdin("create-bucket --dry-run s3://b1\n")
        .assert()
        .success();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();

    let lines: Vec<serde_json::Value> = stderr
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();

    let start = lines
        .iter()
        .find(|v| v["fields"]["event"] == "start")
        .unwrap_or_else(|| panic!("no start event in stderr: {stderr}"));
    assert_eq!(start["fields"]["line"], 1);
    assert_eq!(start["fields"]["command"], "create-bucket");
    assert_eq!(start["fields"]["raw"], "create-bucket --dry-run s3://b1");
    assert_eq!(start["fields"]["message"], "line started");
    assert_eq!(start["level"], "INFO");

    let success = lines
        .iter()
        .find(|v| v["fields"]["event"] == "success")
        .unwrap_or_else(|| panic!("no success event in stderr: {stderr}"));
    assert_eq!(success["fields"]["line"], 1);
    assert_eq!(success["fields"]["exit_code"], 0);
    assert_eq!(success["fields"]["command"], "create-bucket");
    assert_eq!(success["fields"]["raw"], "create-bucket --dry-run s3://b1");
    assert_eq!(success["fields"]["message"], "line completed");
    assert_eq!(success["level"], "INFO");
}

/// Invalid lines emit `event="invalid"` plus invalid_kind, reason, line, raw,
/// exit_code as structured fields under --json-tracing.
#[test]
fn batch_run_json_tracing_invalid_line_fields() {
    let assert = Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--json-tracing", "--continue-on-error", "-"])
        .write_stdin("not-a-real-subcommand foo bar\n")
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();

    let invalid = stderr
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["fields"]["event"] == "invalid")
        .unwrap_or_else(|| panic!("no invalid event in stderr: {stderr}"));

    assert_eq!(invalid["fields"]["line"], 1);
    assert_eq!(invalid["fields"]["invalid_kind"], "parse");
    assert_eq!(invalid["fields"]["exit_code"], 2);
    assert_eq!(invalid["fields"]["raw"], "not-a-real-subcommand foo bar");
    assert_eq!(invalid["fields"]["message"], "line invalid");
    assert_eq!(invalid["level"], "ERROR");
    assert!(
        invalid["fields"]["reason"]
            .as_str()
            .unwrap()
            .contains("unrecognized subcommand"),
        "reason should contain clap error text: {}",
        invalid["fields"]["reason"]
    );
    // reason MUST NOT contain the old "line N: parse error: " prefix
    assert!(
        !invalid["fields"]["reason"]
            .as_str()
            .unwrap()
            .starts_with("line "),
        "reason should not bake the line prefix: {}",
        invalid["fields"]["reason"]
    );
}

/// A state-flag arg error inside a batch line must fail that line (exit 2)
/// without killing the whole batch: upstream's validate_state_flag called
/// process::exit(2), which bypassed per-line containment, skipped every
/// later line, and suppressed the summary.
#[test]
fn batch_run_state_flag_error_does_not_kill_batch() {
    let assert = Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--continue-on-error", "-"])
        .write_stdin(
            "put-bucket-versioning s3://example-bucket\n\
             put-bucket-accelerate-configuration s3://example-bucket\n\
             put-bucket-request-payment s3://example-bucket\n",
        )
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr
            .matches("one of --enabled or --suspended must be specified")
            .count()
            == 2,
        "lines 1 and 2 must both report the state-flag error: {stderr}"
    );
    assert!(
        stderr.contains("one of --requester or --bucket-owner must be specified"),
        "line 3 must still run after earlier lines failed: {stderr}"
    );
    assert!(
        stderr.contains("0 succeeded, 3 failed"),
        "summary must still be printed: {stderr}"
    );
}

/// `--check-format` must surface an over-cap line as a per-line read error
/// (structured `read_error` event) rather than silently stopping — the
/// sync `check_format_lines` walker has its own read-error branch,
/// separate from the executor paths.
#[test]
fn batch_run_check_format_oversized_line_reports_read_error() {
    let long_line = format!("head-bucket s3://{}\n", "a".repeat(17 * 1024));
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args([
            "batch-run",
            "--check-format",
            "--disable-color-tracing",
            "-",
        ])
        .write_stdin(long_line)
        .assert()
        .failure()
        .stderr(predicate::str::contains("line read error"))
        .stderr(predicate::str::contains("read_error"));
}

/// Scripts can come from a real file, not just `-`/stdin — the file branch
/// of `run_read_all` (open + BufReader + read_all) must behave identically
/// to the stdin branch, including the summary.
#[test]
fn batch_run_script_file_executes_lines() {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("batch.txt");
    std::fs::write(
        &script,
        "# comment\ncreate-bucket --dry-run s3://batch-file-a\ncreate-bucket --dry-run s3://batch-file-b\n",
    )
    .expect("write script");
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", script.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("2 succeeded, 0 failed"));
}
