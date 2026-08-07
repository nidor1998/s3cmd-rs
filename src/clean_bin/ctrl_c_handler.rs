// Vendored from s3rm-rs@1.3.3
//   src/bin/s3rm/ctrl_c_handler/mod.rs
// Adjustments: flattened from ctrl_c_handler/mod.rs to ctrl_c_handler.rs;
//              stripped #[cfg(test)] mod tests;
//              CTRL_C_RECEIVED flag / is_ctrl_c_received() ported from
//              s3rm-rs@1.6.0 (nidor1998/s3rm-rs#100) so mod.rs can exit
//              130 on SIGINT; tests serialize through the process-wide
//              crate::signal_test_lock instead of a per-module semaphore
//              (s7cmd has four ctrl_c_handler test modules in one test
//              binary, and a SIGINT sent by one is broadcast to all).

// Ctrl+C signal handler adapted from s3sync's `bin/s3sync/cli/ctrl_c_handler/mod.rs`.
//
// Uses tokio::select! to wait for either pipeline cancellation or Ctrl+C signal.

use std::sync::atomic::{AtomicBool, Ordering};

use s3rm_rs::PipelineCancellationToken;
use tokio::task::JoinHandle;
use tokio::{select, signal};
use tracing::{debug, error};

static CTRL_C_RECEIVED: AtomicBool = AtomicBool::new(false);

/// Whether a Ctrl+C (SIGINT) signal has been received by the handler.
///
/// The flag is stored before the cancellation token is cancelled, so any
/// code that observes the pipeline stopping due to cancellation is
/// guaranteed to see `true` here when Ctrl+C was the cause.
pub fn is_ctrl_c_received() -> bool {
    CTRL_C_RECEIVED.load(Ordering::SeqCst)
}

pub fn spawn_ctrl_c_handler(cancellation_token: PipelineCancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        select! {
            _ = cancellation_token.cancelled() => {
                debug!("cancellation_token canceled.")
            }
            result = signal::ctrl_c() => {
                match result {
                    Ok(()) => {
                        debug!("ctrl-c received, shutting down.");
                        CTRL_C_RECEIVED.store(true, Ordering::SeqCst);
                        cancellation_token.cancel();
                    }
                    Err(e) => {
                        error!("failed to listen for ctrl-c signal: {e}");
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use crate::signal_test_lock::semaphore;
    use s3rm_rs::create_pipeline_cancellation_token;

    use super::*;

    /// The flag is process-global; tests that assert on it reset it first
    /// (while holding the signal-test lock) so they don't start from a
    /// `true` stored by an earlier test. The reset cannot fence stragglers:
    /// tests that drive the full run() paths spawn handlers without holding
    /// the lock, and an earlier lock holder's SIGINT reaches those handlers
    /// whenever their runtime next polls them — observed >100ms late.
    /// `true` is therefore asserted only after this test's own SIGINT,
    /// and "not set" is concluded only after retries (each straggler
    /// fires at most once).
    fn reset_ctrl_c_received() {
        CTRL_C_RECEIVED.store(false, Ordering::SeqCst);
    }

    #[tokio::test]
    async fn ctrl_c_handler_handles_cancellation_token() {
        let _permit = semaphore().acquire_owned().await.unwrap();

        let cancellation_token = create_pipeline_cancellation_token();
        let join_handle = spawn_ctrl_c_handler(cancellation_token.clone());
        cancellation_token.cancel();

        join_handle.await.unwrap();
        assert!(cancellation_token.is_cancelled());
    }

    #[tokio::test]
    #[cfg(target_family = "unix")]
    async fn ctrl_c_handler_handles_sigint() {
        const STARTUP_MS: u64 = 100;

        let _permit = semaphore().acquire_owned().await.unwrap();

        let cancellation_token = create_pipeline_cancellation_token();
        let join_handle = spawn_ctrl_c_handler(cancellation_token.clone());
        // Give the spawned task a moment to register the signal handler
        // before delivering SIGINT.
        tokio::time::sleep(std::time::Duration::from_millis(STARTUP_MS)).await;

        nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGINT).unwrap();

        join_handle.await.unwrap();
        assert!(cancellation_token.is_cancelled());
    }

    #[tokio::test]
    #[cfg(target_family = "unix")]
    async fn ctrl_c_received_flag_is_set_on_sigint() {
        const STARTUP_MS: u64 = 100;

        let _permit = semaphore().acquire_owned().await.unwrap();
        reset_ctrl_c_received();

        let cancellation_token = create_pipeline_cancellation_token();
        let join_handle = spawn_ctrl_c_handler(cancellation_token.clone());
        tokio::time::sleep(std::time::Duration::from_millis(STARTUP_MS)).await;

        // No `!is_ctrl_c_received()` check before the kill: a straggler
        // delivery of an earlier test's SIGINT can land inside the startup
        // sleep above (see reset_ctrl_c_received), so "still false here"
        // is not guaranteed. Only the true-direction below is stable.

        nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGINT).unwrap();

        join_handle.await.unwrap();

        assert!(is_ctrl_c_received());
        assert!(cancellation_token.is_cancelled());
    }

    /// The flag must already be visible when the token cancellation becomes
    /// observable — mod.rs relies on this ordering to decide the exit code
    /// after the pipeline stops.
    #[tokio::test]
    #[cfg(target_family = "unix")]
    async fn ctrl_c_received_flag_is_visible_once_token_is_cancelled() {
        const STARTUP_MS: u64 = 100;

        let _permit = semaphore().acquire_owned().await.unwrap();
        reset_ctrl_c_received();

        let cancellation_token = create_pipeline_cancellation_token();
        let join_handle = spawn_ctrl_c_handler(cancellation_token.clone());
        tokio::time::sleep(std::time::Duration::from_millis(STARTUP_MS)).await;

        nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGINT).unwrap();

        cancellation_token.cancelled().await;
        assert!(is_ctrl_c_received());

        join_handle.await.unwrap();
    }

    /// The handler's cancellation arm must not store the flag. A single
    /// observation can be contaminated by a straggler SIGINT delivery from
    /// an earlier test (see reset_ctrl_c_received), and each straggler
    /// fires at most once — so retry and require only that some attempt
    /// completes with the flag still clear. A regression (storing on the
    /// cancellation arm) sets the flag on every attempt and still fails.
    #[tokio::test]
    async fn ctrl_c_received_flag_is_not_set_on_token_cancellation() {
        const ATTEMPTS: usize = 5;

        let _permit = semaphore().acquire_owned().await.unwrap();

        for attempt in 1..=ATTEMPTS {
            reset_ctrl_c_received();

            let cancellation_token = create_pipeline_cancellation_token();

            let join_handle = spawn_ctrl_c_handler(cancellation_token.clone());
            cancellation_token.cancel();

            join_handle.await.unwrap();

            if !is_ctrl_c_received() {
                return;
            }
            eprintln!("straggler SIGINT contaminated attempt {attempt}/{ATTEMPTS}; retrying");
        }
        panic!(
            "token cancellation must not set CTRL_C_RECEIVED \
             (flag was set on all {ATTEMPTS} attempts)"
        );
    }
}
