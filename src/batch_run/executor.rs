//! Sequential and parallel execution loops.

use crate::batch_run::progress::Progress;
use crate::batch_run::redact::redact_secrets;
use crate::batch_run::summary::Summary;
use crate::cli::Cmd;

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use futures::FutureExt;
use tokio::sync::Semaphore;
use tokio::task::{JoinSet, LocalSet};

// The future returned by a dispatched command is intentionally NOT
// `Send`: upstream subcommand implementations (s3util_rs, s3sync) hold
// non-`Send` types (`&dyn StorageTrait`, `Option<&dyn Error>`) across
// `.await` points. The closure itself is `Send + Sync` so it can be
// shared across worker tasks; the parallel executor uses
// `LocalSet::spawn_local` so its returned future does not need to be
// `Send`.
pub type DispatchFn = Arc<dyn Fn(Cmd) -> Pin<Box<dyn Future<Output = i32>>> + Send + Sync>;

pub struct PreparedLine {
    pub line_no: usize,
    /// The original input line, used by `log_start` / `log_end` to
    /// identify which subcommand each per-line event belongs to.
    pub raw: String,
    pub kind: PreparedLineKind,
}

/// Why a line failed parse / validate. Used as the `invalid_kind`
/// structured field on `event = "invalid"` log entries so consumers can
/// filter by failure category. `as_str` returns a stable, lowercase
/// identifier that doubles as the field's serialized value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidKind {
    /// `parser::tokenize_line` rejected the line (e.g. malformed quoting,
    /// unsupported escape).
    Tokenize,
    /// `clap::Parser` rejected the tokenized argv (unknown subcommand,
    /// missing required arg, …).
    Parse,
    /// Tokenized + parsed cleanly but produced no subcommand
    /// (`Cli { command: None, .. }`).
    Empty,
    /// `validate::validate` rejected the parsed `Cmd` (nested batch-run,
    /// stdin/stdout transfer inside batch-run, per-line tracing flag, …).
    Validate,
}

impl InvalidKind {
    pub fn as_str(self) -> &'static str {
        match self {
            InvalidKind::Tokenize => "tokenize",
            InvalidKind::Parse => "parse",
            InvalidKind::Empty => "empty",
            InvalidKind::Validate => "validate",
        }
    }
}

/// Per-line outcome of the parse + validate stage. `Cmd` is a clean
/// line ready to dispatch; `Invalid` is a line that failed to tokenize,
/// failed clap parsing, was empty, or failed validation. Invalid lines
/// are still scheduled by the executor — they synthesize exit code 2
/// (logged at error level, bucketed as `failed`) so they count toward
/// `--max-errors` and `--continue-on-error` like any other failure
/// instead of unconditionally aborting the whole run.
pub enum PreparedLineKind {
    /// `Cmd` is boxed because the `cli::Cmd` enum is ~1 KiB (some
    /// upstream args structs are large). Boxing keeps the
    /// `PreparedLineKind` enum small so the much-more-common
    /// `Invalid` variant doesn't carry around an unused 1 KiB.
    Cmd(Box<Cmd>),
    /// `reason` carries only the underlying error text — no `"line N: "`
    /// prefix and no trailing `\n  > {raw}` (the executor logs `line` and
    /// `raw` as separate fields). `kind` distinguishes which of the four
    /// stages produced the failure.
    Invalid { kind: InvalidKind, reason: String },
}

/// Synthetic exit code used when an `Invalid` line is "executed":
/// matches clap's convention for argument parsing errors. Counted as a
/// failure by `Summary::record` and `Progress::tick`, and stop-worthy
/// in `is_stop_worthy`, so it flows through `--max-errors` /
/// `--continue-on-error` like any runtime failure.
pub const EXIT_CODE_INVALID_LINE: i32 = 2;

/// Emitted at info level immediately before each dispatched
/// subcommand. Visible with `-v`.
fn log_start(line_no: usize, raw: &str) {
    let raw = raw.trim_end();
    tracing::info!(
        line = line_no,
        event = "start",
        command = command_token(raw),
        raw = redact_secrets(raw).as_ref(),
        "line started",
    );
}

/// Drive one prepared line to completion. For `Cmd`, runs the start /
/// dispatch / end sequence and returns the dispatched exit code. For
/// `Invalid`, emits a structured `event = "invalid"` log entry with
/// `invalid_kind`, `reason`, `line`, `raw`, and `exit_code` as separate
/// fields, then returns the synthetic `EXIT_CODE_INVALID_LINE` (= 2).
/// Returning `(line_no, code)` lets callers record / tick / threshold-
/// check uniformly without inspecting the variant.
///
/// The dispatched future is wrapped in `catch_unwind` so a panic in any
/// subcommand becomes a recorded failure with full line / raw / command
/// identification — instead of unwinding through the executor (which in
/// sequential mode meant aborting the whole `batch-run` process with no
/// summary, no log entry, and no chance to apply `--max-errors` /
/// `--continue-on-error`). `AssertUnwindSafe` is sound here: nothing we
/// mutate outside the dispatch future crosses the catch boundary. All
/// build profiles in this crate use `panic = "unwind"`, so the recovery
/// is always active — `catch_unwind` is a no-op under `panic = "abort"`,
/// which we deliberately do not opt into anywhere.
async fn execute_line(line: PreparedLine, dispatch: &DispatchFn) -> (usize, i32) {
    let PreparedLine { line_no, raw, kind } = line;
    let code = match kind {
        PreparedLineKind::Cmd(cmd) => {
            log_start(line_no, &raw);
            match AssertUnwindSafe(dispatch(*cmd)).catch_unwind().await {
                Ok(code) => {
                    log_end(line_no, &raw, code);
                    code
                }
                Err(payload) => {
                    let raw_trimmed = raw.trim_end();
                    let msg = panic_message(&*payload);
                    tracing::error!(
                        line = line_no,
                        event = "panicked",
                        exit_code = EXIT_CODE_PANIC,
                        command = command_token(raw_trimmed),
                        raw = redact_secrets(raw_trimmed).as_ref(),
                        panic = msg.as_str(),
                        "subcommand panicked",
                    );
                    EXIT_CODE_PANIC
                }
            }
        }
        PreparedLineKind::Invalid { kind, reason } => {
            let raw = raw.trim_end();
            tracing::error!(
                line = line_no,
                event = "invalid",
                invalid_kind = kind.as_str(),
                reason = reason,
                exit_code = EXIT_CODE_INVALID_LINE,
                command = command_token(raw),
                raw = redact_secrets(raw).as_ref(),
                "line invalid",
            );
            EXIT_CODE_INVALID_LINE
        }
    };
    (line_no, code)
}

