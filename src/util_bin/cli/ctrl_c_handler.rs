// Vendored from s3util-rs@0.2.0
//   src/bin/s3util/cli/ctrl_c_handler.rs
// Adjustments: stripped #[cfg(test)] mod tests;
//              tests serialize through the process-wide
//              crate::signal_test_lock instead of a per-module semaphore
//              (s7cmd has four ctrl_c_handler test modules in one test
//              binary, and a SIGINT sent by one is broadcast to all).

use tokio::task::JoinHandle;
use tokio::{select, signal};
use tracing::{debug, error, warn};

use s3util_rs::types::token::PipelineCancellationToken;

pub fn spawn_ctrl_c_handler(cancellation_token: PipelineCancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        select! {
            _ = cancellation_token.cancelled() => {
                debug!("cancellation_token canceled.")
            }
            result = signal::ctrl_c() => {
                match result {
                    Ok(()) => {
                        warn!("ctrl-c received, shutting down.");
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
    use s3util_rs::types::token::create_pipeline_cancellation_token;

    use super::*;

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
        tokio::time::sleep(std::time::Duration::from_millis(STARTUP_MS)).await;

        nix::sys::signal::kill(nix::unistd::Pid::this(), nix::sys::signal::Signal::SIGINT).unwrap();

        join_handle.await.unwrap();
        assert!(cancellation_token.is_cancelled());
    }
}
