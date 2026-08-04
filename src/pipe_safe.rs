// Vendored from s3util-rs@1.9.2
//   src/bin/s3util/pipe_safe.rs
// Adjustments: doc comment adapted to s7cmd — upstream ships one copy per
//              binary (s3sync / s3rm / s3ls carry a write_all-only variant);
//              s7cmd keeps a single shared module at the crate root so the
//              vendored util_bin printers keep their upstream
//              `crate::pipe_safe` import path and `main.rs` can reuse it for
//              completion scripts. Added the flush-error propagation unit
//              test from the s3rm-rs 1.5.2 copy.

//! Pipe-safe stdout printing.
//!
//! `println!` (and any direct stdout write) panics when stdout is a pipe
//! whose reader has already gone away — piping to a `head`/`grep -q`/pager
//! that exited before the output ended, or `s7cmd get-bucket-versioning
//! s3://bucket | head 1`, where `head` fails to open the file `1` and never
//! reads. Report output and pre-rendered output go through these helpers
//! instead: `BrokenPipe` is swallowed — by the time a report is printed the
//! S3 operation has already completed, and a reader that stops early is a
//! normal way for a pipeline to end — so the command still exits 0. Any
//! other write error (full disk or I/O error on a redirect) is propagated
//! so the command fails loudly instead of silently dropping output.
//!
//! Used by the vendored `util_bin` report printers (the `get-*` family,
//! `head-*`, `presign`, and the annotation subcommands) and by `main.rs`
//! for `--auto-complete-shell` scripts. The data-transfer paths are
//! intentionally different and unchanged: `cp` to stdout and
//! `get-object-annotation` payload output deliver object bytes whose
//! truncation is a real failure, and both already report `BrokenPipe` as
//! an error rather than panicking. The stderr counterpart for tracing
//! output is each vendored bin's `PipeSafeWriter`; `ls` listing output
//! already treats `BrokenPipe` as end-of-pipeline inside `ls_bin`.

use std::io::{ErrorKind, Write};

/// `println!("{text}")` minus the BrokenPipe panic: the line goes to
/// stdout, and a closed pipe is reported as success.
pub fn println_pipe_safe(text: &str) -> std::io::Result<()> {
    writeln_ignoring_broken_pipe(&mut std::io::stdout().lock(), text)
}

/// Write `bytes` to stdout as-is (no trailing newline), swallowing
/// BrokenPipe. Used for pre-rendered output such as shell-completion
/// scripts.
pub fn write_all_pipe_safe(bytes: &[u8]) -> std::io::Result<()> {
    write_all_ignoring_broken_pipe(&mut std::io::stdout().lock(), bytes)
}

fn writeln_ignoring_broken_pipe(writer: &mut impl Write, text: &str) -> std::io::Result<()> {
    ignore_broken_pipe(writeln!(writer, "{text}").and_then(|()| writer.flush()))
}

fn write_all_ignoring_broken_pipe(writer: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    ignore_broken_pipe(writer.write_all(bytes).and_then(|()| writer.flush()))
}

fn ignore_broken_pipe(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(e) if e.kind() == ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fails writes (or, with `fail_flush`, only the flush) with `kind`.
    /// The real closed-pipe scenario is exercised process-level by
    /// `tests/cli_broken_pipe.rs`.
    struct FailWriter {
        kind: ErrorKind,
        fail_flush: bool,
    }

    impl Write for FailWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.fail_flush {
                Ok(buf.len())
            } else {
                Err(std::io::Error::new(self.kind, "simulated write failure"))
            }
        }
        fn flush(&mut self) -> std::io::Result<()> {
            if self.fail_flush {
                Err(std::io::Error::new(self.kind, "simulated flush failure"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn writeln_appends_newline_and_passes_through() {
        let mut buf = Vec::new();
        writeln_ignoring_broken_pipe(&mut buf, "{\n  \"Status\": \"Enabled\"\n}").unwrap();
        assert_eq!(buf, b"{\n  \"Status\": \"Enabled\"\n}\n");
    }

    #[test]
    fn writeln_swallows_broken_pipe_on_write() {
        let mut writer = FailWriter {
            kind: ErrorKind::BrokenPipe,
            fail_flush: false,
        };
        writeln_ignoring_broken_pipe(&mut writer, "line").unwrap();
    }

    #[test]
    fn writeln_swallows_broken_pipe_on_flush() {
        let mut writer = FailWriter {
            kind: ErrorKind::BrokenPipe,
            fail_flush: true,
        };
        writeln_ignoring_broken_pipe(&mut writer, "line").unwrap();
    }

    #[test]
    fn writeln_propagates_other_errors() {
        // A failed redirect (disk full, I/O error) must still fail the
        // command — only a vanished reader is benign.
        let mut writer = FailWriter {
            kind: ErrorKind::StorageFull,
            fail_flush: false,
        };
        let err = writeln_ignoring_broken_pipe(&mut writer, "line").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::StorageFull);
    }

    #[test]
    fn write_all_passes_bytes_through_verbatim() {
        let mut buf = Vec::new();
        write_all_ignoring_broken_pipe(&mut buf, b"complete -c s7cmd\n").unwrap();
        assert_eq!(buf, b"complete -c s7cmd\n");
    }

    #[test]
    fn write_all_swallows_broken_pipe_on_write() {
        let mut writer = FailWriter {
            kind: ErrorKind::BrokenPipe,
            fail_flush: false,
        };
        write_all_ignoring_broken_pipe(&mut writer, b"data").unwrap();
    }

    #[test]
    fn write_all_swallows_broken_pipe_on_flush() {
        let mut writer = FailWriter {
            kind: ErrorKind::BrokenPipe,
            fail_flush: true,
        };
        write_all_ignoring_broken_pipe(&mut writer, b"data").unwrap();
    }

    #[test]
    fn write_all_propagates_other_errors() {
        let mut writer = FailWriter {
            kind: ErrorKind::StorageFull,
            fail_flush: false,
        };
        let err = write_all_ignoring_broken_pipe(&mut writer, b"data").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::StorageFull);
    }

    #[test]
    fn write_all_propagates_other_errors_on_flush() {
        let mut writer = FailWriter {
            kind: ErrorKind::StorageFull,
            fail_flush: true,
        };
        let err = write_all_ignoring_broken_pipe(&mut writer, b"data").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::StorageFull);
    }
}