/// Synthetic exit code surfaced when a dispatched subcommand panics.
/// Matches the conventional Unix code for an uncaught Rust panic
/// (`std::process::abort` / `SIGABRT` is 134, but the language-level
/// "panic produced by `panic!`" exit on most platforms is 101). Counted
/// as a failure by `Summary::record` and stop-worthy by `is_stop_worthy`,
/// so panics flow through `--max-errors` / `--continue-on-error` like
/// any runtime failure.
pub const EXIT_CODE_PANIC: i32 = 101;

/// Best-effort extraction of the panic message from a `catch_unwind`
/// payload. The payload is whatever was passed to `panic!` — usually a
/// `&'static str` or `String`. Any other payload type (a structured
/// type set via `panic_any`, etc.) falls back to a placeholder so the
/// log entry still carries something the user can grep on.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "unknown panic payload".to_string()
}

/// Emitted immediately after each dispatched subcommand. The level
/// matches the outcome so non-zero exits are visible at the default
/// `warn` verbosity without needing `-v`:
///   - exit 0 → info (`success`)
///   - exit 3 / 4 (`EXIT_CODE_WARNING`, `EXIT_CODE_NOT_FOUND`) → warn
///   - exit 130 (SIGINT, returned by per-subcommand cancellation
///     handlers when the user hits Ctrl-C) → warn (`skipped`) — visible
///     at the default verbosity so the user can see which lines were
///     interrupted, but not error level since the user asked for it.
///   - any other non-zero exit → error (`failure`)
///
/// The 3/4/130 literals avoid a cross-module dep just for three numbers;
/// each bucket matches the progress-bar / summary bucket the code feeds.
fn log_end(line_no: usize, raw: &str, code: i32) {
    let trimmed = raw.trim_end();
    let command = command_token(trimmed);
    let redacted = redact_secrets(trimmed);
    let raw = redacted.as_ref();
    match code {
        0 => tracing::info!(
            line = line_no,
            event = "success",
            exit_code = code,
            command = command,
            raw = raw,
            "line completed",
        ),
        3 | 4 => tracing::warn!(
            line = line_no,
            event = "warning",
            exit_code = code,
            command = command,
            raw = raw,
            "line warning",
        ),
        130 => tracing::warn!(
            line = line_no,
            event = "skipped",
            exit_code = code,
            command = command,
            raw = raw,
            "line skipped",
        ),
        _ => tracing::error!(
            line = line_no,
            event = "failure",
            exit_code = code,
            command = command,
            raw = raw,
            "line failed",
        ),
    }
}

/// Leading whitespace-delimited token of `raw`, used as the `command`
/// field on per-line events. Returns `None` when `raw` is empty so the
/// `command` field is simply omitted (tracing's `Option` field-value
/// support drops `None` automatically).
fn command_token(raw: &str) -> Option<&str> {
    raw.split_whitespace().next()
}

#[derive(Debug, Clone, Copy)]
pub struct ExecutorOptions {
    pub workers: usize, // resolved (1 = sequential)
    /// `Some(N)` = stop spawning new commands once `N` failures have
    /// been recorded (graceful: in-flight commands complete). `None` =
    /// `--continue-on-error` (no failure-driven stop). Default mapping
    /// performed in `batch_run::run`: no flag → `Some(1)` (fail-fast),
    /// `--max-errors N` → `Some(N)`, `--continue-on-error` → `None`.
    pub error_threshold: Option<u64>,
    /// `--continue-on-warning`: per-line exit codes 3 and 4 are
    /// classified as warnings and do NOT count toward `error_threshold`.
    /// Other non-zero exits still count.
    pub continue_on_warning: bool,
    pub streaming: bool,
    pub no_progress: bool,
}

/// Returns true when this exit code should count toward the
/// `error_threshold`. Zero never counts. Exit codes 3
/// (`EXIT_CODE_WARNING`) and 4 (`EXIT_CODE_NOT_FOUND`) — kept literal
/// to avoid a cross-module dep just for two numbers — only count when
/// `--continue-on-warning` is NOT set. Exit code 130 (SIGINT, returned
/// by per-subcommand cancellation handlers when the user hits Ctrl-C)
/// never counts: SIGINT already breaks the spawn loop via the shared
/// interrupt flag, and the cancellation is the user's intent — not a
/// failure that should trip `--max-errors`.
fn is_stop_worthy(code: i32, continue_on_warning: bool) -> bool {
    match code {
        0 | 130 => false,
        3 | 4 => !continue_on_warning,
        _ => true,
    }
}

/// Severity ranking for picking the "worst" exit code across a batch
/// run. Higher = worse. Numeric `>` on the raw codes is wrong because
/// 130 (SIGINT) and 4 (not found) would outrank 1 (general error),
/// which is the most actionable failure. Ordering: 1 > 2 > 3 > 4 >
/// any other non-zero (101, 130, …) > 0.
fn severity_rank(code: i32) -> u32 {
    match code {
        0 => 0,
        1 => 5,
        2 => 4,
        3 => 3,
        4 => 2,
        _ => 1,
    }
}

/// Returns whichever of `a` / `b` ranks higher by `severity_rank`.
/// Ties keep `a` (the existing accumulator), so callers can write
/// `worst = worse_of(worst, code)` without surprises.
fn worse_of(a: i32, b: i32) -> i32 {
    if severity_rank(b) > severity_rank(a) {
        b
    } else {
        a
    }
}

/// Shared interrupt flag. Set by the SIGINT handler installed in
/// `batch_run::run`; checked by both executors to stop spawning new
/// commands. Per-subcommand cancellation tokens (registered inside
/// each dispatched command) handle propagation into in-flight transfers.
pub type Interrupt = Arc<AtomicBool>;

pub async fn run_sequential(
    lines: Vec<PreparedLine>,
    opts: ExecutorOptions,
    dispatch: DispatchFn,
    interrupt: Interrupt,
) -> (i32, Summary) {
    let total = lines.len() as u64;
    let mut progress = Progress::new(
        total,
        Progress::should_show(opts.streaming, opts.no_progress),
    );
    let mut summary = Summary::default();
    let start = Instant::now();
    let mut worst = 0i32;
    // Local stop-worthy counter, distinct from `summary.failed` (which
    // still counts every non-zero exit). With `--continue-on-warning`
    // this skips exit codes 3 and 4.
    let mut fail_count: u64 = 0;

    for (idx, line) in lines.into_iter().enumerate() {
        // Bail out if SIGINT arrived between commands (regardless of
        // error threshold — interrupt is unconditional). `+=` so that
        // any per-line exit-130 skips already counted via `record()`
        // are preserved alongside the never-dispatched count.
        if interrupt.load(Ordering::SeqCst) {
            let processed = idx as u64;
            summary.skipped += total.saturating_sub(processed);
            break;
        }
        let (_, code) = execute_line(line, &dispatch).await;
        progress.tick(code);
        summary.record(code);
        worst = worse_of(worst, code);
        if is_stop_worthy(code, opts.continue_on_warning) {
            fail_count += 1;
            if opts.error_threshold.is_some_and(|t| fail_count >= t) {
                let processed = (idx + 1) as u64;
                summary.skipped += total.saturating_sub(processed);
                break;
            }
        }
    }

    finish_or_abandon(&progress, &summary);
    summary.elapsed = start.elapsed();
    (worst, summary)
}

