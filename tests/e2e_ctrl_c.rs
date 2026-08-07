//! Process-level e2e tests for graceful Ctrl+C handling.
//!
//! Each test spawns the binary directly (NOT via cargo run — cargo
//! intercepts SIGINT), waits for it to enter its work loop, sends SIGINT,
//! and asserts the child exits with the expected code within a 30s timeout.
//!
//! Unix-only: SIGINT delivery via the `nix` crate.

#![cfg(all(e2e_test, unix))]

mod common;

use common::{
    REGION, TestHelper, create_sized_file, create_temp_dir, generate_bucket_name, s7cmd_cmd,
};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::process::Stdio;
use std::time::Duration;

/// How long to let the child run before delivering SIGINT, per attempt.
///
/// A single fixed delay is machine-dependent and was the source of flaky
/// `None` results: the commands install their Ctrl-C handler only after
/// credential/profile loading and client construction, and an s3→s3 command
/// (mv) builds *two* clients before it gets there. Signal too early and the
/// default disposition kills the process outright.
///
/// `run_with_sigint` escalates through these delays, so a slower machine or
/// higher-latency region self-corrects instead of failing. The largest delay
/// must still be comfortably inside every test's throttled work window (the
/// shortest is ~15s: a 30 MiB transfer at `--rate-limit-bandwidth 2MiB`).
const STARTUP_DELAYS_MS: [u64; 3] = [2000, 5000, 9000];
const WAIT_TIMEOUT_SECS: u64 = 30;

