//! Process-level e2e tests for batch-run.
//!
//! Gated by cfg(e2e_test) — hits real AWS via the s7cmd-e2e-test profile.
//!
//! Per-test pattern:
//! 1. Create a fresh bucket via `helper.create_bucket(&bucket, REGION)`.
//! 2. Build a script that bakes `--target-profile s7cmd-e2e-test
//!    --target-region {REGION}` into every dispatched line (batch-run does
//!    not propagate top-level flags to subcommands).
//! 3. Run `s7cmd batch-run -` (or with a script file) via `assert_cmd`.
//! 4. Assert exit code, stderr summary, and SDK-visible side effects.
//! 5. Tear down the bucket with `helper.delete_bucket_with_cascade(&bucket)`.

#![cfg(e2e_test)]

mod common;

use assert_cmd::Command as AssertCommand;
use common::{
    EXPRESS_ONE_ZONE_AZ, REGION, TestHelper, create_temp_dir, create_test_file,
    generate_bucket_name, run, s7cmd_cmd,
};
use predicates::prelude::*;
use std::path::Path;

/// Convert a local path to a form safe to embed in a batch-run script.
/// `batch-run` tokenizes lines with POSIX shlex, which treats `\` as an
/// escape character — so a Windows path like `tmp_xxx\payload.txt` would
/// be reduced to `tmp_xxxpayload.txt` and the file would not be found.
/// Forward slashes are accepted by Windows filesystem APIs and have no
/// special meaning in shlex, so converting on the way in is the simplest
/// portable fix.
fn shell_path(p: &Path) -> String {
    p.to_str().unwrap().replace('\\', "/")
}

/// Test #1 — mixed-subcommand happy path.
///
/// Script: create-bucket → put-bucket-tagging → cp → head-object → clean →
/// delete-bucket. Verifies every step landed on S3 and the final bucket is
/// gone.
#[tokio::test]
async fn batch_run_e2e_mixed_workflow_succeeds() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    let local_dir = create_temp_dir();
    let local_file = create_test_file(&local_dir, "payload.txt", b"hello batch-run");

    let bucket_url = format!("s3://{bucket}/");
    let key_url = format!("s3://{bucket}/key");

    let script = format!(
        "create-bucket --target-profile s7cmd-e2e-test --target-region {REGION} {bucket_url}\n\
         put-bucket-tagging --target-profile s7cmd-e2e-test --target-region {REGION} \
         --tagging \"team=data\" {bucket_url}\n\
         cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} {key_url}\n\
         head-object --target-profile s7cmd-e2e-test --target-region {REGION} {key_url}\n\
         clean --force --target-profile s7cmd-e2e-test --target-region {REGION} {bucket_url}\n\
         delete-bucket --target-profile s7cmd-e2e-test --target-region {REGION} {bucket_url}\n",
        local = shell_path(&local_file),
    );

    let assert = AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin(script)
        .assert();

    assert.success().stderr(predicate::str::contains(
        "6 succeeded, 0 failed, 0 warnings, 0 skipped",
    ));

    // SDK-side verification: bucket should be gone.
    assert!(
        !helper.is_bucket_exist(&bucket).await,
        "delete-bucket step should have removed the bucket"
    );

    // Belt-and-suspenders cleanup (idempotent on already-gone buckets).
    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Test #2 — same script as #1, but written to a file and passed
/// positionally instead of piped via stdin. Confirms the `<FILE>` source
/// path is exercised against real AWS.
#[tokio::test]
async fn batch_run_e2e_reads_from_file() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    let local_dir = create_temp_dir();
    let local_file = create_test_file(&local_dir, "payload.txt", b"hello batch-run");

    let bucket_url = format!("s3://{bucket}/");
    let key_url = format!("s3://{bucket}/key");

    let script_body = format!(
        "create-bucket --target-profile s7cmd-e2e-test --target-region {REGION} {bucket_url}\n\
         put-bucket-tagging --target-profile s7cmd-e2e-test --target-region {REGION} \
         --tagging \"team=data\" {bucket_url}\n\
         cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} {key_url}\n\
         head-object --target-profile s7cmd-e2e-test --target-region {REGION} {key_url}\n\
         clean --force --target-profile s7cmd-e2e-test --target-region {REGION} {bucket_url}\n\
         delete-bucket --target-profile s7cmd-e2e-test --target-region {REGION} {bucket_url}\n",
        local = shell_path(&local_file),
    );
    let script_path = create_test_file(&local_dir, "script.txt", script_body.as_bytes());

    let (code, _stdout, stderr) =
        run(s7cmd_cmd().args(["batch-run", script_path.to_str().unwrap()]));

    assert_eq!(code, Some(0), "expected exit 0; stderr={stderr}");
    assert!(
        stderr.contains("6 succeeded, 0 failed, 0 warnings, 0 skipped"),
        "summary mismatch; stderr={stderr}"
    );
    assert!(
        !helper.is_bucket_exist(&bucket).await,
        "delete-bucket step should have removed the bucket"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Test #3 — streaming mode. Two-line script (create + delete) executed
/// as it is read. Verifies the streaming reader dispatches real ops.
#[tokio::test]
async fn batch_run_e2e_streaming_dispatch_succeeds() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();

    let bucket_url = format!("s3://{bucket}/");
    let script = format!(
        "create-bucket --target-profile s7cmd-e2e-test --target-region {REGION} {bucket_url}\n\
         delete-bucket --target-profile s7cmd-e2e-test --target-region {REGION} {bucket_url}\n",
    );

    AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--streaming", "-"])
        .write_stdin(script)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "2 succeeded, 0 failed, 0 warnings, 0 skipped",
        ));

    assert!(
        !helper.is_bucket_exist(&bucket).await,
        "bucket should be gone after streaming run"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}