pub async fn run_parallel(
    lines: Vec<PreparedLine>,
    opts: ExecutorOptions,
    dispatch: DispatchFn,
    interrupt: Interrupt,
) -> (i32, Summary) {
    let total = lines.len() as u64;
    let progress = Arc::new(tokio::sync::Mutex::new(Progress::new(
        total,
        Progress::should_show(opts.streaming, opts.no_progress),
    )));
    let mut summary = Summary::default();
    let start = Instant::now();

    let sem = Arc::new(Semaphore::new(opts.workers));
    let fail_cancel = Arc::new(AtomicBool::new(false));
    let fail_count = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut spawned = 0u64;

    // Dispatched futures hold non-`Send` types (e.g. `&dyn
    // StorageTrait`) across `.await` points, so we cannot use
    // `tokio::spawn` / `JoinSet::spawn`. A `LocalSet` lets us drive
    // many non-`Send` futures concurrently on the current thread; the
    // semaphore still bounds active workers to `opts.workers`.
    let local = LocalSet::new();
    let worst = local
        .run_until(async {
            let mut joinset: JoinSet<(usize, i32)> = JoinSet::new();
            for line in lines {
                // SIGINT (interrupt) unconditionally stops spawning.
                if interrupt.load(Ordering::SeqCst) {
                    break;
                }
                // Failure-driven cancel: tasks set `fail_cancel` only after
                // the per-run error threshold has been crossed (None =
                // unbounded → never set).
                if fail_cancel.load(Ordering::SeqCst) {
                    break;
                }
                let permit = sem.clone().acquire_owned().await.expect("sem closed");
                // Re-check after the await: a SIGINT that landed while this
                // loop was parked on the semaphore must not dispatch one more
                // line — that line's freshly installed Ctrl+C handler would
                // never see the already-delivered signal, so it would run to
                // full completion yet read the process-global interruption
                // state stored by a sibling line and misreport as skipped.
                if interrupt.load(Ordering::SeqCst) {
                    break;
                }
                let dispatch = Arc::clone(&dispatch);
                let progress = Arc::clone(&progress);
                let fail_cancel = Arc::clone(&fail_cancel);
                let fail_count = Arc::clone(&fail_count);
                let error_threshold = opts.error_threshold;
                let continue_on_warning = opts.continue_on_warning;
                spawned += 1;
                joinset.spawn_local(async move {
                    let _permit = permit;
                    let (line_no, code) = execute_line(line, &dispatch).await;
                    if is_stop_worthy(code, continue_on_warning) {
                        let new_count = fail_count.fetch_add(1, Ordering::SeqCst) + 1;
                        if let Some(threshold) = error_threshold
                            && new_count >= threshold
                        {
                            fail_cancel.store(true, Ordering::SeqCst);
                        }
                    }
                    progress.lock().await.tick(code);
                    (line_no, code)
                });
            }

            let mut worst = 0i32;
            while let Some(joined) = joinset.join_next().await {
                match joined {
                    Ok((_, code)) => {
                        summary.record(code);
                        worst = worse_of(worst, code);
                    }
                    Err(e) => {
                        tracing::error!(error = format!("{e:#}"), "task panicked");
                        summary.record(1);
                        worst = worse_of(worst, 1);
                    }
                }
            }
            worst
        })
        .await;

    // `+=`: any per-line exit-130 skips already counted via `record()`
    // are preserved alongside the never-spawned count.
    summary.skipped += total.saturating_sub(spawned);
    {
        let p = progress.lock().await;
        if summary.skipped > 0 {
            p.abandon();
        } else {
            p.finish();
        }
    }
    summary.elapsed = start.elapsed();
    (worst, summary)
}

/// Streaming sequential executor. Identical semantics to
/// [`run_sequential`], except lines arrive on a channel rather than as a
/// pre-built `Vec`. Total is unknown so the progress bar is unconditionally
/// off (matching `Progress::should_show`); skipped count is accumulated as
/// drain count when fail-fast or interrupt trips.
pub async fn run_sequential_streaming(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<PreparedLine>,
    opts: ExecutorOptions,
    dispatch: DispatchFn,
    interrupt: Interrupt,
) -> (i32, Summary) {
    let mut progress = Progress::new(0, false);
    let mut summary = Summary::default();
    let start = Instant::now();
    let mut worst = 0i32;
    // Local stop-worthy counter; see `run_sequential` for rationale.
    let mut fail_count: u64 = 0;

    while let Some(line) = rx.recv().await {
        if interrupt.load(Ordering::SeqCst) {
            summary.skipped += 1;
            // Drain remaining items the reader has already produced.
            while rx.recv().await.is_some() {
                summary.skipped += 1;
            }
            break;
        }
        let (_, code) = execute_line(line, &dispatch).await;
        progress.tick(code);
        summary.record(code);
        worst = worse_of(worst, code);
        if is_stop_worthy(code, opts.continue_on_warning) {
            fail_count += 1;
            if opts.error_threshold.is_some_and(|t| fail_count >= t) {
                // Drain remaining items already in the channel as skipped.
                while rx.recv().await.is_some() {
                    summary.skipped += 1;
                }
                break;
            }
        }
    }

    finish_or_abandon(&progress, &summary);
    summary.elapsed = start.elapsed();
    (worst, summary)
}