/// Signal-timing tests must not run concurrently. `run_with_sigint` waits a
/// bounded time for the child to install its Ctrl-C handler, and a machine
/// busy seeding objects for another test can push a child past that window —
/// SIGINT then lands under the *default* disposition and kills it outright
/// (exit code `None`) instead of driving graceful cancellation. The retry in
/// `run_with_sigint` recovers from that, but only after re-running the whole
/// command; holding this lock for each test body keeps the machine
/// uncontended so the first attempt usually suffices.
static SIGINT_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Spawn `cmd`, let it settle, deliver SIGINT, wait for exit (capped at
/// `WAIT_TIMEOUT_SECS`), and return the exit code. Stdout and stderr of the
/// child are discarded — these tests assert on exit code, not output.
///
/// Returns `None` only if the child was terminated *by* the signal rather
/// than exiting on its own. That specifically means the Ctrl-C handler was
/// not yet installed when the signal landed, so the attempt is retried with
/// a longer startup delay. Retrying is safe precisely because a child that
/// died this way was still starting up: it had not begun the throttled
/// transfer/listing/deletion, so the seeded workload is untouched.
async fn run_with_sigint(cmd: &mut std::process::Command) -> Option<i32> {
    for (attempt, delay_ms) in STARTUP_DELAYS_MS.iter().enumerate() {
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn s7cmd");

        tokio::time::sleep(Duration::from_millis(*delay_ms)).await;
        let pid = Pid::from_raw(child.id() as i32);
        let _ = kill(pid, Signal::SIGINT);

        // Bounded wait so a hang fails fast instead of stalling CI.
        let wait_handle = tokio::task::spawn_blocking(move || child.wait());
        let status =
            match tokio::time::timeout(Duration::from_secs(WAIT_TIMEOUT_SECS), wait_handle).await {
                Ok(Ok(Ok(status))) => status,
                Ok(Ok(Err(e))) => panic!("child.wait() failed: {e}"),
                Ok(Err(e)) => panic!("spawn_blocking join failed: {e}"),
                Err(_) => panic!("child did not exit within {WAIT_TIMEOUT_SECS}s after SIGINT"),
            };

        match status.code() {
            Some(code) => return Some(code),
            None => {
                eprintln!(
                    "SIGINT at {delay_ms}ms killed the child before its handler was installed \
                     (attempt {}/{}); retrying with a longer delay",
                    attempt + 1,
                    STARTUP_DELAYS_MS.len(),
                );
            }
        }
    }
    // Every attempt died by signal — report it as such so the caller's
    // assertion fails with the same `None` it would have seen before.
    None
}

// ---- sync ----

#[tokio::test]
async fn cancel_sync_sigint_exits_130() {
    let _serial = SIGINT_TEST_LOCK.lock().await;
    // src/sync_bin/cli/mod.rs checks `is_ctrl_c_received()` once the
    // pipeline stops and returns SIGINT_EXIT_CODE (130) regardless of what
    // the forced shutdown recorded (Ctrl+C takes precedence over errors and
    // warnings), so the exit code is deterministic: the throttled 30 MiB
    // transfer guarantees SIGINT lands mid-run, and `run_with_sigint`
    // retries any attempt where the signal beat the handler installation.
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper
        .put_object(&bucket, "big.bin", vec![0u8; 30 * 1024 * 1024])
        .await;

    let local_dir = create_temp_dir();
    let source = format!("s3://{bucket}/");
    let mut cmd = s7cmd_cmd();
    cmd.args([
        "sync",
        "--source-profile",
        "s7cmd-e2e-test",
        "--source-region",
        REGION,
        "--rate-limit-bandwidth",
        "2MiB",
        &source,
        local_dir.to_str().unwrap(),
    ]);

    let code = run_with_sigint(&mut cmd).await;
    assert_eq!(code, Some(130), "sync SIGINT must exit 130; got {code:?}");

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

// ---- ls ----

#[tokio::test]
async fn cancel_ls_sigint_does_not_hang() {
    let _serial = SIGINT_TEST_LOCK.lock().await;
    // S3 paginates ListObjectsV2 at 1000 objects; --rate-limit-api throttles
    // BETWEEN pages, not within a page, so 200 objects in a single page
    // would return before SIGINT is delivered on a fast network.
    // Seeding 1000+ objects to force multi-page listing is wasteful for a
    // dispatch-only test, so we fall back to the spec-authorized soft
    // assertion: confirm the process exits (i.e. doesn't hang) and that
    // SIGINT was honored — exact exit code is not required because, on a
    // very fast listing, the process may complete normally (exit 0) before
    // SIGINT lands, while an interrupted run exits 130. The strict
    // "must exit 130" assertion is covered by
    // cancel_ls_sigint_mid_paginated_listing_exits_130 below (and by sync,
    // cp, mv, and clean, which all have per-byte/per-object throttles).
    //
    // `--rate-limit-api` must be >= 10 (its clap range is 10..=u32::MAX);
    // a lower value makes the run die at exit 2 before any S3 call, which
    // this test's exit-code-agnostic assertion would silently accept.
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    for i in 0..200 {
        helper
            .put_object(&bucket, &format!("k{i:04}"), b"x".to_vec())
            .await;
    }

    let target = format!("s3://{bucket}/");
    let mut cmd = s7cmd_cmd();
    cmd.args([
        "ls",
        "--target-profile",
        "s7cmd-e2e-test",
        "--target-region",
        REGION,
        "--recursive",
        "--rate-limit-api",
        "10",
        &target,
    ]);

    // run_with_sigint already enforces a 30s timeout — passing it means the
    // process exited (not hung). We don't assert on the exit code, but it
    // must not be the clap-error 2 (that would mean nothing was listed).
    let code = run_with_sigint(&mut cmd).await;
    assert_ne!(
        code,
        Some(2),
        "invocation must be valid enough to reach the listing"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}

// ---- clean ----

#[tokio::test]
async fn cancel_clean_sigint_does_not_hang() {
    let _serial = SIGINT_TEST_LOCK.lock().await;
    // clean's bulk-delete is too fast to reliably catch with SIGINT at scale
    // suitable for a dispatch test. --rate-limit-objects has hard floor 10
    // and must be >= --batch-size (default 200), so the practical minimum
    // throttle is 10/sec with --batch-size 10. With 200 seeded objects the
    // theoretical duration is ~20s, but the leaky-bucket token allowance
    // and concurrent batch deletion mean the first delete can drain the
    // bucket fast enough that exit 0 (normal completion) races SIGINT
    // (exit 130) — observed in practice. Per the spec's section-7 fallback,
    // soften to "process exits, doesn't hang." Strict exit-130 coverage
    // lives in cancel_clean_sigint_mid_deletion_exits_130 below and in
    // cp/mv (per-byte bandwidth throttle on a 30 MiB transfer is reliable).
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    for i in 0..200 {
        helper
            .put_object(&bucket, &format!("k{i:04}"), b"x".to_vec())
            .await;
    }

    let target = format!("s3://{bucket}/");
    let mut cmd = s7cmd_cmd();
    cmd.args([
        "clean",
        "--target-profile",
        "s7cmd-e2e-test",
        "--target-region",
        REGION,
        "--force",
        "--batch-size",
        "10",
        "--rate-limit-objects",
        "10",
        &target,
    ]);

    let _code = run_with_sigint(&mut cmd).await;

    helper.delete_bucket_with_cascade(&bucket).await;
}

// ---- cp ----

#[tokio::test]
async fn cancel_cp_sigint_exits_130() {
    let _serial = SIGINT_TEST_LOCK.lock().await;
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let local_dir = create_temp_dir();
    let big = create_sized_file(&local_dir, "big.bin", 30 * 1024 * 1024);
    let target = format!("s3://{bucket}/big.bin");

    let mut cmd = s7cmd_cmd();
    cmd.args([
        "cp",
        "--target-profile",
        "s7cmd-e2e-test",
        "--target-region",
        REGION,
        "--rate-limit-bandwidth",
        "2MiB",
        big.to_str().unwrap(),
        &target,
    ]);

    let code = run_with_sigint(&mut cmd).await;
    assert_eq!(code, Some(130), "cp SIGINT must exit 130; got {code:?}");

    helper.abort_all_multipart_uploads(&bucket).await;
    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&local_dir);
}

// ---- mv ----

#[tokio::test]
async fn cancel_mv_sigint_exits_130() {
    let _serial = SIGINT_TEST_LOCK.lock().await;
    let helper = TestHelper::new().await;
    let src_bucket = generate_bucket_name();
    let dst_bucket = generate_bucket_name();
    helper.create_bucket(&src_bucket, REGION).await;
    helper.create_bucket(&dst_bucket, REGION).await;
    helper
        .put_object(&src_bucket, "big.bin", vec![0u8; 30 * 1024 * 1024])
        .await;

    let source = format!("s3://{src_bucket}/big.bin");
    let target = format!("s3://{dst_bucket}/big.bin");
    let mut cmd = s7cmd_cmd();
    cmd.args([
        "mv",
        "--source-profile",
        "s7cmd-e2e-test",
        "--source-region",
        REGION,
        "--target-profile",
        "s7cmd-e2e-test",
        "--target-region",
        REGION,
        "--rate-limit-bandwidth",
        "2MiB",
        &source,
        &target,
    ]);

    let code = run_with_sigint(&mut cmd).await;
    assert_eq!(code, Some(130), "mv SIGINT must exit 130; got {code:?}");

    helper.abort_all_multipart_uploads(&src_bucket).await;
    helper.abort_all_multipart_uploads(&dst_bucket).await;
    helper.delete_bucket_with_cascade(&src_bucket).await;
    helper.delete_bucket_with_cascade(&dst_bucket).await;
}

// ---- cancellation-path coverage ----
//
// The two tests below differ from the "does_not_hang" family above: they
// shape the workload so SIGINT reliably lands while the operation is still
// in flight, driving the explicit cancellation returns (ls_bin's and
// clean_bin's `is_ctrl_c_received()` → SIGINT_EXIT_CODE, i.e. exit 130).
//
// NOTE on flag values: `--rate-limit-api` has a hard floor of 10
// (`10..=u32::MAX`). A smaller value is a clap error (exit 2) that kills
// the run before any S3 call — which a bare "process exited" assertion
// would silently accept. Keep throttles at or above their documented
// floors and prefer asserting a specific exit code.

/// ls with `--max-keys 1` over 150 objects forces 150 paginated
/// ListObjectsV2 calls; `--rate-limit-api 10` (the floor) spaces them
/// ~10/sec for roughly 15s of listing — comfortably longer than the largest
/// startup delay in `STARTUP_DELAYS_MS`, so SIGINT always lands mid-listing
/// and the cancellation branch (exit 130) is taken.
#[tokio::test]
async fn cancel_ls_sigint_mid_paginated_listing_exits_130() {
    let _serial = SIGINT_TEST_LOCK.lock().await;
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    for i in 0..150 {
        helper
            .put_object(&bucket, &format!("k{i:04}"), b"x".to_vec())
            .await;
    }

    let target = format!("s3://{bucket}/");
    let mut cmd = s7cmd_cmd();
    cmd.args([
        "ls",
        "--target-profile",
        "s7cmd-e2e-test",
        "--target-region",
        REGION,
        "--recursive",
        "--max-keys",
        "1",
        "--rate-limit-api",
        "10",
        &target,
    ]);

    let code = run_with_sigint(&mut cmd).await;
    assert_eq!(
        code,
        Some(130),
        "cancelled listing must exit 130 (ls cancellation path)"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}

/// clean over 300 objects with `--worker-size 1 --batch-size 10
/// --rate-limit-objects 10` keeps the deletion pipeline busy for ~29s of
/// deterministic work, so SIGINT lands mid-run at any delay in
/// `STARTUP_DELAYS_MS` (at most 9s) and drives the post-run "deletion
/// cancelled by user" branch (exit 130).
///
/// Margin math — the workload must survive `run_with_sigint`'s retry
/// escalation, which re-runs `clean` on the SAME (partially drained)
/// bucket: the single worker serializes batches, so each fresh process
/// drains at most the 10-token initial burst plus 10 objects/sec of run
/// time. Worst case across all three attempts (2s + 5s + 9s of running,
/// three bursts) is ~190 objects — well short of 300, so the final
/// attempt still interrupts a busy run. The sibling
/// `cancel_clean_sigint_does_not_hang` keeps the soft assertion for the
/// default multi-worker configuration, whose burst behavior has been
/// observed to outrun the nominal rate.
#[tokio::test]
async fn cancel_clean_sigint_mid_deletion_exits_130() {
    let _serial = SIGINT_TEST_LOCK.lock().await;
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    for i in 0..300 {
        helper
            .put_object(&bucket, &format!("k{i:04}"), b"x".to_vec())
            .await;
    }

    let target = format!("s3://{bucket}/");
    let mut cmd = s7cmd_cmd();
    cmd.args([
        "clean",
        "--target-profile",
        "s7cmd-e2e-test",
        "--target-region",
        REGION,
        "--force",
        "--worker-size",
        "1",
        "--batch-size",
        "10",
        "--rate-limit-objects",
        "10",
        &target,
    ]);

    let code = run_with_sigint(&mut cmd).await;
    assert_eq!(
        code,
        Some(130),
        "cancelled deletion must exit 130 (clean cancellation path)"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}

// batch-run's SIGINT behavior needs no AWS, so its coverage lives in
// tests/cli_sigint.rs (runs under a plain `cargo test` on Unix) rather
// than here. See that file for the documented caveat about
// `--streaming -` with a stdin pipe that never closes.