/// Test #4 — `--parallel 2` against four concurrent put-object-tagging
/// lines. Setup pre-creates the bucket and four objects via SDK helpers;
/// the script only does the tagging. Verifies all four tags landed.
#[tokio::test]
async fn batch_run_e2e_parallel_two_workers_succeeds() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    for n in 1..=4 {
        let key = format!("k{n}");
        helper.put_object(&bucket, &key, b"x".to_vec()).await;
    }

    let script = (1..=4)
        .map(|n| {
            format!(
                "put-object-tagging --target-profile s7cmd-e2e-test --target-region {REGION} \
                 --tagging \"team=data\" s3://{bucket}/k{n}"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--parallel", "2", "-"])
        .write_stdin(script)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "4 succeeded, 0 failed, 0 warnings, 0 skipped",
        ));

    for n in 1..=4 {
        let key = format!("k{n}");
        let tagging = helper.get_object_tagging(&bucket, &key, None).await;
        let tags = tagging.tag_set();
        let found = tags
            .iter()
            .any(|t| t.key() == "team" && t.value() == "data");
        assert!(found, "expected tag team=data on key {key}; got {tags:?}");
    }

    helper.delete_bucket_with_cascade(&bucket).await;
}

/// Test #5 — default fail-fast: a `cp` to a never-created bucket fails
/// (NoSuchBucket → exit 1), the second line never dispatches, the real
/// bucket is never created. We use `cp` (not `head-bucket`) because the
/// HEAD-class subcommands map NoSuchBucket to EXIT_CODE_NOT_FOUND (4), a
/// warning — `cp` returns the true error class (1).
#[tokio::test]
async fn batch_run_e2e_default_fails_fast_on_aws_error() {
    let helper = TestHelper::new().await;
    let missing = generate_bucket_name();
    let real = generate_bucket_name();

    let local_dir = create_temp_dir();
    let local_file = create_test_file(&local_dir, "payload.txt", b"hello");

    let script = format!(
        "cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} \
         s3://{missing}/key\n\
         create-bucket --target-profile s7cmd-e2e-test --target-region {REGION} s3://{real}\n",
        local = shell_path(&local_file),
    );

    let assert = AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin(script)
        .assert();

    assert.failure().code(1).stderr(predicate::str::contains(
        "0 succeeded, 1 failed, 0 warnings, 1 skipped",
    ));

    assert!(
        !helper.is_bucket_exist(&real).await,
        "second line should not have dispatched; real bucket must not exist"
    );

    // Cleanup is no-op for both names (neither was created).
    helper.delete_bucket_with_cascade(&real).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Test #6 — `--continue-on-error` keeps running past failures. Two
/// `cp` lines targeting never-created buckets fail; create-bucket then
/// delete-bucket succeed. Worst-of exit code is 1.
#[tokio::test]
async fn batch_run_e2e_continue_on_error_runs_all_lines() {
    let helper = TestHelper::new().await;
    let missing_a = generate_bucket_name();
    let missing_b = generate_bucket_name();
    let real = generate_bucket_name();

    let local_dir = create_temp_dir();
    let local_file = create_test_file(&local_dir, "payload.txt", b"hello");

    let script = format!(
        "cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} \
         s3://{missing_a}/key\n\
         cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} \
         s3://{missing_b}/key\n\
         create-bucket --target-profile s7cmd-e2e-test --target-region {REGION} s3://{real}\n\
         delete-bucket --target-profile s7cmd-e2e-test --target-region {REGION} s3://{real}\n",
        local = shell_path(&local_file),
    );

    let assert = AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--continue-on-error", "-"])
        .write_stdin(script)
        .assert();

    assert.failure().code(1).stderr(predicate::str::contains(
        "2 succeeded, 2 failed, 0 warnings, 0 skipped",
    ));

    assert!(
        !helper.is_bucket_exist(&real).await,
        "real bucket should be gone (delete-bucket ran)"
    );

    helper.delete_bucket_with_cascade(&real).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Test #7 — `--max-errors 2`: three missing-bucket lines + one
/// create-bucket. After the second failure, dispatch stops; lines 3 and 4
/// never run. The real bucket must not exist.
#[tokio::test]
async fn batch_run_e2e_max_errors_two_stops_after_second_failure() {
    let helper = TestHelper::new().await;
    let missing_a = generate_bucket_name();
    let missing_b = generate_bucket_name();
    let missing_c = generate_bucket_name();
    let real = generate_bucket_name();

    let local_dir = create_temp_dir();
    let local_file = create_test_file(&local_dir, "payload.txt", b"hello");

    let script = format!(
        "cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} \
         s3://{missing_a}/key\n\
         cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} \
         s3://{missing_b}/key\n\
         cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} \
         s3://{missing_c}/key\n\
         create-bucket --target-profile s7cmd-e2e-test --target-region {REGION} s3://{real}\n",
        local = shell_path(&local_file),
    );

    let assert = AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--max-errors", "2", "-"])
        .write_stdin(script)
        .assert();

    assert.failure().code(1).stderr(predicate::str::contains(
        "0 succeeded, 2 failed, 0 warnings, 2 skipped",
    ));

    assert!(
        !helper.is_bucket_exist(&real).await,
        "real bucket must not exist (lines 3 and 4 should have been skipped)"
    );

    helper.delete_bucket_with_cascade(&real).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Test #8 — `--continue-on-warning`: head-object on a missing key
/// returns exit 4 (NOT_FOUND warning). With the flag, the run continues
/// past it; subsequent cp + head-object on a real key both succeed.
/// Worst-of exit code is 4.
#[tokio::test]
async fn batch_run_e2e_continue_on_warning_past_not_found() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let local_dir = create_temp_dir();
    let local_file = create_test_file(&local_dir, "payload.txt", b"hello");

    let script = format!(
        "head-object --target-profile s7cmd-e2e-test --target-region {REGION} \
         s3://{bucket}/missing-key\n\
         cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} \
         s3://{bucket}/real-key\n\
         head-object --target-profile s7cmd-e2e-test --target-region {REGION} \
         s3://{bucket}/real-key\n",
        local = shell_path(&local_file),
    );

    let assert = AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--continue-on-warning", "-"])
        .write_stdin(script)
        .assert();

    assert.failure().code(4).stderr(predicate::str::contains(
        "2 succeeded, 0 failed, 1 warnings, 0 skipped",
    ));

    assert!(
        helper.is_object_exist(&bucket, "real-key", None).await,
        "real-key should have been uploaded"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Test #9 — pin the worst-of rule: exit codes (0, 4, 1) yield process
/// exit 1. The worst-of is severity-ranked (1 > 2 > 3 > 4 > anything
/// else > 0), not numeric `max`, so the actionable error outranks the
/// "not found" warning. With just `--continue-on-error` all three lines
/// dispatch (the flag continues past warnings AND errors; it is
/// mutually exclusive with `--continue-on-warning` at the clap level).
#[tokio::test]
async fn batch_run_e2e_exit_code_is_worst_of() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper
        .put_object(&bucket, "real-key", b"hello".to_vec())
        .await;

    let missing_bucket = generate_bucket_name();

    let local_dir = create_temp_dir();
    let local_file = create_test_file(&local_dir, "payload.txt", b"hello");

    let script = format!(
        "head-object --target-profile s7cmd-e2e-test --target-region {REGION} \
         s3://{bucket}/real-key\n\
         head-object --target-profile s7cmd-e2e-test --target-region {REGION} \
         s3://{bucket}/missing-key\n\
         cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} \
         s3://{missing_bucket}/key\n",
        local = shell_path(&local_file),
    );

    let assert = AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--continue-on-error", "-"])
        .write_stdin(script)
        .assert();

    assert.failure().code(1).stderr(predicate::str::contains(
        "1 succeeded, 1 failed, 1 warnings, 0 skipped",
    ));

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Test #10 — non-NoSuchBucket failure (`MalformedPolicy`, 400-class)
/// propagates through dispatch the same way 404 does. Bucket cleanup
/// happens on the next line because of `--continue-on-error`.
#[tokio::test]
async fn batch_run_e2e_malformed_policy_failure_propagates() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    // put-bucket-policy takes a positional [POLICY] which is a path to a
    // file containing the policy JSON (or `-` for stdin). Write the
    // malformed body to a temp file and reference it by path.
    let local_dir = create_temp_dir();
    let policy_path = create_test_file(&local_dir, "bad-policy.json", b"{not valid json");

    let script = format!(
        "put-bucket-policy --target-profile s7cmd-e2e-test --target-region {REGION} \
         s3://{bucket} {policy}\n\
         delete-bucket --target-profile s7cmd-e2e-test --target-region {REGION} s3://{bucket}\n",
        policy = shell_path(&policy_path),
    );

    let assert = AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--continue-on-error", "-"])
        .write_stdin(script)
        .assert();

    assert.failure().code(1).stderr(predicate::str::contains(
        "1 succeeded, 1 failed, 0 warnings, 0 skipped",
    ));

    // The original `helper`'s aws-sdk-s3 Client caches bucket-region /
    // endpoint metadata after `create_bucket`, so a HeadBucket from that
    // client can erroneously report the bucket as still present even after
    // a different process (the batch-run subprocess) has deleted it. Use
    // a fresh client so the post-script existence check reflects S3's
    // actual state.
    let verifier = TestHelper::new().await;
    assert!(
        !verifier.is_bucket_exist(&bucket).await,
        "delete-bucket should have removed the bucket on the second line"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Test #11 — `--json-tracing` emits a single-line JSON summary object on
/// stderr. Parse it with serde_json (already a dev-dependency) and check
/// each numeric field against the expected counts.
///
/// Note: `assert_cmd::Command::output()` is used (not the `run()` helper
/// from `common`) because we need to both pipe stdin (`write_stdin`) and
/// capture stderr bytes; `run()` does not accept stdin.
#[tokio::test]
async fn batch_run_e2e_json_tracing_summary_object() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let missing = generate_bucket_name();

    let local_dir = create_temp_dir();
    let local_file = create_test_file(&local_dir, "payload.txt", b"hello");

    // Line 1: head-bucket on a real bucket → exit 0.
    // Line 2: cp to a never-created bucket → exit 1 (NoSuchBucket on the
    // PUT path is mapped to EXIT_CODE_ERROR, not the head-class warning).
    let script = format!(
        "head-bucket --target-profile s7cmd-e2e-test --target-region {REGION} s3://{bucket}\n\
         cp --target-profile s7cmd-e2e-test --target-region {REGION} {local} \
         s3://{missing}/key\n",
        local = shell_path(&local_file),
    );

    let output = AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--json-tracing", "--continue-on-error", "-"])
        .write_stdin(script)
        .output()
        .expect("spawn s7cmd");

    assert_eq!(output.status.code(), Some(1), "expected exit 1");

    let stderr_text = String::from_utf8_lossy(&output.stderr);
    // The summary line is a JSON object with alphabetically-sorted keys, so
    // it does not start with `{"summary"`. Match by the unique
    // `"summary":"batch-run"` field-presence marker instead.
    let summary_line = stderr_text
        .lines()
        .find(|l| l.contains(r#""summary":"batch-run""#))
        .unwrap_or_else(|| panic!("no JSON summary line in stderr; stderr={stderr_text}"));

    let parsed: serde_json::Value =
        serde_json::from_str(summary_line.trim()).expect("summary line must be JSON");

    assert_eq!(parsed["summary"], "batch-run");
    assert_eq!(parsed["succeeded"], 1);
    assert_eq!(parsed["failed"], 1);
    assert_eq!(parsed["warnings"], 0);
    assert_eq!(parsed["skipped"], 0);
    assert!(
        parsed["elapsed_seconds"].is_number(),
        "elapsed_seconds missing or not numeric"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Test #12 — `--no-summary` suppresses the summary line even on a real
/// AWS-backed run. Two head-bucket lines on a real bucket; both succeed.
#[tokio::test]
async fn batch_run_e2e_no_summary_suppresses_summary_real_run() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let script = format!(
        "head-bucket --target-profile s7cmd-e2e-test --target-region {REGION} s3://{bucket}\n\
         head-bucket --target-profile s7cmd-e2e-test --target-region {REGION} s3://{bucket}\n",
    );

    AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--no-summary", "-"])
        .write_stdin(script)
        .assert()
        .success()
        .stderr(predicate::str::contains("succeeded,").not());

    helper.delete_bucket_with_cascade(&bucket).await;
}

#[cfg(all(e2e_test, unix))]
#[tokio::test]
async fn batch_run_e2e_sigint_marks_in_flight_skipped() {
    use common::create_sized_file;
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::process::Stdio;
    use std::time::Duration;

    const STARTUP_DELAY_MS: u64 = 1500;
    const WAIT_TIMEOUT_SECS: u64 = 30;
    const PAYLOAD_BYTES: usize = 200 * 1024 * 1024; // 200 MB

    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let local_dir = create_temp_dir();
    let payload = create_sized_file(&local_dir, "big.bin", PAYLOAD_BYTES);

    let script = format!(
        "cp --target-profile s7cmd-e2e-test --target-region {REGION} {p} s3://{bucket}/key1\n\
         cp --target-profile s7cmd-e2e-test --target-region {REGION} {p} s3://{bucket}/key2\n",
        p = shell_path(&payload),
    );

    let mut child = s7cmd_cmd()
        .args(["batch-run", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn s7cmd");

    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().expect("stdin piped");
        stdin
            .write_all(script.as_bytes())
            .expect("write script to child stdin");
    }
    // Closing stdin ensures batch-run sees EOF after the two lines.
    drop(child.stdin.take());

    tokio::time::sleep(Duration::from_millis(STARTUP_DELAY_MS)).await;
    let pid = Pid::from_raw(child.id() as i32);
    let _ = kill(pid, Signal::SIGINT);

    let wait_handle = tokio::task::spawn_blocking(move || child.wait_with_output());
    let output =
        match tokio::time::timeout(Duration::from_secs(WAIT_TIMEOUT_SECS), wait_handle).await {
            Ok(Ok(Ok(output))) => output,
            Ok(Ok(Err(e))) => panic!("child.wait_with_output failed: {e}"),
            Ok(Err(e)) => panic!("spawn_blocking join failed: {e}"),
            Err(_) => panic!("child did not exit within {WAIT_TIMEOUT_SECS}s after SIGINT"),
        };

    assert_eq!(
        output.status.code(),
        Some(130),
        "SIGINT must yield exit 130; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr_text = String::from_utf8_lossy(&output.stderr);
    // Summary line is `N succeeded, N failed, N warnings, N skipped, elapsed Ts`.
    // We assert at least one skipped (an in-flight or never-dispatched cp).
    let skipped_nonzero = stderr_text
        .lines()
        .filter(|l| l.contains("succeeded,") && l.contains("skipped"))
        .any(|l| {
            // crude: pull the integer immediately before `skipped`
            l.split(',')
                .find_map(|chunk| {
                    let trimmed = chunk.trim();
                    trimmed
                        .strip_suffix("skipped")
                        .map(str::trim)
                        .and_then(|n| n.parse::<u64>().ok())
                })
                .map(|n| n > 0)
                .unwrap_or(false)
        });
    assert!(
        skipped_nonzero,
        "expected skipped >= 1 in summary; stderr={stderr_text}"
    );

    // Cleanup: in-flight multipart uploads + bucket.
    helper.abort_all_multipart_uploads(&bucket).await;
    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

// ---- 100-object batch-run tests (file / stdin / streaming / parallel 3) ----
//
// Each of the four tests below uploads exactly 100 objects keyed
// `k-000`…`k-099` via batch-run, then verifies the same 100 keys are
// present on S3 by listing the bucket. The four tests differ only in how
// the script reaches batch-run and which execution mode is exercised.

const OBJECT_COUNT_100: usize = 100;

fn build_100_object_script(bucket: &str, payload_path: &str) -> String {
    (0..OBJECT_COUNT_100)
        .map(|n| {
            format!(
                "cp --target-profile s7cmd-e2e-test --target-region {REGION} \
                 {payload_path} s3://{bucket}/k-{n:03}\n"
            )
        })
        .collect()
}

async fn verify_100_keys_present(helper: &TestHelper, bucket: &str) {
    let out = helper
        .client
        .list_objects_v2()
        .bucket(bucket)
        .send()
        .await
        .expect("list_objects_v2");
    assert!(
        !matches!(out.is_truncated(), Some(true)),
        "list_objects_v2 truncated for {bucket}; default MaxKeys should fit 100"
    );
    let mut keys: Vec<String> = out
        .contents()
        .iter()
        .filter_map(|o| o.key().map(str::to_string))
        .collect();
    keys.sort();
    assert_eq!(
        keys.len(),
        OBJECT_COUNT_100,
        "expected {OBJECT_COUNT_100} objects in {bucket}, got {}",
        keys.len()
    );
    for (i, key) in keys.iter().enumerate() {
        let expected = format!("k-{i:03}");
        assert_eq!(key, &expected, "object index {i} key mismatch in {bucket}");
    }
}

/// 100 objects via a script file (default read-all, sequential).
#[tokio::test]
async fn batch_run_e2e_100_objects_via_file() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let local_dir = create_temp_dir();
    let payload = create_test_file(&local_dir, "payload.txt", b"x");
    let script_body = build_100_object_script(&bucket, &shell_path(&payload));
    let script_path = create_test_file(&local_dir, "script.txt", script_body.as_bytes());

    let (code, _stdout, stderr) =
        run(s7cmd_cmd().args(["batch-run", script_path.to_str().unwrap()]));

    assert_eq!(code, Some(0), "expected exit 0; stderr={stderr}");
    assert!(
        stderr.contains("100 succeeded, 0 failed, 0 warnings, 0 skipped"),
        "summary mismatch; stderr={stderr}"
    );

    verify_100_keys_present(&helper, &bucket).await;

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// 100 objects via stdin pipe (default read-all, sequential).
#[tokio::test]
async fn batch_run_e2e_100_objects_via_stdin() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let local_dir = create_temp_dir();
    let payload = create_test_file(&local_dir, "payload.txt", b"x");
    let script = build_100_object_script(&bucket, &shell_path(&payload));

    AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "-"])
        .write_stdin(script)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "100 succeeded, 0 failed, 0 warnings, 0 skipped",
        ));

    verify_100_keys_present(&helper, &bucket).await;

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// 100 objects via stdin pipe with `--streaming` (executes lines as read).
#[tokio::test]
async fn batch_run_e2e_100_objects_streaming() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let local_dir = create_temp_dir();
    let payload = create_test_file(&local_dir, "payload.txt", b"x");
    let script = build_100_object_script(&bucket, &shell_path(&payload));

    AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--streaming", "-"])
        .write_stdin(script)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "100 succeeded, 0 failed, 0 warnings, 0 skipped",
        ));

    verify_100_keys_present(&helper, &bucket).await;

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// 100 objects via a script file with `--streaming` (executes lines as
/// read; the file is the source rather than stdin). Covers the
/// `--streaming <FILE>` combination that the per-mode tests above leave
/// unexercised at the dispatch level.
#[tokio::test]
async fn batch_run_e2e_100_objects_streaming_via_file() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let local_dir = create_temp_dir();
    let payload = create_test_file(&local_dir, "payload.txt", b"x");
    let script_body = build_100_object_script(&bucket, &shell_path(&payload));
    let script_path = create_test_file(&local_dir, "script.txt", script_body.as_bytes());

    let (code, _stdout, stderr) =
        run(s7cmd_cmd().args(["batch-run", "--streaming", script_path.to_str().unwrap()]));

    assert_eq!(code, Some(0), "expected exit 0; stderr={stderr}");
    assert!(
        stderr.contains("100 succeeded, 0 failed, 0 warnings, 0 skipped"),
        "summary mismatch; stderr={stderr}"
    );

    verify_100_keys_present(&helper, &bucket).await;

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// 100 objects via stdin pipe with `--parallel 3` (3 concurrent dispatches;
/// completion order is not preserved, but every key must land exactly once).
#[tokio::test]
async fn batch_run_e2e_100_objects_parallel3() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let local_dir = create_temp_dir();
    let payload = create_test_file(&local_dir, "payload.txt", b"x");
    let script = build_100_object_script(&bucket, &shell_path(&payload));

    AssertCommand::cargo_bin("s7cmd")
        .unwrap()
        .args(["batch-run", "--parallel", "3", "-"])
        .write_stdin(script)
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "100 succeeded, 0 failed, 0 warnings, 0 skipped",
        ));

    verify_100_keys_present(&helper, &bucket).await;

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

// ---- All-subcommands smoke test ----
//
// One large script that exercises every batch-run-able subcommand
// end-to-end against real AWS. Read from a file with `--parallel 1`,
// asserts only that the process exits 0 (per the user's "doesn't fail"
// requirement). Bucket-level configurations (lifecycle, encryption,
// CORS, public-access-block, website, logging, notification, policy)
// reuse the JSON shapes from the per-config e2e files. Order matters:
// put-bucket-policy lands before put-public-access-block, and the
// teardown deletes them in the reverse-of-create order so each
// subcommand operates on the state it expects.

fn sample_cors_json_inline() -> &'static str {
    r#"{"CORSRules":[{"ID":"r1","AllowedMethods":["GET","HEAD"],"AllowedOrigins":["*"],"AllowedHeaders":["*"],"MaxAgeSeconds":3000}]}"#
}

fn sample_encryption_json_inline() -> &'static str {
    r#"{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"AES256"}}]}"#
}