/// Streaming parallel executor. Identical semantics to [`run_parallel`],
/// except lines arrive on a channel rather than as a pre-built `Vec`.
/// Dispatched futures hold non-`Send` types, so a `LocalSet` drives them
/// on the current thread.
pub async fn run_parallel_streaming(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<PreparedLine>,
    opts: ExecutorOptions,
    dispatch: DispatchFn,
    interrupt: Interrupt,
) -> (i32, Summary) {
    let progress = Arc::new(tokio::sync::Mutex::new(Progress::new(0, false)));
    let mut summary = Summary::default();
    let start = Instant::now();

    let sem = Arc::new(Semaphore::new(opts.workers));
    let fail_cancel = Arc::new(AtomicBool::new(false));
    let fail_count = Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Use LocalSet because dispatch futures are not Send.
    let local = LocalSet::new();
    let worst = local
        .run_until(async {
            let mut joinset: JoinSet<(usize, i32)> = JoinSet::new();
            loop {
                // SIGINT (interrupt) unconditionally stops spawning.
                if interrupt.load(Ordering::SeqCst) {
                    break;
                }
                // Failure-driven cancel: tasks set `fail_cancel` only after
                // the per-run error threshold has been crossed (None =
                // unbounded → never set).
                if fail_cancel.load(Ordering::SeqCst) {
                    break;
                }

                // Pull the next line from the channel, OR notice channel
                // closed (meaning reader is done).
                let line = match rx.recv().await {
                    Some(line) => line,
                    None => break, // channel closed, reader finished
                };

                let permit = sem.clone().acquire_owned().await.expect("sem closed");
                // Re-check after the awaits: a SIGINT that landed while this
                // loop was parked on `rx.recv()` or the semaphore must not
                // dispatch one more line — that line's freshly installed
                // Ctrl+C handler would never see the already-delivered
                // signal, so it would run to full completion yet read the
                // process-global interruption state stored by a sibling line
                // and misreport as skipped. The line pulled above never
                // spawns; count it skipped like the post-loop drain does.
                if interrupt.load(Ordering::SeqCst) {
                    summary.skipped += 1;
                    break;
                }
                let dispatch = Arc::clone(&dispatch);
                let progress = Arc::clone(&progress);
                let fail_cancel = Arc::clone(&fail_cancel);
                let fail_count = Arc::clone(&fail_count);
                let error_threshold = opts.error_threshold;
                let continue_on_warning = opts.continue_on_warning;
                joinset.spawn_local(async move {
                    let _permit = permit;
                    let (line_no, code) = execute_line(line, &dispatch).await;
                    if is_stop_worthy(code, continue_on_warning) {
                        let new_count = fail_count.fetch_add(1, Ordering::SeqCst) + 1;
                        if let Some(threshold) = error_threshold
                            && new_count >= threshold
                        {
                            fail_cancel.store(true, Ordering::SeqCst);
                        }
                    }
                    progress.lock().await.tick(code);
                    (line_no, code)
                });
            }

            // Spawn loop ended. Drain any remaining channel items as skipped.
            while rx.recv().await.is_some() {
                summary.skipped += 1;
            }

            // Wait for all spawned tasks to finish.
            let mut worst = 0i32;
            while let Some(joined) = joinset.join_next().await {
                match joined {
                    Ok((_, code)) => {
                        summary.record(code);
                        worst = worse_of(worst, code);
                    }
                    Err(e) => {
                        tracing::error!(error = format!("{e:#}"), "task panicked");
                        summary.record(1);
                        worst = worse_of(worst, 1);
                    }
                }
            }
            worst
        })
        .await;

    {
        let p = progress.lock().await;
        if summary.skipped > 0 {
            p.abandon();
        } else {
            p.finish();
        }
    }
    summary.elapsed = start.elapsed();
    (worst, summary)
}

/// `finish()` makes the bar jump to 100%; `abandon()` leaves it at the
/// current position. Pick based on whether anything was skipped — if so,
/// the run was cut short (fail-fast or interrupt) and a "100%" finale
/// would be misleading.
fn finish_or_abandon(progress: &Progress, summary: &Summary) {
    if summary.skipped > 0 {
        progress.abandon();
    } else {
        progress.finish();
    }
}

/// Resolve `--parallel 0` to `num_cpus`. Otherwise pass through.
pub fn resolve_workers(parallel: usize) -> usize {
    if parallel == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    } else {
        parallel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn fake_dispatch(codes: Vec<i32>) -> DispatchFn {
        let codes = Arc::new(tokio::sync::Mutex::new(codes.into_iter()));
        Arc::new(move |_cmd: Cmd| {
            let codes = Arc::clone(&codes);
            Box::pin(async move {
                let mut g = codes.lock().await;
                g.next().expect("not enough fake codes")
            })
        })
    }

    /// Dispatch fn whose first invocation panics — used to drive the
    /// `Err(e)` join arm in the parallel executors (the panic recovery
    /// path that records a synthetic failure and bumps the worst exit
    /// code to 1).
    fn panicking_dispatch() -> DispatchFn {
        Arc::new(|_cmd: Cmd| Box::pin(async move { panic!("synthetic panic for test") }))
    }

    fn make_lines(n: usize) -> Vec<PreparedLine> {
        (0..n)
            .map(|i| PreparedLine {
                line_no: i + 1,
                raw: format!("create-bucket s3://b{i}"),
                kind: PreparedLineKind::Cmd(Box::new(
                    crate::cli::Cli::try_parse_from(["s7cmd", "create-bucket", "s3://b"])
                        .unwrap()
                        .command
                        .unwrap(),
                )),
            })
            .collect()
    }

    /// Mix of `Cmd` and `Invalid` lines, useful for verifying that
    /// `--max-errors` counts parse failures alongside runtime failures.
    /// The `cmd_codes` slice supplies the dispatch result for each `Cmd`
    /// line in order; `Invalid` lines do not consume an entry.
    fn make_mixed_lines(kinds: &[bool]) -> Vec<PreparedLine> {
        // `kinds[i] == true` → Cmd, `false` → Invalid.
        kinds
            .iter()
            .enumerate()
            .map(|(i, is_cmd)| {
                let line_no = i + 1;
                let raw = format!("line-{line_no}");
                let kind = if *is_cmd {
                    PreparedLineKind::Cmd(Box::new(
                        crate::cli::Cli::try_parse_from(["s7cmd", "create-bucket", "s3://b"])
                            .unwrap()
                            .command
                            .unwrap(),
                    ))
                } else {
                    PreparedLineKind::Invalid {
                        kind: InvalidKind::Parse,
                        reason: "synthetic test failure".to_string(),
                    }
                };
                PreparedLine { line_no, raw, kind }
            })
            .collect()
    }

    /// `error_threshold = Some(1)` is the historical "fail-fast" mode
    /// (no flag); `None` is `--continue-on-error`; `Some(N>1)` is
    /// `--max-errors N`.
    fn opts(workers: usize, error_threshold: Option<u64>) -> ExecutorOptions {
        ExecutorOptions {
            workers,
            error_threshold,
            continue_on_warning: false,
            streaming: false,
            no_progress: false,
        }
    }

    fn opts_warn(workers: usize, error_threshold: Option<u64>) -> ExecutorOptions {
        ExecutorOptions {
            workers,
            error_threshold,
            continue_on_warning: true,
            streaming: false,
            no_progress: false,
        }
    }

    fn no_interrupt() -> Interrupt {
        Arc::new(AtomicBool::new(false))
    }

    fn already_interrupted() -> Interrupt {
        Arc::new(AtomicBool::new(true))
    }

    #[tokio::test]
    async fn sequential_all_ok() {
        let lines = make_lines(3);
        let (code, summary) = run_sequential(
            lines,
            opts(1, Some(1)),
            fake_dispatch(vec![0, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 0);
        assert_eq!(summary.ok, 3);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }

    #[tokio::test]
    async fn sequential_fail_fast() {
        let lines = make_lines(5);
        let (code, summary) = run_sequential(
            lines,
            opts(1, Some(1)),
            fake_dispatch(vec![0, 1]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1);
        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 3);
    }

    #[tokio::test]
    async fn sequential_continue_on_error() {
        let lines = make_lines(4);
        let (code, summary) = run_sequential(
            lines,
            opts(1, None),
            fake_dispatch(vec![0, 1, 0, 4]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1); // exit 1 outranks exit 4 by severity
        assert_eq!(summary.ok, 2);
        assert_eq!(summary.failed, 1); // exit 1
        assert_eq!(summary.warning, 1); // exit 4
        assert_eq!(summary.skipped, 0);
    }

    #[tokio::test]
    async fn sequential_interrupt_stops_before_first_command() {
        let lines = make_lines(3);
        let (code, summary) = run_sequential(
            lines,
            opts(1, None), // even with continue_on_error
            fake_dispatch(vec![]),
            already_interrupted(),
        )
        .await;
        assert_eq!(code, 0);
        assert_eq!(summary.ok, 0);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 3);
    }

    #[tokio::test]
    async fn parallel_all_ok() {
        let lines = make_lines(4);
        let (code, summary) = run_parallel(
            lines,
            opts(2, Some(1)),
            fake_dispatch(vec![0, 0, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 0);
        assert_eq!(summary.ok, 4);
    }

    #[tokio::test]
    async fn parallel_fail_fast_skips_nothing_already_in_flight() {
        // With workers=2 and 5 lines, first 2 spawn immediately. Once
        // they finish, more spawn. If line 1 fails and continue_on_error
        // is false, lines 3-5 are skipped.
        let lines = make_lines(5);
        let (code, _) = run_parallel(
            lines,
            opts(2, Some(1)),
            fake_dispatch(vec![1, 0, 0, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1);
        // We don't assert exact skipped count because parallel ordering
        // is racy — what matters is that fail_fast did stop the spawn loop.
    }

    #[test]
    fn resolve_workers_zero_picks_num_cpus() {
        let n = resolve_workers(0);
        assert!(n >= 1);
    }

    #[test]
    fn resolve_workers_passthrough() {
        assert_eq!(resolve_workers(1), 1);
        assert_eq!(resolve_workers(8), 8);
    }

    #[test]
    fn worse_of_orders_codes_by_severity_not_numeric_value() {
        // 1 is the most severe outcome.
        assert_eq!(worse_of(0, 1), 1);
        assert_eq!(worse_of(1, 130), 1);
        assert_eq!(worse_of(130, 1), 1);
        assert_eq!(worse_of(1, 4), 1);
        assert_eq!(worse_of(4, 1), 1);
        // 1 > 2 > 3 > 4.
        assert_eq!(worse_of(2, 4), 2);
        assert_eq!(worse_of(3, 4), 3);
        // Any other non-zero (e.g. 130, 101) outranks success but not 1-4.
        assert_eq!(worse_of(0, 130), 130);
        assert_eq!(worse_of(4, 130), 4);
        assert_eq!(worse_of(101, 4), 4);
        // Identity / ties keep the accumulator (left arg).
        assert_eq!(worse_of(0, 0), 0);
        assert_eq!(worse_of(1, 1), 1);
    }

    // ---- streaming variants ----

    #[tokio::test]
    async fn sequential_streaming_all_ok() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(3) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) = run_sequential_streaming(
            rx,
            opts(1, Some(1)),
            fake_dispatch(vec![0, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 0);
        assert_eq!(summary.ok, 3);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }

    #[tokio::test]
    async fn sequential_streaming_fail_fast_drains_channel() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(5) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) = run_sequential_streaming(
            rx,
            opts(1, Some(1)),
            fake_dispatch(vec![0, 1]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1);
        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 3);
    }

    #[tokio::test]
    async fn sequential_streaming_continue_on_error() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(4) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) = run_sequential_streaming(
            rx,
            opts(1, None),
            fake_dispatch(vec![0, 1, 0, 4]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1); // exit 1 outranks exit 4 by severity
        assert_eq!(summary.ok, 2);
        assert_eq!(summary.failed, 1); // exit 1
        assert_eq!(summary.warning, 1); // exit 4
        assert_eq!(summary.skipped, 0);
    }

    #[tokio::test]
    async fn sequential_streaming_interrupt_skips_all() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(3) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) = run_sequential_streaming(
            rx,
            opts(1, None),
            fake_dispatch(vec![]),
            already_interrupted(),
        )
        .await;
        assert_eq!(code, 0);
        assert_eq!(summary.ok, 0);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 3);
    }

    #[tokio::test]
    async fn parallel_streaming_all_ok() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(4) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) = run_parallel_streaming(
            rx,
            opts(2, Some(1)),
            fake_dispatch(vec![0, 0, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 0);
        assert_eq!(summary.ok, 4);
    }

    #[tokio::test]
    async fn parallel_streaming_fail_fast_stops_spawning() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(5) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, _) = run_parallel_streaming(
            rx,
            opts(2, Some(1)),
            fake_dispatch(vec![1, 0, 0, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1);
        // We don't assert exact skipped count because parallel ordering
        // is racy — what matters is that fail_fast did stop the spawn loop.
    }

    // ---- --max-errors threshold ----

    /// Threshold of 2: the run continues past the first failure (line 1
    /// fails, line 2 ok, line 3 fails → threshold reached) and stops
    /// before line 4. Sequential ordering makes the skip count exact.
    #[tokio::test]
    async fn sequential_max_errors_two_stops_after_second_failure() {
        let lines = make_lines(5);
        let (code, summary) = run_sequential(
            lines,
            opts(1, Some(2)),
            fake_dispatch(vec![1, 0, 1, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1);
        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.skipped, 2); // lines 4 and 5 not run
    }

    /// Threshold of 3 with only two failures across the whole run: the
    /// threshold is never reached, every line executes, and nothing is
    /// skipped.
    #[tokio::test]
    async fn sequential_max_errors_three_runs_to_completion_with_two_failures() {
        let lines = make_lines(4);
        let (code, summary) = run_sequential(
            lines,
            opts(1, Some(3)),
            fake_dispatch(vec![0, 1, 0, 1]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1);
        assert_eq!(summary.ok, 2);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.skipped, 0);
    }

    /// Same shape in the streaming sequential executor.
    #[tokio::test]
    async fn sequential_streaming_max_errors_two_stops_after_second_failure() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(5) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) = run_sequential_streaming(
            rx,
            opts(1, Some(2)),
            fake_dispatch(vec![1, 0, 1, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1);
        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.skipped, 2);
    }

    /// Parallel executor with workers=2 and threshold=2: at least one of
    /// the trailing lines must be skipped because the spawn loop stops
    /// once two failures have been recorded. Exact skip count is racy
    /// (an in-flight task may complete first) so we only assert the
    /// load-bearing invariant.
    #[tokio::test]
    async fn parallel_max_errors_two_stops_spawning() {
        let lines = make_lines(8);
        let (code, summary) = run_parallel(
            lines,
            opts(2, Some(2)),
            fake_dispatch(vec![1, 1, 0, 0, 0, 0, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1);
        assert!(summary.failed >= 2);
        assert!(
            summary.skipped > 0,
            "threshold should have stopped the spawn loop; summary={summary:?}"
        );
    }

    // ---- is_stop_worthy ----

    #[test]
    fn is_stop_worthy_zero_never_counts() {
        assert!(!is_stop_worthy(0, false));
        assert!(!is_stop_worthy(0, true));
    }

    #[test]
    fn is_stop_worthy_warnings_only_count_without_continue_on_warning() {
        assert!(is_stop_worthy(3, false));
        assert!(is_stop_worthy(4, false));
        assert!(!is_stop_worthy(3, true));
        assert!(!is_stop_worthy(4, true));
    }

    #[test]
    fn is_stop_worthy_other_nonzero_always_counts() {
        for code in [1, 2, 5, 255] {
            assert!(is_stop_worthy(code, false), "code {code}");
            assert!(is_stop_worthy(code, true), "code {code}");
        }
    }

    #[test]
    fn is_stop_worthy_130_never_counts() {
        // Exit 130 is SIGINT (the user's intent, not a failure). The
        // shared interrupt flag already breaks the spawn loop; counting
        // 130 toward `--max-errors` would be redundant noise.
        assert!(!is_stop_worthy(130, false));
        assert!(!is_stop_worthy(130, true));
    }

    // ---- --continue-on-warning ----

    /// `--continue-on-warning` with default threshold (`Some(1)`):
    /// warnings (3, 4) are skipped over and the run completes; the worst
    /// exit code is still propagated. Severity ranks 3 above 4, so the
    /// worst surfaced is 3.
    #[tokio::test]
    async fn sequential_continue_on_warning_skips_warnings() {
        let lines = make_lines(4);
        let (code, summary) = run_sequential(
            lines,
            opts_warn(1, Some(1)),
            fake_dispatch(vec![0, 3, 4, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 3); // worst exit code surfaces (3 outranks 4)
        assert_eq!(summary.ok, 2);
        assert_eq!(summary.warning, 2); // exits 3 and 4
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0); // nothing skipped — warnings did NOT stop the run
    }

    /// `--continue-on-warning` still stops on a true failure (default
    /// threshold `Some(1)` = first failure stops). Preceding warnings do
    /// not exhaust the threshold.
    #[tokio::test]
    async fn sequential_continue_on_warning_stops_on_failure() {
        let lines = make_lines(5);
        let (code, summary) = run_sequential(
            lines,
            opts_warn(1, Some(1)),
            fake_dispatch(vec![3, 4, 1, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1); // exit 1 outranks the warnings (severity rule)
        assert_eq!(summary.ok, 0);
        assert_eq!(summary.warning, 2); // exits 3 and 4
        assert_eq!(summary.failed, 1); // exit 1
        assert_eq!(summary.skipped, 2); // lines 4 and 5 skipped after the failure
    }

    /// `--continue-on-warning` combined with `--max-errors 2`: warnings
    /// don't count, so the run stops on the second TRUE failure.
    #[tokio::test]
    async fn sequential_continue_on_warning_with_max_errors_two() {
        let lines = make_lines(6);
        let (code, summary) = run_sequential(
            lines,
            opts_warn(1, Some(2)),
            fake_dispatch(vec![3, 1, 4, 1, 0, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1); // exit 1 is the highest-severity outcome
        assert_eq!(summary.failed, 2); // exits 1, 1
        assert_eq!(summary.warning, 2); // exits 3, 4
        assert_eq!(summary.skipped, 2); // lines 5 and 6 skipped after second failure
    }

    /// Streaming sequential mirror.
    #[tokio::test]
    async fn sequential_streaming_continue_on_warning_skips_warnings() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(4) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) = run_sequential_streaming(
            rx,
            opts_warn(1, Some(1)),
            fake_dispatch(vec![0, 3, 4, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 3);
        assert_eq!(summary.ok, 2);
        assert_eq!(summary.warning, 2);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 0);
    }

    /// Parallel: `--continue-on-warning` lets warnings pass and the run
    /// completes when only warnings occur.
    #[tokio::test]
    async fn parallel_continue_on_warning_skips_warnings() {
        let lines = make_lines(4);
        let (code, summary) = run_parallel(
            lines,
            opts_warn(2, Some(1)),
            fake_dispatch(vec![3, 4, 3, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 3);
        // No spawn-loop stop expected: nothing skipped.
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.warning, 3); // exits 3, 4, 3
        assert_eq!(summary.failed, 0);
    }

    /// Parallel streaming mirror.
    #[tokio::test]
    async fn parallel_streaming_continue_on_warning_skips_warnings() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(4) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) = run_parallel_streaming(
            rx,
            opts_warn(2, Some(1)),
            fake_dispatch(vec![3, 4, 3, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 3);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.warning, 3);
        assert_eq!(summary.failed, 0);
    }

    // ---- Invalid lines (parse / validate failures) ----
    //
    // `Invalid` PreparedLines synthesize exit code 2, log at error
    // level, count toward `--max-errors`, and record as `failed`.

    #[tokio::test]
    async fn sequential_invalid_line_counts_as_failure_with_continue_on_error() {
        // Three Invalid lines + one Cmd, `--continue-on-error`. All four
        // must be processed; the three Invalid ones bucket as failed
        // (exit 2), and the Cmd (returning 0) buckets as ok.
        let lines = make_mixed_lines(&[false, true, false, false]);
        let (code, summary) = run_sequential(
            lines,
            opts(1, None), // continue-on-error
            fake_dispatch(vec![0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 2);
        assert_eq!(summary.ok, 1);
        assert_eq!(summary.failed, 3);
        assert_eq!(summary.skipped, 0);
    }

    #[tokio::test]
    async fn sequential_invalid_line_trips_max_errors_threshold() {
        // Six Invalid lines, `--max-errors 3`. The third Invalid line
        // should trip the threshold and skip the remaining three.
        let lines = make_mixed_lines(&[false; 6]);
        let (code, summary) = run_sequential(
            lines,
            opts(1, Some(3)),
            fake_dispatch(vec![]), // no Cmd lines, so dispatch is never called
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 2);
        assert_eq!(summary.failed, 3);
        assert_eq!(summary.skipped, 3);
    }

    #[tokio::test]
    async fn sequential_invalid_line_default_threshold_stops_after_first() {
        // Default `Some(1)`: a single Invalid line on line 1 stops
        // the run; the other four lines (2 Cmd + 2 Invalid) are
        // skipped without any dispatch call.
        let lines = make_mixed_lines(&[false, true, true, false, false]);
        let (code, summary) = run_sequential(
            lines,
            opts(1, Some(1)),
            fake_dispatch(vec![]), // dispatch must NOT be called
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 2);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.skipped, 4);
    }

    /// Parallel mirror: an Invalid line dispatched into a worker also
    /// flows through `record(2)` and trips the threshold.
    #[tokio::test]
    async fn parallel_invalid_line_counts_toward_max_errors() {
        let lines = make_mixed_lines(&[false, false, false, false]);
        let (code, summary) = run_parallel(
            lines,
            opts(2, Some(2)),
            fake_dispatch(vec![]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 2);
        assert!(summary.failed >= 2);
        // The exact skipped count is racy in parallel mode (in-flight
        // tasks may complete first), but at least one should be skipped
        // since the threshold tripped before all 4 spawned.
        assert!(
            summary.failed + summary.skipped == 4,
            "all lines accounted for; summary={summary:?}"
        );
    }

    // ---- exit 130 (SIGINT) bucketing ----
    //
    // Per-line exit 130 (returned by per-subcommand cancellation
    // handlers when the user hits Ctrl-C) is bucketed as `skipped`,
    // not `failed`, and does NOT count toward `--max-errors`. When
    // 130 is the only non-zero outcome the process exit surfaces 130;
    // when a real failure is also present, severity ranking puts the
    // failure ahead of the SIGINT.

    #[tokio::test]
    async fn sequential_exit_130_buckets_as_skipped_and_does_not_trip_threshold() {
        // Default threshold (`Some(1)`): if 130 counted, the run would
        // stop after line 1. Here every line must execute and 130s
        // accumulate in `skipped`, not `failed`.
        let lines = make_lines(4);
        let (code, summary) = run_sequential(
            lines,
            opts(1, Some(1)),
            fake_dispatch(vec![130, 0, 130, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 130);
        assert_eq!(summary.ok, 2);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.warning, 0);
        assert_eq!(summary.skipped, 2); // both 130s
    }

    /// Parallel mirror: 130s land in `skipped` and do not stop spawning.
    #[tokio::test]
    async fn parallel_exit_130_buckets_as_skipped_and_does_not_trip_threshold() {
        let lines = make_lines(4);
        let (code, summary) = run_parallel(
            lines,
            opts(2, Some(1)),
            fake_dispatch(vec![130, 0, 130, 0]),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 130);
        assert_eq!(summary.ok, 2);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 2);
    }

    /// Sequential: a real failure mid-run still trips the threshold,
    /// and the trailing `skipped` count reflects BOTH the never-
    /// dispatched lines AND any earlier 130 (via `+=`).
    #[tokio::test]
    async fn sequential_exit_130_then_failure_accumulates_skipped() {
        let lines = make_lines(5);
        let (code, summary) = run_sequential(
            lines,
            opts(1, Some(1)),
            fake_dispatch(vec![130, 1]), // line 1 = 130, line 2 = failure → stop
            no_interrupt(),
        )
        .await;
        assert_eq!(code, 1); // exit 1 outranks 130 by severity
        assert_eq!(summary.ok, 0);
        assert_eq!(summary.failed, 1); // line 2
        assert_eq!(summary.skipped, 1 + 3); // line 1 (130) + lines 3,4,5 (never run)
    }

    // ---- panic recovery in execute_line ----
    //
    // `execute_line` wraps `dispatch(...).await` in `catch_unwind`, so a
    // panic in any dispatched subcommand becomes `EXIT_CODE_PANIC` (= 101)
    // with full line / raw / command identification in the structured
    // error log. The parallel join loop's `Err(JoinError)` arm is now a
    // defense-in-depth fallback for panics that originate OUTSIDE the
    // dispatch future (in the post-dispatch bookkeeping code) — it
    // records `failed += 1` with code 1 the way it always has.

    /// `run_sequential`: a panicking dispatch is caught inside
    /// `execute_line`, surfaced as `EXIT_CODE_PANIC`, recorded as a
    /// failure, and counted toward the default fail-fast threshold (no
    /// further lines run, even though there are more).
    #[tokio::test]
    async fn sequential_dispatch_panic_recovers_and_records_failure() {
        let _serial = PANIC_CALLSITE_LOCK.lock().await;
        let lines = make_lines(3);
        let (code, summary) = run_sequential(
            lines,
            opts(1, Some(1)), // default fail-fast
            panicking_dispatch(),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, EXIT_CODE_PANIC);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.ok, 0);
        assert_eq!(summary.skipped, 2); // lines 2, 3 not dispatched
    }

    /// `run_sequential` with `--continue-on-error`: the panicking line is
    /// recorded but the run continues, dispatching the remaining lines.
    /// Verifies that panic recovery participates in the same threshold
    /// machinery as other failures.
    #[tokio::test]
    async fn sequential_dispatch_panic_continues_with_continue_on_error() {
        let _serial = PANIC_CALLSITE_LOCK.lock().await;
        // First call panics, subsequent calls return 0 — but
        // `panicking_dispatch` panics on every call, so use a custom
        // dispatch that panics once then returns 0.
        let codes = Arc::new(tokio::sync::Mutex::new(vec![0i32, 0i32].into_iter()));
        let panic_first = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let dispatch: DispatchFn = {
            let codes = Arc::clone(&codes);
            let panic_first = Arc::clone(&panic_first);
            Arc::new(move |_cmd: Cmd| {
                let codes = Arc::clone(&codes);
                let panic_first = Arc::clone(&panic_first);
                Box::pin(async move {
                    if panic_first.swap(false, Ordering::SeqCst) {
                        panic!("synthetic panic for test");
                    }
                    codes.lock().await.next().expect("not enough fake codes")
                })
            })
        };

        let lines = make_lines(3);
        let (code, summary) = run_sequential(lines, opts(1, None), dispatch, no_interrupt()).await;
        assert_eq!(code, EXIT_CODE_PANIC);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.ok, 2); // lines 2 and 3 ran successfully
        assert_eq!(summary.skipped, 0);
    }

    /// Streaming sequential: panic recovery via the channel path.
    #[tokio::test]
    async fn sequential_streaming_dispatch_panic_recovers_and_records_failure() {
        let _serial = PANIC_CALLSITE_LOCK.lock().await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(1) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) =
            run_sequential_streaming(rx, opts(1, None), panicking_dispatch(), no_interrupt()).await;
        assert_eq!(code, EXIT_CODE_PANIC);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.ok, 0);
    }

    /// `run_parallel`: dispatch panic now flows through `execute_line`'s
    /// `catch_unwind` (not the join-loop fallback), surfacing as
    /// `EXIT_CODE_PANIC`. The recorded summary remains the same shape:
    /// one failed entry, no successes.
    #[tokio::test]
    async fn parallel_dispatch_panic_recovers_and_records_failure() {
        let _serial = PANIC_CALLSITE_LOCK.lock().await;
        let lines = make_lines(1);
        let (code, summary) = run_parallel(
            lines,
            opts(1, None), // continue-on-error semantics
            panicking_dispatch(),
            no_interrupt(),
        )
        .await;
        assert_eq!(code, EXIT_CODE_PANIC);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.ok, 0);
    }

    /// `run_parallel_streaming`: same shape, via the channel.
    #[tokio::test]
    async fn parallel_streaming_dispatch_panic_recovers_and_records_failure() {
        let _serial = PANIC_CALLSITE_LOCK.lock().await;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(1) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) =
            run_parallel_streaming(rx, opts(1, None), panicking_dispatch(), no_interrupt()).await;
        assert_eq!(code, EXIT_CODE_PANIC);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.ok, 0);
    }

    /// Panic counts toward `--max-errors`: with threshold 2, the second
    /// panic stops the run before any subsequent line dispatches. This is
    /// the load-bearing invariant — without panic recovery, panics either
    /// crashed the process (sequential) or recorded `failed` but did NOT
    /// trip the threshold (parallel join arm only updated summary, not
    /// fail_count).
    #[tokio::test]
    async fn sequential_panic_counts_toward_max_errors() {
        let _serial = PANIC_CALLSITE_LOCK.lock().await;
        let lines = make_lines(5);
        let (code, summary) = run_sequential(
            lines,
            opts(1, Some(2)),     // stop after 2 stop-worthy outcomes
            panicking_dispatch(), // every line panics
            no_interrupt(),
        )
        .await;
        assert_eq!(code, EXIT_CODE_PANIC);
        assert_eq!(summary.failed, 2);
        assert_eq!(summary.skipped, 3); // lines 3-5 never dispatched
    }

    /// `panic_message` extracts the message for a `&'static str` payload.
    #[test]
    fn panic_message_extracts_static_str() {
        let payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(panic_message(&*payload), "boom");
    }

    /// `panic_message` extracts the message for a `String` payload.
    #[test]
    fn panic_message_extracts_owned_string() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(String::from("kaboom"));
        assert_eq!(panic_message(&*payload), "kaboom");
    }

    /// `panic_message` falls back gracefully for non-string payloads
    /// (e.g. `panic_any(42i32)`).
    #[test]
    fn panic_message_handles_unknown_payload() {
        let payload: Box<dyn std::any::Any + Send> = Box::new(42i32);
        assert_eq!(panic_message(&*payload), "unknown panic payload");
    }

    // ---- interrupt-mid-spawn-loop in parallel executors ----
    //
    // When `interrupt` is already set when the spawn loop starts, the
    // loop immediately breaks before spawning anything. Sets `summary.skipped`
    // to `total` (parallel) or drains the channel (streaming-parallel).

    #[tokio::test]
    async fn parallel_interrupt_breaks_spawn_loop_immediately() {
        let lines = make_lines(3);
        let (code, summary) = run_parallel(
            lines,
            opts(2, Some(1)),
            fake_dispatch(vec![]),
            already_interrupted(),
        )
        .await;
        assert_eq!(code, 0);
        assert_eq!(summary.ok, 0);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 3);
    }

    #[tokio::test]
    async fn parallel_streaming_interrupt_breaks_spawn_loop_immediately() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for line in make_lines(3) {
            tx.send(line).unwrap();
        }
        drop(tx);
        let (code, summary) = run_parallel_streaming(
            rx,
            opts(2, Some(1)),
            fake_dispatch(vec![]),
            already_interrupted(),
        )
        .await;
        assert_eq!(code, 0);
        assert_eq!(summary.ok, 0);
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.skipped, 3);
    }

    /// Serializes every test that drives `execute_line`'s panic-recovery
    /// `error!`. Concurrent first-hits of that callsite from threads with
    /// no subscriber can win a lost-update race on tracing's per-callsite
    /// interest cache and pin it at `Interest::never`, which silently
    /// drops the event for the capture test below. tokio's Mutex so the
    /// guard may be held across the tests' await points.
    static PANIC_CALLSITE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// The panic-recovery `error!` in `execute_line` carries `command` and
    /// `panic` fields whose expressions only run when a subscriber has the
    /// event enabled — install one and pin both the exit code and the
    /// structured payload.
    #[tokio::test]
    async fn execute_line_panic_emits_error_event_with_fields() {
        use std::sync::Mutex;
        use tracing_subscriber::fmt::MakeWriter;

        let _serial = PANIC_CALLSITE_LOCK.lock().await;

        #[derive(Clone)]
        struct Buf(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Buf {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'a> MakeWriter<'a> for Buf {
            type Writer = Buf;
            fn make_writer(&'a self) -> Buf {
                self.clone()
            }
        }

        let sink = Buf(Arc::new(Mutex::new(Vec::new())));
        let subscriber = tracing_subscriber::fmt()
            .with_writer(sink.clone())
            .with_max_level(tracing::Level::ERROR)
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        // Repair any `Interest::never` a concurrent pre-lock first-hit may
        // have cached for the panic-recovery callsite before our
        // subscriber registered.
        tracing::callsite::rebuild_interest_cache();

        let line = make_lines(1).remove(0);
        let dispatch = panicking_dispatch();
        let (line_no, code) = execute_line(line, &dispatch).await;

        assert_eq!(line_no, 1);
        assert_eq!(code, EXIT_CODE_PANIC);
        let output = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(
            output.contains("subcommand panicked"),
            "expected the panic event; got: {output}"
        );
        assert!(
            output.contains("create-bucket"),
            "expected the command field; got: {output}"
        );
        assert!(
            output.contains("synthetic panic for test"),
            "expected the panic message field; got: {output}"
        );
    }
}