fn sample_lifecycle_json_inline() -> &'static str {
    r#"{"Rules":[{"ID":"r1","Status":"Enabled","Filter":{"Prefix":"logs/"},"Expiration":{"Days":365}}]}"#
}

fn sample_pab_json_inline() -> &'static str {
    r#"{"BlockPublicAcls":true,"IgnorePublicAcls":true,"BlockPublicPolicy":true,"RestrictPublicBuckets":true}"#
}

fn sample_website_json_inline() -> &'static str {
    r#"{"IndexDocument":{"Suffix":"index.html"}}"#
}

fn sample_policy_inline(bucket: &str) -> String {
    format!(
        r#"{{"Version":"2012-10-17","Statement":[{{"Sid":"DenyInsecureConnections","Effect":"Deny","Principal":"*","Action":"s3:*","Resource":["arn:aws:s3:::{bucket}","arn:aws:s3:::{bucket}/*"],"Condition":{{"Bool":{{"aws:SecureTransport":"false"}}}}}}]}}"#,
    )
}

/// Smoke-test that every batch-run-able subcommand dispatches successfully
/// from a single script. Reads from a file (positional `<FILE>` form),
/// `--parallel 1`. Verifies process exit 0; the script's `delete-bucket`
/// line is the last statement, so a 0 exit implies every step succeeded
/// against real AWS.
#[tokio::test]
async fn batch_run_e2e_all_subcommands_via_file() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    let expr_bucket = format!("s7cmd-e2e-{}--apne1-az4--x-s3", uuid::Uuid::new_v4());
    helper
        .create_directory_bucket(&expr_bucket, EXPRESS_ONE_ZONE_AZ)
        .await;

    let local_dir = create_temp_dir();
    let payload = create_test_file(&local_dir, "payload.txt", b"hello all-subcommands");

    // Sync source dir with two small files.
    let sync_src = local_dir.join("sync-src");
    std::fs::create_dir_all(&sync_src).expect("create sync-src");
    create_test_file(&sync_src, "a.txt", b"a");
    create_test_file(&sync_src, "b.txt", b"b");

    // Bucket-level configuration JSON files.
    let policy_path = create_test_file(
        &local_dir,
        "policy.json",
        sample_policy_inline(&bucket).as_bytes(),
    );
    let lifecycle_path = create_test_file(
        &local_dir,
        "lifecycle.json",
        sample_lifecycle_json_inline().as_bytes(),
    );
    let encryption_path = create_test_file(
        &local_dir,
        "encryption.json",
        sample_encryption_json_inline().as_bytes(),
    );
    let cors_path = create_test_file(
        &local_dir,
        "cors.json",
        sample_cors_json_inline().as_bytes(),
    );
    let pab_path = create_test_file(&local_dir, "pab.json", sample_pab_json_inline().as_bytes());
    let website_path = create_test_file(
        &local_dir,
        "website.json",
        sample_website_json_inline().as_bytes(),
    );
    // Empty `{}` is a valid no-op for both put-bucket-logging (disables
    // logging) and put-bucket-notification-configuration (removes all
    // notifications). Per s7cmd's help text, neither subcommand has a
    // delete-* counterpart.
    let logging_path = create_test_file(&local_dir, "logging.json", b"{}");
    let notification_path = create_test_file(&local_dir, "notification.json", b"{}");

    // Per-line auth flags. Most subcommands take target-only; mv
    // s3://X → s3://X needs both source and target pairs; rename
    // uses only source-prefixed flags (source and target are in the
    // same bucket).
    let auth_target = format!("--target-profile s7cmd-e2e-test --target-region {REGION}");
    let auth_both = format!(
        "--source-profile s7cmd-e2e-test --source-region {REGION} \
         --target-profile s7cmd-e2e-test --target-region {REGION}"
    );
    let auth_source = format!("--source-profile s7cmd-e2e-test --source-region {REGION}");

    let payload_p = shell_path(&payload);
    let sync_p = shell_path(&sync_src);
    let policy_p = shell_path(&policy_path);
    let lifecycle_p = shell_path(&lifecycle_path);
    let encryption_p = shell_path(&encryption_path);
    let cors_p = shell_path(&cors_path);
    let pab_p = shell_path(&pab_path);
    let website_p = shell_path(&website_path);
    let logging_p = shell_path(&logging_path);
    let notification_p = shell_path(&notification_path);
    // Destination file for get-object-annotation (created by the script line).
    let annotation_out = local_dir.join("annotation-out.bin");
    let annotation_out_p = shell_path(&annotation_out);

    // NOTE: `put-bucket-versioning --suspended` is intentionally placed
    // AFTER every DELETE-class object operation. On a Suspended bucket,
    // DELETEs create delete-markers (special "versions") that
    // `list_objects_v2` does not enumerate, so `clean --force` would
    // leave them behind and the final `delete-bucket` would fail with
    // BucketNotEmpty. By keeping versioning untouched until after
    // `clean`, every object operation runs on a plain non-versioned
    // bucket and `clean` truly empties it.
    let script = format!(
        "\
# ---- create + configure (versioning intentionally deferred) ----
create-bucket {auth_target} s3://{bucket}
put-bucket-tagging {auth_target} --tagging \"team=data\" s3://{bucket}
put-bucket-policy {auth_target} s3://{bucket} {policy_p}
put-bucket-lifecycle-configuration {auth_target} s3://{bucket} {lifecycle_p}
put-bucket-encryption {auth_target} s3://{bucket} {encryption_p}
put-bucket-cors {auth_target} s3://{bucket} {cors_p}
put-public-access-block {auth_target} s3://{bucket} {pab_p}
put-bucket-website {auth_target} s3://{bucket} {website_p}
put-bucket-logging {auth_target} s3://{bucket} {logging_p}
put-bucket-notification-configuration {auth_target} s3://{bucket} {notification_p}

# ---- v1.3.0 bucket-level configuration (Transfer Acceleration,
# ---- Request Payment). Replication is intentionally NOT included
# ---- here: it requires versioning enabled on both source and
# ---- destination buckets plus an IAM role, which is out of scope
# ---- for a self-contained smoke test.
put-bucket-accelerate-configuration {auth_target} --enabled s3://{bucket}
put-bucket-request-payment {auth_target} --requester s3://{bucket}

# ---- read everything back (versioning later) ----
head-bucket {auth_target} s3://{bucket}
get-bucket-tagging {auth_target} s3://{bucket}
get-bucket-policy {auth_target} s3://{bucket}
get-bucket-policy-status {auth_target} s3://{bucket}
get-bucket-lifecycle-configuration {auth_target} s3://{bucket}
get-bucket-encryption {auth_target} s3://{bucket}
get-bucket-cors {auth_target} s3://{bucket}
get-public-access-block {auth_target} s3://{bucket}
get-bucket-website {auth_target} s3://{bucket}
get-bucket-logging {auth_target} s3://{bucket}
get-bucket-notification-configuration {auth_target} s3://{bucket}
get-bucket-accelerate-configuration {auth_target} s3://{bucket}
get-bucket-request-payment {auth_target} s3://{bucket}

# ---- object operations (bucket is still non-versioned here) ----
cp {auth_target} {payload_p} s3://{bucket}/object1
head-object {auth_target} s3://{bucket}/object1
presign {auth_target} s3://{bucket}/object1
put-object-tagging {auth_target} --tagging \"k=v\" s3://{bucket}/object1
get-object-tagging {auth_target} s3://{bucket}/object1
delete-object-tagging {auth_target} s3://{bucket}/object1
put-object-annotation {auth_target} --annotation-name note --annotation-payload {payload_p} s3://{bucket}/object1
get-object-annotation {auth_target} --annotation-name note s3://{bucket}/object1 {annotation_out_p}
list-object-annotations {auth_target} s3://{bucket}/object1
delete-object-annotation {auth_target} --annotation-name note s3://{bucket}/object1
ls {auth_target} s3://{bucket}
sync {auth_target} {sync_p} s3://{bucket}/synced/
mv {auth_both} s3://{bucket}/object1 s3://{bucket}/object1-moved
rm {auth_target} s3://{bucket}/object1-moved
cp {auth_target} {payload_p} s3://{expr_bucket}/rename-src
rename {auth_source} s3://{expr_bucket}/rename-src s3://{expr_bucket}/rename-dst
clean --force {auth_target} s3://{bucket}

# ---- versioning subcommand pair, on the now-empty bucket ----
put-bucket-versioning {auth_target} --suspended s3://{bucket}
get-bucket-versioning {auth_target} s3://{bucket}

# ---- tear down configuration ----
delete-bucket-policy {auth_target} s3://{bucket}
delete-public-access-block {auth_target} s3://{bucket}
delete-bucket-tagging {auth_target} s3://{bucket}
delete-bucket-lifecycle-configuration {auth_target} s3://{bucket}
delete-bucket-encryption {auth_target} s3://{bucket}
delete-bucket-cors {auth_target} s3://{bucket}
delete-bucket-website {auth_target} s3://{bucket}
delete-bucket-replication {auth_target} s3://{bucket}

# ---- Express One Zone bucket teardown (rename test) ----
clean --force {auth_target} s3://{expr_bucket}
delete-bucket {auth_target} s3://{expr_bucket}

# ---- final teardown ----
delete-bucket {auth_target} s3://{bucket}
"
    );
    let script_path = create_test_file(&local_dir, "script.txt", script.as_bytes());

    let (code, _stdout, stderr) = run(s7cmd_cmd().args([
        "batch-run",
        "--parallel",
        "1",
        script_path.to_str().unwrap(),
    ]));

    assert_eq!(
        code,
        Some(0),
        "all-subcommands script must exit 0; stderr={stderr}"
    );

    // Defensive cleanup in case any line failed before the final
    // delete-bucket. Both helpers are idempotent on already-gone buckets.
    helper.delete_bucket_with_cascade(&bucket).await;
    helper
        .delete_directory_bucket_with_cascade(&expr_bucket)
        .await;
    let _ = std::fs::remove_dir_all(&local_dir);
}
