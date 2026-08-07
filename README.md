# s7cmd

[![Crates.io](https://img.shields.io/crates/v/s7cmd.svg)](https://crates.io/crates/s7cmd)
[![GitHub](https://img.shields.io/github/downloads/nidor1998/s7cmd/total?label=downloads%20%28GitHub%29)](https://github.com/nidor1998/s7cmd/releases)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
![CI](https://github.com/nidor1998/s7cmd/actions/workflows/ci.yml/badge.svg?branch=main)
[![codecov](https://codecov.io/gh/nidor1998/s7cmd/graph/badge.svg?token=XFICDPTMDG)](https://codecov.io/gh/nidor1998/s7cmd)

A reliable, flexible, and fast command-line tool for Amazon S3.

s7cmd combines the speed of a Rust async runtime with the breadth of
the AWS S3 API surface, providing high-throughput object operations
(`ls`, `cp`, `mv`, `rename`, `rm`, `sync`, `clean`) alongside comprehensive
bucket administration (lifecycle, policy, encryption, object annotations, CORS, public
access block, website, logging, notification, and more) — all from a
single static binary.

s7cmd is a thin command-line wrapper over four Rust libraries by the
same author: [s3sync](https://github.com/nidor1998/s3sync),
[s3ls-rs](https://github.com/nidor1998/s3ls-rs),
[s3util-rs](https://github.com/nidor1998/s3util-rs), and
[s3rm-rs](https://github.com/nidor1998/s3rm-rs). Originally, only
s3sync and s3rm-rs existed and were not intended to be merged, but
in response to user requests for a unified interface, the
functionality was split into focused libraries and bundled together
into a single binary as s7cmd. Built on `aws-sdk-rust` and `tokio`,
s7cmd targets workloads that demand both performance and operational
completeness: data engineering pipelines, ML training data
preparation, multi-account bucket governance, and integrity-critical
migrations.

## Features highlights

s7cmd is designed to pursue reliability and performance as equal goals,
rather than trading one for the other.

- **Single binary, full coverage.** Object transfer, bulk delete, and
  every common bucket-level configuration in one tool.
- **Strong integrity verification.** Native support for SHA256, SHA1,
  CRC32, CRC32C, and CRC64NVME — aligned with S3's 2026 default
  checksum policy.
- **Predictable performance.** Configurable workers, multipart
  thresholds, and chunk sizes; bounded memory footprint suitable for
  small instances and large CI runners alike.
- **Apache-2.0 licensed.** No copyleft concerns for enterprise
  deployment or container distribution.

### About the name

The name follows the `s3cmd` / `s4cmd` / `s5cmd` / `s6cmd` lineage,
but s7cmd is not affiliated with, derived from, or compatible with
any of them. The number 7 was chosen simply because it was the
next available one. There is no deeper meaning.

## Usage

```
Usage: s7cmd [OPTIONS] [COMMAND]

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
      --auto-complete-shell <SHELL>  Generate shell completions for s7cmd (all subcommands) and exit [possible values: bash, elvish, fish, powershell, zsh]
  -h, --help                         Print help (see more with '--help')
  -V, --version                      Print version
```

### `batch-run`

Reads s7cmd commands from a file (or stdin via `-`), one per line,
and executes them in the same process — avoiding the per-command
fork/exec, dynamic linker work, and Rust/tokio runtime startup you
would pay if you invoked `s7cmd` once per line from a shell loop.
(AWS SDK clients and TLS connections are still built per dispatched
command, so network-side overhead is not eliminated; the win is
process startup.) It is the recommended way to drive thousands of
small operations (per-object tagging, mixed bucket-config edits,
etc.) without spawning a process per command.

> Note that although batch-run avoids launching a separate process
> for each command, it still initializes a new AWS client per
> command. This incurs per-command overhead such as credential
> resolution, region resolution, and HTTP client setup, so batch-run
> is not intended for high-throughput parallel processing of large
> workloads. 
> In my EC2 environment, even after disabling IMDS, copying small files was limited to about 1,000 copies per second (with --parallel 32).
> Using the `sync` subcommand allows you to copy more than 3,000 files per second under similar conditions (though you'll need to adjust the `--worker-size` parameter), so there is a trade-off between flexibility and speed.

```text
Usage: s7cmd batch-run [OPTIONS] <FILE>
```

`<FILE>` is a path to a script file, or `-` to read from stdin
(mirrors `put-bucket-policy`).

**Input format.** One command per line. Each line is tokenized with
shell-style quoting (POSIX `shlex`), so quoted arguments with
spaces work as expected. Blank lines are ignored. Lines whose first
non-whitespace character is `#` are treated as comments and skipped.
Each line is parsed as if it were a top-level s7cmd invocation
*without* the leading `s7cmd` — i.e. start with the subcommand name:

```text
# create two buckets, then tag one of them
create-bucket s3://example-bucket-1
create-bucket s3://example-bucket-2
put-bucket-tagging --tagging "team=data&env=prod" s3://example-bucket-1

# upload a file with a key that contains spaces
cp ./report.csv "s3://example-bucket-1/reports/Q1 2026.csv"
```

Pass the script as a file argument, or pipe it in via `-`:

```sh
s7cmd batch-run commands.txt
s7cmd batch-run - < commands.txt
```

**Execution modes.** Two read modes and two parallelism modes,
freely combined:

| Flag | Effect |
|------|--------|
| (default) | Read the whole script first, validate every line, then execute. Catches bad lines before any line runs. Shows a progress bar when stderr is a TTY. |
| `--streaming` | Execute commands as they are read. No progress bar. Use for unbounded or pipelined input where buffering the whole script is undesirable. |
| `--parallel 1` (default) | Sequential execution. Lines run in script order. |
| `--parallel N` | Run up to *N* commands concurrently (max 1024; a larger value is rejected at parse time). Completion order is not guaranteed. |
| `--parallel 0` | Use all logical CPUs. Completion order is not guaranteed. |

Script order is preserved only with `--parallel 1`. With
`--parallel N` (or `--parallel 0`), commands may complete in any
order; do not rely on later lines observing the effects of earlier
ones.

Default mode loads the entire script into memory, so very large
scripts will use proportional memory. Use `--streaming` to execute
lines as they are read.

**Failure handling.** By default, the first failing command stops
sequential execution and prevents new spawns in parallel
mode. Pass `--continue-on-error` to run every line regardless, or
`--max-errors N` (`N` ≥ 1) to keep running up to `N` failures and
then stop gracefully (sequential: stops after the N-th failure;
parallel: stops spawning new commands once N failures have been
recorded — in-flight commands complete). Pass
`--continue-on-warning` to keep running past per-line warnings
(exit codes 3 and 4 — `EXIT_CODE_WARNING` and
`EXIT_CODE_NOT_FOUND`) while still stopping on true failures
according to `--max-errors` (or the default first-failure stop).
`--continue-on-error` is mutually exclusive with both
`--max-errors` and `--continue-on-warning`. The process exit code
is the worst seen across all executed commands, ranked by severity
rather than numeric value: `1` (error) > `2` (invalid args) > `3`
(warning) > `4` (not found) > any other non-zero (e.g. `130`
SIGINT) > `0`. So a run mixing exit `1` and exit `130` exits `1`,
not `130`.

Lines that can't be parsed or validated (quoting errors, unknown
subcommands, missing or invalid arguments, empty commands) count as
failures the same way runtime failures do — they synthesize exit
code `2`, log at error level, increment the `failed` bucket, and
count toward `--max-errors` / `--continue-on-error`. So
`--max-errors 5` will let you tolerate up to 5 typo'd lines anywhere
in the script. (True read I/O errors — line over the 16 KiB cap,
non-UTF-8 bytes, file unreadable — still abort the whole run.)

**Format check.** Pass `--check-format` to validate the script
without executing anything. The walk stops at the first
parse or validation problem (or read I/O error), reports
that line as a single error-level log entry — identifying the
script source (file path, or `stdin` for `-`) and the line
number — and exits 1. On a clean pass a `"format OK"` message
is emitted.

**Per-line tracing.** Each dispatched line emits a `start` event
and a matching outcome event (`success`, `warning (exit N)`,
`skipped (exit 130)`, or `failure (exit N)`) prefixed with the
line number and the original input text. `start` and `success`
are info level (silent at the default warn level — pass `-v` to
see them); `warning` and `skipped` are warn level and `failure`
is error level, all three visible without `-v`. If a line carries
an inline credential option (`--*-access-key`,
`--*-secret-access-key`, `--*-session-token`, `--*-sse-c-key`,
`--*-sse-c-key-md5`), its value is masked to `****` in the logged
text.

**Tracing flags belong to `batch-run`, not per-line.** Pass
`--json-tracing`, `--aws-sdk-tracing`, `--span-events-tracing`,
`--disable-color-tracing`, and `-v`/`-q` to `batch-run` itself —
e.g. `s7cmd batch-run --aws-sdk-tracing commands.txt`. Lines that
set `--json-tracing`, `--aws-sdk-tracing`, `--span-events-tracing`,
or `--disable-color-tracing` are rejected at validation time;
per-line `-v`/`-q` is silently ignored (the tracing subscriber is
installed once, at the top of the run).

**Caveats and safety.**

- If you're not concerned with performance, it's best to leave
  `--parallel` at its default setting and run the process in series.
  There are many factors to consider when parallelizing.
- Even when you increase the parallelism level (`--parallel`), the
  various rate limits apply on a per-command basis (they are not
  divided across or aggregated over the workers).
- Increasing `--parallel` may increase the load on the operating
  system. It consumes CPU, memory, file descriptors, and other
  resources — pick a value the host and the target service can
  absorb.
- On EC2 instances using an IAM instance profile, setting
  `--parallel` too high is likely to trigger IMDS-related errors
  (credential resolution hits the instance metadata service per
  command, and IMDS will throttle under heavy concurrent load).
- The failure threshold in parallel mode is "stop spawning new
  commands", not "cancel in-flight commands." When `--parallel N`
  is close to or exceeds the number of script lines, every line may
  already be in flight by the time the threshold trips, so the run
  completes as if no threshold were set. The threshold is most
  effective when the line count is significantly larger than `N`. To
  cancel work that is already in flight, send SIGINT (Ctrl-C); per-
  subcommand cancellation handlers propagate it into in-flight
  transfers.
- `batch-run` is a dangerous command and must be used with caution.
  Whenever possible, perform a dry run by using each subcommand's
  `--dry-run` flag, and pass `-v` to `batch-run` itself to surface
  the per-line info-level logs for preliminary verification.

For example, suppose you want to create two buckets and tag one of
them. First, prepare a dry-run script (`sample_dry_run.txt`) with
each subcommand's `--dry-run` flag baked in:

```text
# sample_dry_run.txt — preview only; nothing is sent to S3 except per-subcommand --dry-run client-side validation.
create-bucket --dry-run s3://example-bucket-1
create-bucket --dry-run s3://example-bucket-2
put-bucket-tagging --dry-run --tagging "team=data&env=prod" s3://example-bucket-1
```

Run it with `-v` on `batch-run` itself so the per-line `start` /
`success` events and the per-subcommand `[dry-run] would …` info
lines are visible. `--no-progress` is added so the live progress
bar (drawn by default on TTY stderr) does not interleave with
the log lines you want to read:

```console
$ s7cmd batch-run -v --no-progress sample_dry_run.txt
2026-04-30T23:34:11.178191Z  INFO line started line=2 event="start" command="create-bucket" raw="create-bucket --dry-run s3://example-bucket-1"
2026-04-30T23:34:11.282653Z  INFO [dry-run] would create bucket. bucket=example-bucket-1
2026-04-30T23:34:11.282756Z  INFO line completed line=2 event="success" exit_code=0 command="create-bucket" raw="create-bucket --dry-run s3://example-bucket-1"
2026-04-30T23:34:11.282762Z  INFO line started line=3 event="start" command="create-bucket" raw="create-bucket --dry-run s3://example-bucket-2"
2026-04-30T23:34:11.283018Z  INFO [dry-run] would create bucket. bucket=example-bucket-2
2026-04-30T23:34:11.283038Z  INFO line completed line=3 event="success" exit_code=0 command="create-bucket" raw="create-bucket --dry-run s3://example-bucket-2"
2026-04-30T23:34:11.283040Z  INFO line started line=4 event="start" command="put-bucket-tagging" raw="put-bucket-tagging --dry-run --tagging \"team=data&env=prod\" s3://example-bucket-1"
2026-04-30T23:34:11.283239Z  INFO [dry-run] would put bucket tagging. bucket=example-bucket-1
2026-04-30T23:34:11.283284Z  INFO line completed line=4 event="success" exit_code=0 command="put-bucket-tagging" raw="put-bucket-tagging --dry-run --tagging \"team=data&env=prod\" s3://example-bucket-1"
batch-run: 3 succeeded, 0 failed, 0 warnings, 0 skipped, elapsed 0.1s
```

Once the dry run looks correct, run the same commands without
`--dry-run` (`sample.txt`):

```text
# sample.txt — the real run; this DOES create buckets and apply tags.
create-bucket s3://example-bucket-1
create-bucket s3://example-bucket-2
put-bucket-tagging --tagging "team=data&env=prod" s3://example-bucket-1
```

```console
$ s7cmd batch-run -v --no-progress sample.txt
2026-04-30T23:35:42.418901Z  INFO line started line=2 event="start" command="create-bucket" raw="create-bucket s3://example-bucket-1"
2026-04-30T23:35:43.512214Z  INFO Bucket created. bucket=example-bucket-1
2026-04-30T23:35:43.512410Z  INFO line completed line=2 event="success" exit_code=0 command="create-bucket" raw="create-bucket s3://example-bucket-1"
2026-04-30T23:35:43.512430Z  INFO line started line=3 event="start" command="create-bucket" raw="create-bucket s3://example-bucket-2"
2026-04-30T23:35:44.601877Z  INFO Bucket created. bucket=example-bucket-2
2026-04-30T23:35:44.602008Z  INFO line completed line=3 event="success" exit_code=0 command="create-bucket" raw="create-bucket s3://example-bucket-2"
2026-04-30T23:35:44.602020Z  INFO line started line=4 event="start" command="put-bucket-tagging" raw="put-bucket-tagging --tagging \"team=data&env=prod\" s3://example-bucket-1"
2026-04-30T23:35:44.881342Z  INFO Bucket tagging set. bucket=example-bucket-1
2026-04-30T23:35:44.881455Z  INFO line completed line=4 event="success" exit_code=0 command="put-bucket-tagging" raw="put-bucket-tagging --tagging \"team=data&env=prod\" s3://example-bucket-1"
batch-run: 3 succeeded, 0 failed, 0 warnings, 0 skipped, elapsed 2.5s
```

The per-subcommand `[dry-run] would …` info lines are replaced by
their concrete counterparts (`Bucket created.`,
`Bucket tagging set.`); everything else — the `start` / `success`
events, the trailing summary — is the same shape.

Without `-v`, both the per-line `start` / `success` events and
the per-subcommand info lines (`[dry-run] would …`,
`Bucket created.`, etc.) are suppressed at the default warn
level — only warnings and errors are logged, plus the trailing
summary line on stderr. That is why the safety guidance pairs
`--dry-run` with `-v`: you need info-level output to see what
*would* happen.

**Restrictions.**

- Nested `batch-run` is rejected.
- `cp`/`mv` lines may not use `-` (stdin/stdout) as source or target.
- Per-line input is capped at 16 KiB.

**Summary.** When the run completes (or aborts), an
`N succeeded, N failed, N warnings, N skipped, elapsed Ts` line is
written to stderr. Per-line outcomes bucket as: exit `0` →
`succeeded`; exit `3` or `4` (`EXIT_CODE_WARNING`,
`EXIT_CODE_NOT_FOUND`) → `warnings`; exit `130` (the conventional
Unix code for SIGINT — returned by per-subcommand cancellation
handlers when the user hits Ctrl-C) → `skipped` (logged at warn
level, not error, and never counted toward `--max-errors`); any
other non-zero exit → `failed`; lines that were never dispatched
(fail-fast or `--max-errors` threshold tripped, or SIGINT) →
`skipped`.
Suppress the line with `--no-summary`. With `--json-tracing` the
same information is emitted as a single-line JSON object instead,
e.g. `{"summary":"batch-run","succeeded":48,"failed":1,"warnings":2,"skipped":1,"elapsed_seconds":3.4}`.

**Progress bar.** In read-all mode, when stderr is a TTY, a live
progress bar is drawn on stderr while the run is in progress.
Suppress it with `--no-progress` — useful when stderr is a TTY
but you want machine-readable log output (terminal multiplexers,
`script(1)`, some CI runners). Streaming mode and non-TTY stderr
already suppress the bar. `--json-tracing` also suppresses it
automatically (the bar would interleave with JSON output).
`--no-progress` and `--no-summary` are independent — each controls
only its own visual element; pass both for fully clean output.

## Proxy support

s7cmd respects the standard proxy environment variables
(`HTTP_PROXY`, `HTTPS_PROXY`) automatically.
No flags are required — set the variables in your shell and every
subcommand routes its S3 traffic through the proxy.

Proxy authentication is supported via the URL form
`http(s)://user:password@proxy:port`.

## Documentation

Each subcommand is documented in the README of its underlying
library. For details on flags, semantics, and exit codes, refer to:

| Subcommand                         | Documentation                                          |
| ---------------------------------- | ------------------------------------------------------ |
| `ls`                               | [s3ls-rs](https://github.com/nidor1998/s3ls-rs)        |
| `sync`                             | [s3sync](https://github.com/nidor1998/s3sync)          |
| `clean`                            | [s3rm-rs](https://github.com/nidor1998/s3rm-rs)        |
| `cp`, `mv`, `rename`, `rm`, and all others | [s3util-rs](https://github.com/nidor1998/s3util-rs)    |
| `batch-run`                        | s7cmd-only — see the section above                     |

Each of these projects (except `batch-run`) also ships its own
standalone binary, which can be used independently of s7cmd.

**Exit code on interruption (v1.8.0+).** Every subcommand that catches
Ctrl-C (SIGINT) for graceful shutdown — `sync`, `ls`, `clean`, `cp`,
and `mv` — exits with code `130` (128 + SIGINT, the conventional shell
encoding for termination by signal) when interrupted, and the
interruption takes precedence over any errors or warnings the aborted
run had recorded. This is one place s7cmd can differ from the upstream
documentation linked above: the vendored frontends carry this fix
ahead of the pinned s3rm-rs / s3ls-rs versions and apply the same
behavior to `sync`, whose standalone upstream binary currently exits
`0` on interruption. Subcommands that finish in a single API call
install no handler, so Ctrl-C terminates them through the default
signal disposition — which shells also report as `130`. At `clean`'s
confirmation prompt, Ctrl-C likewise terminates immediately via the
default handler. Inside `batch-run`, an interrupted line is bucketed
`skipped (exit 130)` and the batch exit code follows the severity
ranking described in the `batch-run` section above.

## Security assumptions

s7cmd is built on a fundamental security assumption: **both the object storage system and the specific bucket you
operate on must be trusted.**

Within this trust model, s7cmd implements the security measures you would reasonably expect of an S3 command-line
tool: encrypted transport (TLS/HTTPS) for data in transit, end-to-end integrity verification for transfers (ETag,
MD5, SHA256, and CRC checksums), support for server-side encryption, secure handling of credentials through the
standard AWS credential providers (with credential environment-variable values hidden from `--help` output), and —
for `ls` — control-character escaping of the object keys, prefixes, and owner names returned by S3.
These measures protect the confidentiality and integrity of your data against transport-level and accidental
threats.

However, s7cmd assumes that the storage endpoint is honest and non-adversarial — that it correctly implements the
S3 API and returns the data, listings, metadata, and checksum values it actually stores, without tampering. Because
subcommands decide what to transfer, list, or delete from the listings and metadata the endpoint reports, the
integrity-verification and filtering features are **not** a defense against a malicious or compromised storage
backend that deliberately returns falsified data, fabricated listings, or forged checksums. Against such an
adversarial endpoint, these guarantees do not hold.

Crucially, trust must extend to the **bucket**, not just the storage provider. Even when the object storage system
itself is fully trustworthy, a bucket can still be adversarial — for example, a bucket you do not control, a shared
bucket writable by others, or one whose objects, metadata, or checksums were crafted by an attacker. If you
synchronize from, copy from, list, or delete from such a bucket, the data and metadata it serves are already
untrusted at the source, and s7cmd's guarantees no longer apply. A trusted storage provider hosting an untrusted
bucket is, for the purposes of this security model, an untrusted source.

Operating on an untrusted, compromised, or non-conformant endpoint or bucket is outside s7cmd's security model.
Selecting a trustworthy storage provider, and ensuring that every bucket you operate on is one you control or
trust — including its credentials, encryption, and access policies — remains your responsibility.

This mirrors the "Security assumptions" sections of the underlying
[s3sync](https://github.com/nidor1998/s3sync) / [s3util-rs](https://github.com/nidor1998/s3util-rs) /
[s3rm-rs](https://github.com/nidor1998/s3rm-rs) / [s3ls-rs](https://github.com/nidor1998/s3ls-rs) projects.

## Requirements

- x86_64 Linux (kernel 3.2 or later)
- ARM64 Linux (kernel 4.1 or later)
- Windows 11 (x86_64, aarch64)
- macOS 11.0 or later (aarch64)

All features are tested on the above platforms.

## Installation

Download the latest binary from [GitHub Releases](https://github.com/nidor1998/s7cmd/releases)

You should build ARM64 Windows binaries yourself.

## Fully AI-generated, always human-verified

Every line of s7cmd's own source code (including the vendored adaptations from upstream), every test, all documentation, CI/CD configuration, and this README were generated by AI using [Claude Code](https://docs.anthropic.com/en/docs/claude-code/overview) (Anthropic). The same applies to three of the four underlying libraries: [s3util-rs](https://github.com/nidor1998/s3util-rs), [s3ls-rs](https://github.com/nidor1998/s3ls-rs), and [s3rm-rs](https://github.com/nidor1998/s3rm-rs). The fourth, [s3sync](https://github.com/nidor1998/s3sync), is human-written and serves as the reference architecture from which the AI-generated siblings were derived.

Human verification is a permanent policy, not a one-time event applied only to the initial build. Human engineers authored the requirements, design specifications, and s3sync reference architecture, and continue to review and verify every change to the design, source code, and tests. Every release is manually tested by humans before it ships, and all E2E test scenarios are verified against live AWS S3. No AI-generated change is released without human review and testing — this applies equally to the initial build and to all future updates, including dependency bumps, bug fixes, and new features. The development follows a spec-driven process: requirements and design documents are written first, and the AI generates code to match those specifications under continuous human oversight.

Every underlying library maintains 96%+ automated test coverage. This serves a dual purpose: it verifies that AI-generated code meets its specifications, and it ensures the project remains maintainable by hand — whether because AI tooling becomes unavailable, or because a future maintainer prefers to work without AI assistance. Combined with the modular library design and Apache-2.0 licensing, this means s7cmd can be safely forked and maintained without AI assistance if the need arises.

Discussions about the legitimacy, licensing, or ethics of AI-generated code in general are out of scope for this issue tracker. Issues opened on those grounds — without a concrete, reproducible defect in s7cmd's behavior — will be closed.

### Quality verification (by AI self-assessment)

Measurements below are taken at v1.6.0 (commit `c454694` on `main`, tagged 2026-07-20; measured 2026-07-21). The coverage figures are sourced from `llvm-cov-report.txt` (`cargo llvm-cov`; `lcov.info` is the matching machine-readable LCOV artifact) and reflect a single combined run — `cargo llvm-cov` with `RUSTFLAGS="--cfg e2e_test"` on the maintainer's machine, 2026-07-20 — so the unit tests, the process-level CLI and batch-run tests, and the live-AWS e2e suite are all included in the report.

| Metric                         | Value                                                         |
|--------------------------------|---------------------------------------------------------------|
| Production code                | ~16,900 lines of Rust across 84 source files in `src/`        |
| Unit tests (in `src/`)         | 477 `#[test]` / `#[tokio::test]` annotations                  |
| CLI integration tests          | 692 annotations across 63 files (62 `tests/cli_*.rs` files plus `tests/batch_run.rs`); they spawn the real binary with no AWS credentials — S3 interactions, where exercised, hit an in-process loopback mock server; run in CI |
| E2E integration tests          | 258 annotations across 28 `tests/e2e_*.rs` files (gated behind `--cfg e2e_test`; run only by the maintainer against live AWS) |
| Code coverage (llvm-cov, combined unit + CLI + e2e run) | 97.63% regions (347 / 14,629 missed), 96.94% functions (35 / 1,145 missed), 98.55% lines (151 / 10,448 missed) |
| Static analysis (clippy)       | 0 warnings (`cargo clippy --all-features`)                    |
| Formatting                     | 0 diffs (`cargo fmt --all --check`)                           |
| Supply chain (cargo-deny)      | Clean (`cargo deny -L error check`); runs on every push and PR in `ci.yml` and daily at 01:34 UTC in `cargo-deny.yml`; `advisories.ignore = []` |
| Code adapted from the underlying projects | CLI frontends vendored from the upstream binaries — `src/sync_bin/` ([s3sync](https://github.com/nidor1998/s3sync)), `src/util_bin/` ([s3util-rs](https://github.com/nidor1998/s3util-rs)), `src/clean_bin/` ([s3rm-rs](https://github.com/nidor1998/s3rm-rs)), `src/ls_bin/` ([s3ls-rs](https://github.com/nidor1998/s3ls-rs)) — plus adapted dispatch arms in `src/dispatch.rs`; the engines themselves are consumed as exact-pinned library dependencies (s3sync 1.60.0, s3util-rs 1.8.0, s3rm-rs 1.4.0, s3ls-rs 1.1.0). `src/batch_run/` is s7cmd-original |

What these numbers do and do not show:
- They show what the combined test run exercises — including the live-AWS e2e suite — not how the binary behaves under production load over time. CI asserts only the non-e2e build (unit, CLI, and batch-run tests) on every push and PR, across six build targets; it neither runs the e2e suite nor produces or gates on coverage.
- Coverage measures s7cmd's own `src/` — the wrapper layer: the clap argument surfaces, subcommand dispatch, the vendored CLI frontends, and batch-run. The transfer, listing, and bulk-delete engines are separate crates measured in their own repositories (each underlying library maintains 96%+ coverage, as noted above); their figures are not part of the numbers in this table.
- Coverage is a structural metric. A covered line can still be incorrect; an uncovered line can still be correct. Use it to size the test surface, not to certify behaviour.
- The e2e suite covers live-AWS paths (object, bucket-configuration, and stdio round-trips, object annotations, batch-run, dry-run, ctrl-c cancellation, exit codes) but runs only on the maintainer's machine, and reproducing the coverage figures above requires AWS credentials.

The codebase is built through spec-driven development with human review at every step. Test counts and coverage will change as dependencies are updated and refinements land.

### AI assessment of safety and correctness (by Claude, Anthropic)

<details>
<summary>Click to expand the full assessment</summary>

> Assessment date: 2026-07-22.
>
> Assessed version: 1.6.0 (branch `main`, commit `c454694`, tag `v1.6.0`).
>
> Method and scope of evidence: this assessment was performed from scratch at v1.6.0, deriving every claim from the source rather than carrying conclusions forward from this README or any earlier assessment. All 84 Rust source files under `src/` (16,949 lines) were read in full: the s7cmd-original `src/batch_run/` engine, `src/dispatch.rs`, `src/cli.rs` and `src/main.rs` directly line by line, and the four vendored CLI frontends (`src/util_bin/`, 59 files; `src/sync_bin/`, `src/clean_bin/`, `src/ls_bin/`, 14 files) in full parallel review passes whose safety-relevant claims were re-verified against the cited lines. Also read: the 63 offline process-level test files (`tests/cli_*.rs` plus `tests/batch_run.rs`), the 28 live-AWS suites (`tests/e2e_*.rs`), `tests/common/mod.rs`, `Cargo.toml` and `Cargo.lock`, `deny.toml`, all five GitHub Actions workflows, `Dockerfile`, `build.rs`, `.cargo/config.toml`, and the coverage artifacts `llvm-cov-report.txt` / `lcov.info` (verified internally consistent: identical line and function totals). Where a guarantee lives in an engine crate, the pinned published sources (s3sync 1.60.0, s3util-rs 1.8.0, s3rm-rs 1.4.0, s3ls-rs 1.1.0) were consulted directly. The static checks were re-run during this assessment and confirmed: `cargo fmt --all --check` clean; `cargo clippy --all-features --all-targets` zero warnings, also under `RUSTFLAGS="--cfg e2e_test"` (so the e2e corpus type-checks); `cargo deny -L error check` fully clean against the advisory database of the assessment date.
>
> Limits of the evidence: this assessment cannot rule out all bugs. It includes no fuzzing, sanitizer or Miri runs, formal verification, or penetration testing. The e2e suite runs only against the maintainer's AWS account; CI neither compiles nor runs it. Coverage measures what the tests execute, not whether the executed code is correct. The transfer, listing and bulk-delete engines are separate crates assessed in their own repositories; they are covered here only as s7cmd consumes them.

**Question addressed.** s7cmd is a wrapper: 55 of its 56 subcommands hand off to four exact-pinned engine libraries, and the one s7cmd-original engine is `batch-run`. Three failure modes drive the analysis: (1) the wrapper mis-routes — a subcommand reaches the wrong engine, mistranslates its configuration, or weakens a guarantee (dry-run, exit codes, credential hygiene) that the standalone upstream binary provides; (2) `batch-run` mis-executes a script — runs the wrong line, loses a failure, kills the whole batch, or copies an inline credential into logs; (3) an operator mistake in one line of a batch mutates the wrong resource with no preview. Each is examined in turn.

#### Architecture and subcommand surface (v1.6.0)

The `Cmd` enum has 56 variants (`src/cli.rs:270-426`): 21 read-only subcommands, 34 mutating ones, and `batch-run`. Every arm of `dispatch` returns an `i32` instead of exiting (`src/dispatch.rs:23`), and `main` delivers it via `ExitCode::from` (`src/main.rs:44-45`) — there is no `std::process::exit` anywhere under `src/`; the vendoring deliberately removed upstream's process-exiting paths (`load_config_exit_if_err` for `ls`/`clean`, `validate_state_flag` for the three state-flag `put-*` commands, re-implemented as non-exiting checks called from `dispatch.rs:278,401,418`) so a bad line cannot kill a batch. Config-translation failures for `sync`/`ls`/`clean`/`cp`/`mv` print a clap-formatted error and return exit 2 (`dispatch.rs:28-140`). Large subcommand futures are `Box::pin`-ed to keep dispatch's frame below CI's 2 MB test-thread stacks (`dispatch.rs:16-22`). Routing is pinned three ways: in-process parse tests over every subcommand string (`tests/cli_routing.rs`, 62 tests), 63 dispatch unit tests in `src/dispatch.rs`, and live end-to-end dispatch tests per family.

#### `batch-run` (the s7cmd-original engine)

`batch-run` is where s7cmd adds real machinery, and it is defensively built:

- Input handling: lines are read with an incrementally enforced 16 KiB cap — a multi-GB line is rejected after one buffer, not buffered (`src/batch_run/parser.rs:101-135`) — validated as UTF-8, and tokenized with POSIX `shlex`. Blank/comment lines are skipped; a tokenization error becomes a per-line failure, not an abort.
- Per-line validation before anything runs: nested `batch-run`, stdin/stdout operands (`cp`/`mv` `-`, `put-bucket-* -`, `--annotation-payload -`), and per-line tracing flags are rejected — the tracing check inspects the parsed clap fields of all ~50 variants, not raw strings, so every accepted spelling is caught (`src/batch_run/validate.rs`).
- Invalid lines (tokenize / parse / empty / validate) synthesize exit 2, log at error level with structured fields, and count toward `--max-errors` / `--continue-on-error` exactly like runtime failures (`src/batch_run/executor.rs:74-92`) — a regression test pins that they no longer abort the whole run (`tests/batch_run.rs:194-220`).
- Panic containment: every dispatched line runs under `catch_unwind`; a panicking subcommand becomes exit 101, is logged with the line number and redacted text, and counts toward the failure threshold (`executor.rs:125-167`). This depends on `panic = "unwind"` in **all** build profiles (`Cargo.toml:103-113`) — a deliberate 1.2.3 change (release-min-size had been `panic = "abort"`, which would have defeated the recovery) and a standing invariant future profile edits must not break.
- Exit-code aggregation is severity-ranked, not numeric: 1 > 2 > 3 > 4 > any other non-zero > 0 (`executor.rs:294-303`), so a run mixing exit 1 and exit 130 exits 1. Per-line exit 130 (SIGINT) buckets as `skipped` and never trips `--max-errors` (`executor.rs:281-287`).
- SIGINT semantics are phased: during read-all's read/validate phases the default handler applies (Ctrl-C kills immediately, nothing has run); the listener is installed only before execution, after which Ctrl-C stops new spawns while in-flight commands cancel via their own handlers (`src/batch_run/mod.rs:253-331`). A live-AWS test SIGINTs a 200 MB upload mid-batch and asserts exit 130 with the remaining lines skipped (`tests/e2e_batch_run.rs:553-645`).
- `--parallel` is capped at 1024 by a custom clap parser, rejecting at parse time (exit 2) values that previously panicked `tokio::sync::Semaphore::new` (exit 101) — with the exact panic-boundary value pinned in tests (`src/cli.rs:148-170`, `tests/cli_arg_validation.rs:717-728`). Parallel mode drives the non-`Send` dispatch futures concurrently on a `LocalSet` with a semaphore bound; the engines' own worker pools provide the actual multi-core parallelism.
- Line parsing goes through the same post-processed clap tree as the top-level binary (`mod.rs:34-37`), which disarms the upstream-inherited `--auto-complete-shell` flag including its `AUTO_COMPLETE_SHELL` env binding — closing a fixed 1.6.0 bug where the exported variable silently rewrote argument handling (required args lifted, sync/clean paths defaulted) and a per-line `cp --auto-complete-shell` panicked.

#### Wrapper fidelity and operator-mistake protection

- `--dry-run` exists on all 34 mutating subcommands and none of the 21 read-only ones — both directions asserted by tests (`tests/cli_dry_run.rs`, 67 tests, including that read-only helps do **not** expose the flag). Wrapper-level short-circuits return before the mutating call in all 30 thin wrappers; `cp`/`mv`/`sync`/`clean` enforce dry-run in their engines. Dry-run also forces verbosity to at least info so the `[dry-run]` line is visible at the default warn level (`src/main.rs:62-134`). The strongest evidence is live: 34 e2e dry-run tests create real AWS state, run the dry-run, then verify via the SDK that nothing changed (`tests/e2e_dry_run.rs`).
- `clean` — the highest-blast-radius command — keeps its engine's safety gate: dry-run/`--force` checks, refusal to run non-interactively without `--force`, and an exact-`"yes"` confirmation prompt, checked before the Ctrl-C handler or indicator are even wired (`src/clean_bin/mod.rs:57-69`); at the prompt, Ctrl-C still kills the process via the OS default handler.
- `mv` (copy-then-delete) rejects self-moves before any transfer — same bucket, same endpoint, same *resolved* key, with an explicit `--source-version-id` allowed as version promotion except the `null` pseudo-version (`src/util_bin/cli/mv.rs:46-98`) — and deletes the source only after a four-gate decision tree (no cancellation, no transfer error, no verification warning unless `--no-fail-on-verify-error`, final cancellation re-check), pinning the delete to the version id the copy actually read (`mv.rs:100-162`). Every gate is unit-tested against a fake storage that counts delete calls.
- Object annotations: payloads are read through a hard 1 MiB bounded reader on both upload and download (TOCTOU-free `take(cap+1)` on upload, streamed cap on download so a hostile endpoint cannot OOM the process; `src/util_bin/cli/put_object_annotation.rs:44-61`, `get_object_annotation.rs:316-333`). Uploads send Content-MD5 plus CRC64NVME and fail if the response omits the verification checksum; downloads verify, write to a temp file, **re-verify the on-disk bytes, and only then atomically rename** — an s7cmd-local hardening over upstream (which renamed first), regression-tested to preserve a pre-existing good file on verification failure (`get_object_annotation.rs:181-225`).
- `create-bucket --tagging` is the one two-call wrapper: a tagging failure after bucket creation warns with the exact partial state and exits 3; no rollback, by design (`src/util_bin/cli/create_bucket.rs:83-91`).
- Remaining operator exposure: there is no interactive confirmation outside `clean`; and the upstream arg structs make positional `SOURCE`/`TARGET` (and e.g. `POLICY`) environment-backed, so an exported `TARGET` silently supplies the target of a destructive command — including on every `batch-run` line, which inherits the process environment. A deliberate upstream scripting affordance, but a real foot-gun.

#### Credential handling

Three independent layers were verified:

- Upstream types: `AccessKeys`, SSE-KMS ids and SSE-C keys in all four pinned engine crates derive `Zeroize`/`ZeroizeOnDrop` and print `** redacted **` from `Debug` (verified in the published 1.60.0/1.8.0/1.4.0/1.1.0 sources), so the trace-level `config = {:?}` lines in the sync/ls/clean dispatch arms cannot print a secret. The cp/mv arm goes further and logs only five enumerated non-secret fields instead of `{:?}` (`src/dispatch.rs:453-465`).
- `--help` cannot echo env-supplied secrets: `hide_credential_env_values` re-hides every env-backed `*access_key*` / `*session_token*` / `*sse_c_key*` argument tree-wide as a guard on top of the upstream fixes, enforced by a meta-test that walks every subcommand and requires ≥100 such arguments checked (`src/cli.rs:503-517,636-663`), plus a process-level test that exports real-shaped values and asserts names render but values never do — with a non-secret control value proving the test cannot pass vacuously (`tests/cli_help.rs:611-737`).
- `batch-run` masks inline credential option values to `****` before any line text reaches a log or `--json-tracing` output, in both `--flag value` and `--flag=value` forms, with a whitespace-level fallback so a secret on an unparseable (unbalanced-quote) line is still scrubbed (`src/batch_run/redact.rs`); masking is asserted end-to-end, including that a presigned URL never contains the secret key (`tests/batch_run.rs:1309-1410`).

#### Supply chain, build, and CI

- The four engine crates are exact-pinned (`=1.60.0`, `=1.8.0`, `=1.4.0`, `=1.1.0`) to keep the vendored frontend code in sync. `Cargo.lock` is committed; all release builds and the crates.io publish use `--locked`.
- TLS is rustls 0.23.42 with aws-lc-rs 1.17.3 and OS trust anchors; `openssl-sys` is absent from the lockfile and additionally deny-listed. `ring` enters the lock only via the e2e-test dev-dependency `ureq`, not the shipped binary.
- `cargo deny -L error check` runs on every push and PR (`ci.yml`) and daily at 01:34 UTC (`cargo-deny.yml`); `deny.toml` sets `advisories.ignore = []`, denies unknown registries and git sources, and restricts licenses to a permissive allowlist.
- CI builds and tests six targets with blocking `cargo fmt --check`, `cargo clippy -- -D warnings`, and cargo-deny jobs. The release workflow (`cd.yml`) triggers on version tags, builds `--locked`, attaches per-artifact SHA-256 checksums and GitHub build-provenance attestations, and publishes to crates.io via OIDC trusted publishing (no long-lived token).
- The release binary embeds a Lua interpreter (mlua 0.12.0) because s3sync's default features include `lua_support` for its filter-callback flags; `Cargo.toml:14-18` documents this and the opt-out. Intentional, but it is real additional attack surface, and `-rdynamic` linking on some targets exists solely to support it.
- Gaps worth naming: the gating clippy job runs without `--all-features` (the `--all-features` clippy run is a separate non-gating code-scanning workflow); no CI job compiles the `--cfg e2e_test` configuration, so the 258-test e2e corpus is type-checked only locally (it does compile warning-free — re-verified in this assessment); CI neither produces nor gates on coverage (the codecov badge is fed from local runs); `cd.yml` runs no tests itself; the Dockerfile builds without `--locked` (though it copies the committed lockfile).

#### Test corpus and coverage measurement

1,427 test annotations total: 477 unit tests in `src/` (the batch-run engine and dispatch table are the densest: 63 in `dispatch.rs`, 48 in `executor.rs`), 692 offline process-level tests (62 `tests/cli_*.rs` files plus `tests/batch_run.rs` — these spawn the real binary; S3 interactions hit a loopback-only mock bound to `127.0.0.1:0` with fake credentials passed as flags, so no AWS account or ambient credentials are touched), and 258 live-AWS tests in 28 `tests/e2e_*.rs` files, every one gated behind `#![cfg(e2e_test)]`. Sixteen distinct regression clusters pin previously fixed bugs, including the `--parallel` semaphore panic, the `AUTO_COMPLETE_SHELL` re-arming, the mv self-move data loss, credential values in `--help`, batch-run credential masking, and the mid-batch `process::exit` kills.

`llvm-cov-report.txt` (one combined unit + CLI + `--cfg e2e_test` live-AWS run, produced 2026-07-20 on the maintainer's machine; `lcov.info` is the same data in LCOV form, verified to carry identical totals) reports 97.63% region, 96.94% function, 98.55% line coverage over 82 files (the two 2-line module stubs contain no executable code). On the paths this assessment cares most about: `batch_run/executor.rs` 98.69% regions, `batch_run/mod.rs` 98.39%, `batch_run/redact.rs` 99.39% regions / 100% lines, `batch_run/validate.rs` 98.76%, `dispatch.rs` 98.33%, `util_bin/cli/mv.rs` 99.21% regions / 100% lines, `util_bin/cli/get_object_annotation.rs` 97.93%. The weakest files are small error-path wrappers (`head_bucket.rs` 80.65%, `delete_bucket_policy.rs` 82.61% regions). Branch coverage is not measured (the branch columns are empty), and coverage remains structural: a covered line can still be incorrect.

Test-harness limits found: the mock server replays canned responses and discards request bodies — only five files assert on captured requests, and only method + path, so a wrapper that sent a wrong body to the right URL would pass offline (the e2e round-trips are what actually verify payloads); roughly 100 tests are smoke-only (help exits 0, missing-args exits 2); `sync` and `mv` have no dedicated per-subcommand offline file; and the signal tests use fixed sleeps with bounded polls.

#### Known limitations and findings of this assessment

This from-scratch pass found no critical defects at v1.6.0. What it did find, ranked:

- Cancellation exit codes are inconsistent across engines, and `clean` can mask real errors on Ctrl-C. `cp`/`mv` exit 130 on SIGINT, but the vendored `sync`/`ls`/`clean` frontends map user cancellation to exit 0 — and `clean`'s error loop returns success on the first cancellation entry even when genuine deletion failures were recorded alongside it (`src/clean_bin/mod.rs:115-118`, faithfully vendored from the upstream binary). Consequently a Ctrl-C'd `clean` that had already hit real errors exits 0, and inside `batch-run` a Ctrl-C'd `sync`/`clean` line is bucketed `succeeded` while a `cp` line is bucketed `skipped`.
- The `mv` self-move guard compares endpoints textually (`src/util_bin/cli/mv.rs:66-77`, documented best-effort): two spellings of the same endpoint on an unversioned bucket re-open the delete-what-you-just-wrote window the guard exists to close. Bucket versioning remains the real backstop.
- Parallel-executor SIGINT window: the spawn loop checks the interrupt flag before awaiting a semaphore permit (or the channel), so a SIGINT that lands during that await lets one already-queued line spawn afterwards (`src/batch_run/executor.rs:399-430,537-577`); since its cancellation handler is installed after the signal fired, that line runs to completion rather than cancelling.
- Severity-ranking edges: any exit code outside 1–4 — including the synthetic panic code 101 — ranks below exit 4, so a `--continue-on-error` batch mixing a panic with a warning exits 3/4, understating what happened (the error-level `panicked` log entry is the reliable signal). In streaming mode a reader I/O error raises the exit only to `max(code, 1)` numerically, so prior warning exits (3/4) survive where the severity rule would surface 1 (`src/batch_run/mod.rs:402-412`).
- Wrapper exit-code inconsistencies: `rename` maps a missing object/bucket to exit 1 where sibling subcommands use 4; the mutating `put-*`/`delete-*` wrappers likewise surface NotFound as 1, so scripts cannot distinguish "bucket absent" from other failures on mutations; and `mv --no-fail-on-verify-error` exits 0 after a verification warning where `cp` exits 3.
- Dry-run is not always zero-network: every mutating wrapper builds its SDK client (credential resolution, possibly IMDS/SSO traffic) before the short-circuit, and three paths issue a live read-only call under `--dry-run` — `cp --skip-existing` (HeadObject), `create-bucket --if-not-exists` (HeadBucket), and `cp --enable-sync-object-annotations` (ListObjectAnnotations). No mutation is reachable on any dry-run path (verified per wrapper and asserted by the state-unchanged e2e suite).
- The nine `put-bucket-*` configuration bodies and the bucket policy are read with no client-side size cap — a mistyped multi-GB path is fully buffered before S3 rejects it. Batch-run's own 16 KiB line cap does not extend to the files those lines reference.
- A documented-but-unfixed defect, pinned by its own test rather than hidden: `batch-run --streaming -` with a stdin pipe that is never closed does not exit on SIGINT alone (`tests/cli_sigint.rs:72-80`); the reader's blocking stdin read survives the signal until EOF.
- The vendored frontend files cite older upstream versions (s3sync@1.57.1–1.58.6, s3rm-rs@1.3.3–1.3.4, s3ls-rs@0.4.1, s3util-rs@1.0.0–1.7.1) than the pinned engines they drive; the two 1.8.0-era fixes s7cmd needs are explicitly backported, and the stat-enum matches have no wildcard arms so upstream drift breaks the build rather than passing silently — but this vendored/engine skew is a standing maintenance risk unique to s7cmd's architecture.
- Smaller items: the annotation download's atomic rename is not preceded by an `fsync`, so a power loss (not a process crash) can persist the rename without the data; batch-run's structured logs and the trace-level config dumps can include the access key ID (an identifier, never the secret — and batch-run's own masking hides even the ID for inline flags); `restore-object --days` accepts any positive integer; duplicate tag keys pass client-side validation and are left to S3 to reject.

#### Is the software reliable?

For readers who are not software engineers: "reliable" here means whether a careful operator can use s7cmd for routine Amazon S3 work without losing data, leaking credentials, or having a batch script do something other than what was written.

**Risk: the wrapper corrupts or mis-routes.** s7cmd adds no transfer logic of its own — integrity verification, temp-file-then-rename downloads, and version-pinned reads live in the exact-pinned engines, and the wrapper preserves their guarantees (the one place it touches verified bytes, annotation downloads, it re-verifies on disk *before* the rename — stricter than upstream). Routing from subcommand string to engine call is pinned by three independent test layers.

**Risk: a batch run goes wrong.** Every line is validated before anything executes (in the default read-all mode, before *any* line executes); failures — including panics — are contained per line, counted against an explicit threshold, logged with the line number, and rolled into a severity-ranked exit code; inline credentials are masked in every log path; and a 200 MB-upload SIGINT test proves cancellation propagates. The residual sharp edges are the cancellation-exit-code inconsistency and the streaming-stdin SIGINT case above.

**Risk: operator mistake.** All 34 mutating subcommands take `--dry-run`, verified live against AWS to change nothing; `clean` requires `--force` or an interactive "yes"; `mv` refuses self-moves; `batch-run --check-format` validates a whole script without executing it. The exposures that remain are documented and operator-shaped: no interactive confirmation outside `clean`, and environment-backed positionals that can invisibly supply a target.

**What this assessment cannot establish.** That the code is bug-free; that the engine crates (assessed separately) are defect-free; or that S3-compatible endpoints behave like Amazon S3. The findings above that reach observable behavior — cancellation exit codes, `rename`'s exit-1, the severity-ranking edges — share a shape worth noting: they mis-*report* rather than mis-*act*; no finding of this pass identified a path that mutates or deletes the wrong data absent operator error, with the narrow documented exception of the mv endpoint-spelling window.

In plain terms: at v1.6.0 the failure modes most likely to cause silent harm — a batch that keeps running past failures it should stop on, a credential copied into a log, a wrapper that silently weakens an engine guarantee, a dry-run that mutates — each have a specific, citable, and tested safeguard, and this pass re-verified the fixes for every previously known defect, each pinned by a regression test. The fact that the codebase is AI-generated neither raises nor lowers its reliability; what determines that is the design, the tests, and the verifiable evidence above. The tool remains conditionally reliable: previewing destructive work with `--dry-run` (and `batch-run -v --check-format` for scripts), watching exit codes *and* error-level logs (exit codes alone understate panics and Ctrl-C'd `clean` runs), keeping bucket versioning for irreplaceable data, and least-privilege IAM are the operating controls the design assumes. Final responsibility for whether the binary fits a given workflow rests with the operator who runs it.

</details>

### AI assessment of safety and correctness (by Codex)

<details>
<summary>Click to expand the full assessment</summary>

> Assessment date: 2026-07-22.
>
> Assessed tree: `main` at `839acc3`. The Rust implementation and build inputs are identical to v1.6.0 (`c454694`); the commits after that tag change README content only.
>
> Complete review boundary: all 84 Rust files under `src/` were examined in full, including their 477 embedded tests, rather than sampling high-risk modules. The review also covered all 63 offline process-test files, `tests/common/mod.rs`, all 28 `e2e_*.rs` suites, `Cargo.toml`, `Cargo.lock`, `build.rs`, `.cargo/config.toml`, `deny.toml`, `Dockerfile`, and all five GitHub Actions workflows. The four exact-pinned engine crates were checked at the interfaces and guarantees on which this wrapper relies; they remain separate dependencies, not source owned by this repository. Conclusions were derived from code and executable evidence, not from the neighboring assessment text.
>
> Coverage evidence: the supplied `lcov.info` (SHA-256 `2da2901598bffa46b6fda6718c815e34701d24870c3e9f9a984a2da1b28bb22a`) and `llvm-cov-report.txt` (SHA-256 `f7fa71e841669bbb65673fe311832c7f12752b5218b3c9258a45e7a0053daffb`) were parsed independently. Their line and function totals agree exactly.
>
> Limits: this was source review plus deterministic local verification, not formal proof. No fuzzing, Miri, sanitizers, fault injection, penetration test, or fresh live-AWS run was performed. The E2E configuration was compiled and linted, while its prior live execution is represented only by the supplied coverage artifacts.

#### Bottom line

No critical vulnerability was found in s7cmd's own code, and the ordinary Amazon S3 paths show strong defensive engineering. The wrapper has complete dispatch coverage, contains per-line batch failures instead of terminating the process, protects credential values in help and batch logs, and preserves dry-run and transfer-integrity controls. There is no `unsafe` block in production code and no production subprocess execution or shell evaluation; `batch-run` parses and dispatches commands in-process.

I would nevertheless describe v1.6.0 as **conditionally reliable, not safety-certified**. One narrow `mv` case can still delete an object, and batch input/cancellation behavior has availability and result-reporting defects. These are concrete open findings, not hypothetical objections to AI-generated code.

#### Open findings, ordered by operational impact

| Impact / priority | Finding | Consequence and boundary |
|---|---|---|
| High impact, narrow trigger | The `mv` self-move guard compares source and target endpoint strings literally (`src/util_bin/cli/mv.rs:65-77`). Equivalent spellings of the same service therefore bypass the guard. | With the same bucket and resolved key on an unversioned or version-suspended bucket, copy-then-delete can delete the object just written. The operator must explicitly construct this endpoint mismatch; bucket versioning is the strongest mitigation. Endpoint canonicalization or an explicit same-service override would close it. |
| Moderate | Batch memory is not bounded in aggregate. Default mode collects every parsed line in a `Vec` (`src/batch_run/parser.rs:60-88`), while streaming mode feeds an `unbounded_channel` (`src/batch_run/mod.rs:361`). | The 16 KiB per-line limit prevents a single-line allocation attack but not a huge script, and a fast streaming producer can outpace a slow S3 command until memory is exhausted. A total-line/byte cap and a bounded channel would make the resource guarantee real. |
| Moderate | Cancellation is reported inconsistently. `ls` maps cancellation to 0 (`src/ls_bin/mod.rs:75-78`); `sync` can complete a cancelled pipeline without a distinct cancellation status; and `clean` returns 0 as soon as it encounters any cancellation error, even if the same collected error set contains real deletion failures (`src/clean_bin/mod.rs:98-124`). | Automation and `batch-run` can classify interrupted work as success. In `clean`, a genuine error may be masked. Error aggregation should retain real errors and use one cancellation code, preferably 130, across engines. |
| Moderate | Streaming stdin is not promptly cancellable. After fail-fast or SIGINT the executor drains the channel and awaits its closure, then `run_streaming` awaits the reader (`src/batch_run/executor.rs:484-505,579-582`; `src/batch_run/mod.rs:399-412`). Tokio's stdin read can remain blocked until the producer closes the pipe. | `batch-run --streaming -` can hang after SIGINT or an early failure when stdin stays open. The process test explicitly documents that EOF is load-bearing (`tests/cli_sigint.rs:68-105`). A cancellable reader design or documented external pipe closure is required. |
| Low to moderate | Parallel stop checks occur before awaiting the next channel item or semaphore permit, with no re-check after the await (`src/batch_run/executor.rs:398-430,537-577`). | A SIGINT or reached error threshold can allow one already-queued command to start afterward. Because that command installs its own handler after the signal, it may run to completion. |
| Low to moderate | Exit aggregation understates some failures. All nonstandard codes, including caught panic 101, rank below warning 3 and not-found 4 (`src/batch_run/executor.rs:289-313`); streaming reader failure uses numeric `max(code, 1)` (`src/batch_run/mod.rs:402-411`). | A batch containing a panic plus a warning can exit 3/4, and a reader error after a warning can retain the warning code. Error-level logs remain accurate, but exit-code-only automation is not. |
| Low, availability | Nine JSON file/stdin inputs are read with unbounded `read_to_string` before parsing or sending: policy, CORS, encryption, lifecycle, logging, notification, replication, website, and public-access-block. | A mistaken or hostile multi-gigabyte input can exhaust memory. The annotation payload paths correctly demonstrate the bounded-read pattern and should be reused. |

Additional correctness edges are smaller but real. `rename` reports missing source/bucket as general error 1 while many sibling commands use not-found 4 (`src/util_bin/cli/rename.rs:52-58`). `mv --no-fail-on-verify-error` can delete the source and return success despite a verification warning, whereas `cp` returns warning 3. Annotation file output flushes, re-reads, verifies, and atomically renames, but does not `sync_all`, so its promise covers process crashes and detected corruption rather than power-loss durability (`src/util_bin/cli/get_object_annotation.rs:181-224`). Positional arguments inherited from the engine CLIs are environment-backed, so exported `SOURCE`, `TARGET`, or policy variables can silently fill destructive command arguments. There is no confirmation prompt outside `clean`.

`--dry-run` prevents mutations, but it is not a no-I/O or no-network guarantee. Client construction happens first and may perform credential-provider work. `sync`, `clean`, and transfer planning can enumerate or inspect S3 state; thin-wrapper examples include `cp --skip-existing`, annotation synchronization, and `create-bucket --if-not-exists`, which perform read-only S3 checks before deciding what they would do (`src/util_bin/cli/cp.rs:17-33`; `src/util_bin/cli/create_bucket.rs:45-64`). This is safe with respect to S3 state but matters in isolated environments and when metadata/SSO credential providers are enabled.

#### Safety controls that held under full-source review

- The 56-variant command enum has a corresponding non-exiting dispatch path. Configuration failures return 2 instead of invoking upstream `clap::Error::exit`, so one bad batch line cannot terminate its siblings (`src/dispatch.rs`). The large transfer futures are boxed to avoid known small-stack overflows.
- All 34 mutating command surfaces expose `--dry-run`. Thin wrappers return before their mutation calls, while `cp`, `mv`, `sync`, and `clean` carry the flag into their pinned engines. The live E2E corpus contains a state-unchanged case for every mutating command. Read-only setup and planning can still contact S3 as described above.
- `mv` normally has a sound deletion decision tree: no delete after cancellation, copy error, or verification warning without the explicit override; it checks cancellation again immediately before delete and pins deletion to the source version actually read (`src/util_bin/cli/mv.rs:100-161`). The endpoint-equivalence gap above is before that tree, not a failure of those gates.
- Annotation upload and download enforce the 1 MiB limit with bounded streaming reads. Upload sends Content-MD5 and CRC64NVME and verifies the returned checksum; download verifies content length plus available checksums, rejects unsupported returned algorithms cleanly, writes beside the target, re-verifies the saved bytes, then atomically persists. A pre-existing destination survives verification failure (`src/util_bin/cli/put_object_annotation.rs:44-100`; `get_object_annotation.rs:87-224,315-394`).
- `batch-run` enforces a 16 KiB line limit incrementally, rejects invalid UTF-8, tokenizes with `shlex`, rejects nested batches, stdin/stdout conflicts and per-line tracing, caps `--parallel` at 1024, and converts parse/validation errors into per-line exit 2. Dispatch is wrapped in `catch_unwind`, and all build profiles retain `panic = "unwind"`, so a subcommand panic becomes a recorded exit 101 instead of tearing down the batch (`src/batch_run/`; `Cargo.toml:103-113`).
- Credential values are hidden from env-aware help output across the complete clap tree (`src/cli.rs:486-517`). Batch log redaction handles both `--flag value` and `--flag=value`, including the malformed-quoting fallback (`src/batch_run/redact.rs`). The pinned engine credential types redact `Debug` output and zeroize secret fields, so the remaining trace-level full-config logs do not print secret keys or session tokens. Access-key identifiers may remain partially visible by design.
- Production source contains no direct `unsafe`, no shell invocation, and no process-spawning API. Lua support is inherited from s3sync's default features and deliberately expands the binary's attack surface, but Lua runs only operator-supplied filter code rather than data received from S3.

#### Tests, coverage, and what the numbers mean

The supplied report totals are 97.63% regions (14,282/14,629), 96.94% functions (1,110/1,145), and 98.55% lines (10,297/10,448). There are no branch records: both artifacts report zero measured branches. The line and function figures are internally consistent, but two qualifications prevent treating 98.55% as production-only coverage:

1. `lcov.info` includes functions named under `::tests::` and line records from `#[cfg(test)]` modules—for example, the `mv` fake storage and tests above line 900. The 10,448-line denominator therefore mixes product and test implementation. Similarly, the README's 16,949 physical lines under `src/` include the 477 embedded tests; they are not all production lines.
2. The loopback mock records method and request target, then drains request bodies without retaining them and exposes no captured request headers (`tests/common/mod.rs:200-259`). Most offline wrapper tests therefore prove parsing, routing, status mapping, and response handling, but not exact outbound JSON or signed headers. Live-AWS round trips provide the stronger payload evidence and are not a CI gate.

The corpus is broad: 477 embedded test annotations, 692 annotations in 63 offline process-test files, and 258 annotations in 28 gated live-AWS files. Some tests are explicitly coverage-oriented rather than behavioral—for example, local sync tests ignore `run`'s result—so count and coverage must be read alongside assertion quality.

During this assessment, these commands passed without warnings or failures:

- `cargo fmt --all --check`
- `cargo test --all-features --locked`
- `cargo clippy --all-features --all-targets --locked -- -D warnings`
- `RUSTFLAGS="--cfg e2e_test" cargo clippy --all-features --all-targets --locked -- -D warnings`
- `cargo deny -L error check` (`advisories`, `bans`, `licenses`, and `sources` all clean)

The E2E tests were compiled by the second clippy run but not executed because doing so mutates a configured AWS account. Default `cargo test` correctly sees those gated files as zero-test targets.

#### Dependency and delivery assessment

The four engine crates are exact-pinned and the lockfile is committed. Release and publish jobs use `--locked`, produce SHA-256 files, and attest release archives. The resolved HTTP stack uses rustls 0.23.42; `openssl-sys` is absent and explicitly denied. Cargo-deny rejects unknown registries and git sources and has no ignored advisories.

The remaining supply-chain weaknesses are conventional rather than runtime defects: most GitHub Actions are referenced by mutable major-version tags rather than commit SHAs; CI follows moving `stable`; the gating clippy job omits `--all-features --all-targets`; CI does not compile the `e2e_test` configuration or gate on coverage; release jobs do not rerun tests; and the Dockerfile uses mutable base tags and builds without `--locked`. These do not invalidate the tested source tree, but they weaken reproducibility and increase the trust placed in workflow dependencies.

#### Reliability conclusion

For routine, supervised Amazon S3 use, the evidence supports the tool's core claims: routing is exhaustive, mutation previews exist everywhere, credential handling is deliberate, transfer deletion is normally gated, annotation bytes receive unusually strong integrity treatment, and failures generally become structured nonzero results. I found no path in the ordinary same-endpoint flow that silently mutates the wrong S3 resource, no designed logging path that emits credential secrets, and no dry-run path that invokes an S3 mutation.

For unattended destructive automation, the conditions matter. Canonicalize and keep source/target endpoints identical for same-service `mv`, enable bucket versioning for valuable data, bound or trust batch inputs, close streaming stdin on cancellation, and monitor error logs in addition to the final exit code. Until the `mv` endpoint comparison, aggregate batch bounds, and cancellation/error aggregation are fixed, those controls are part of the safety model rather than optional operational advice.

</details>

### AI assessment of safety and correctness (by Gemini)

<details>
<summary>Click to expand the full assessment</summary>

> Assessment date: 2026-07-22.
>
> Assessed version: 1.6.0 (branch `main`, commit `837518b`, tagged `v1.6.0` at `c454694`).
>
> Method and scope of evidence: This assessment was performed completely from scratch for v1.6.0, independently reviewing the entire codebase without relying on prior summaries. All 84 Rust source files under `src/` (16,949 physical lines) were systematically inspected in full, including the `s7cmd`-original `src/batch_run/` engine (8 modules), top-level CLI parsing and dispatch (`src/main.rs`, `src/cli.rs`, `src/dispatch.rs`), and all four vendored CLI frontends (`src/util_bin/`, `src/sync_bin/`, `src/clean_bin/`, `src/ls_bin/`, total 73 files). Additionally, all 63 offline process-level test suites (`tests/cli_*.rs` and `tests/batch_run.rs`), 28 live-AWS E2E test suites (`tests/e2e_*.rs`), test harness helpers (`tests/common/mod.rs`), build configuration (`Cargo.toml`, `Cargo.lock`, `build.rs`, `.cargo/config.toml`), security policies (`deny.toml`), `Dockerfile`, and all five GitHub Actions workflows were examined. Code coverage artifacts `llvm-cov-report.txt` and `lcov.info` were verified for internal consistency (line and function totals match exactly). Local verification commands were run from scratch and confirmed clean: `cargo fmt --all --check` zero diffs; `cargo clippy --all-features --all-targets --locked -- -D warnings` zero warnings; `RUSTFLAGS="--cfg e2e_test" cargo clippy --all-features --all-targets --locked -- -D warnings` zero warnings; `cargo test --all-features --locked` 477 unit tests passed; `cargo deny -L error check` clean with zero advisories ignored. Interfaces to the four exact-pinned upstream engine crates (`s3sync = "=1.60.0"`, `s3util-rs = "=1.8.0"`, `s3rm-rs = "=1.4.0"`, `s3ls-rs = "=1.1.0"`) were audited.
>
> Limits of evidence: This review constitutes static code audit and deterministic local test execution. It does not include formal mathematical proof, fuzzing, Miri execution, memory/thread sanitizers, or live AWS network mutation runs. E2E tests are gated under `cfg(e2e_test)` and run against maintainer AWS infrastructure; coverage measures code execution paths, not absolute logical correctness under arbitrary cloud edge cases.

**Question addressed.** As a CLI wrapper and batch runner for Amazon S3, s7cmd delegates 55 of its 56 subcommands to four exact-pinned engine crates while providing one custom engine (`batch-run`). This evaluation examines whether s7cmd introduces routing errors, credential leaks, unhandled panics, uncontained batch failures, data destruction during transfers, or unexpected mutation behaviors during dry runs.

#### Command Dispatch & Structural Non-Exiting Guarantee

- **Complete Subcommand Routing**: The `Cmd` enum defines 56 variants (`src/cli.rs:270-426`): 21 read-only subcommands, 34 mutating subcommands, and `batch-run`. `src/dispatch.rs` maps every variant to its underlying logic. Routing correctness is validated across 62 subcommand parsing tests (`tests/cli_routing.rs`), 63 unit tests in `src/dispatch.rs`, and E2E suites.
- **Process Stability & Non-Exiting Contract**: Production code under `src/` contains zero calls to `std::process::exit`. Every dispatch branch returns a numeric status (`ExitStatus` or `i32`), which `main()` converts to `std::process::ExitCode` (`src/main.rs:44-45`). In the vendored frontends, upstream process-exiting calls (`load_config_exit_if_err`, state flag validation exits) were deliberately refactored into non-exiting status returns (`dispatch.rs:278,401,418`). A configuration or parameter error in one subcommand returns exit 2 without terminating the parent process or killing a `batch-run` sequence.
- **Stack Memory Protection**: Large subcommand future types are explicitly `Box::pin`-ed (`src/dispatch.rs:16-22`) to keep dispatch stack frames well under the 2 MB stack limit of test worker threads.

#### `batch-run` Engine Architecture & Fault Isolation

`batch-run` represents s7cmd's original execution engine, built with multi-layered defensive controls:

- **Incremental Line Buffering**: Input lines are read using `read_line_capped` (`src/batch_run/parser.rs:101-135`), which enforces a strict 16 KiB limit (`MAX_LINE_LEN`) incrementally via `BufRead::fill_buf`. Pathological multi-gigabyte single-line inputs are aborted after buffering ~16 KiB rather than exhausting process memory. UTF-8 validation and POSIX shell tokenization (`shlex`) are applied to every line.
- **Pre-Execution Validation**: `src/batch_run/validate.rs` validates parsed argument structures before running commands. It explicitly rejects nested `batch-run` invocations, stdin/stdout dash operands (`-`), and per-line tracing/verbosity flags (`-v`, `--tracing-log-format`). Validation failures synthesize exit 2 and count toward `--max-errors` / `--continue-on-error` thresholds rather than aborting the batch.
- **Panic Boundary Containment**: Every subcommand execution is wrapped in `futures::FutureExt::catch_unwind` (`src/batch_run/executor.rs:125-167`). Any unexpected panic inside a subcommand is caught, logged with line numbers and redacted text, assigned exit code 101, and counted toward error limits. This mechanism relies on `panic = "unwind"` specified across all build profiles in `Cargo.toml:103-113`.
- **Severity-Ranked Exit Codes**: Batch exit status is determined by severity ranking rather than simple maximum value: `exit 1` (error) > `exit 2` (arg/validation error) > `exit 3` (warning) > `exit 4` (not found) > other non-zero > `exit 0` (`src/batch_run/executor.rs:294-303`). Per-line SIGINT (exit 130) is bucketed as `skipped` (`executor.rs:281-287`) and does not trip error thresholds.
- **Phased Signal Handling**: Signal listeners are not installed during the script reading/validation phase (where Ctrl-C terminates immediately). The SIGINT handler is registered only before command execution starts, ensuring in-flight futures handle cancellation cleanly while preventing new commands from spawning (`src/batch_run/mod.rs:253-331`).
- **Parallel Execution Safety**: `--parallel` concurrency is constrained to `[1, 1024]` by a custom clap parser (`src/cli.rs:148-170`), preventing semaphore allocation panics. Tokio `LocalSet` drives async execution with a concurrency semaphore.
- **Shell Auto-Completion Isolation**: Top-level `--auto-complete-shell` is disarmed on subcommands (`src/main.rs:26-34`), preventing inherited environment variables from altering subcommand argument parsing.

#### Operator Safeguards, Dry-Run Integrity & Transfer Safety

- **Comprehensive Dry-Run Coverage**: All 34 mutating subcommands accept `--dry-run`; none of the 21 read-only subcommands accept it (`tests/cli_dry_run.rs`). Thin wrappers abort before invoking mutating S3 API calls, while complex operations (`cp`, `mv`, `sync`, `clean`) propagate `--dry-run` into their underlying engine. Dry-run automatically elevates minimum logging verbosity to `info` level (`src/main.rs:62-134`) so `[dry-run]` execution logs are visible. 34 E2E live-AWS tests verify that dry-run calls leave cloud resources unmodified (`tests/e2e_dry_run.rs`).
- **High-Risk Delete Protection**: `clean` (bulk delete) mandates `--force` or interactive `"yes"` confirmation (`src/clean_bin/mod.rs:57-69`). Interrupted prompts exit via default OS signal handling.
- **`mv` Copy-Then-Delete Decision Tree**: `mv` checks for self-move conditions (`src/util_bin/cli/mv.rs:46-98`) by comparing source and target buckets, endpoints, resolved keys, and version IDs before executing any transfer. Deletion of the source object is guarded by a 4-gate decision tree (`mv.rs:100-162`): (1) no cancellation during transfer, (2) successful copy completion, (3) no checksum/verification warnings (unless `--no-fail-on-verify-error`), (4) final cancellation token re-check immediately prior to delete. Source deletion specifies the exact version ID read during copy.
- **Object Annotation Integrity**: Annotation payloads enforce a 1 MiB limit (`src/util_bin/cli/put_object_annotation.rs:44-61`, `get_object_annotation.rs:316-333`). Uploads verify Content-MD5 and CRC64NVME response checksums. Downloads stream bytes into a temporary file (`tempfile`), verify on-disk payload checksums, and perform an atomic filesystem rename only after successful verification (`get_object_annotation.rs:181-225`). Pre-existing destination files remain intact if verification fails.
- **Partial State Warnings**: `create-bucket --tagging` executes bucket creation followed by tagging. If tagging fails after bucket creation, it emits a warning detailing partial state and exits 3 (`src/util_bin/cli/create_bucket.rs:83-91`).

#### Credential Hygiene & Masking

- **Engine Credential Redaction**: Access key structs and SSE encryption keys in engine crates derive `Zeroize`/`ZeroizeOnDrop` and implement `Debug` formatting returning `** redacted **`. Detailed config dumps in `cp`/`mv` dispatch limit logged fields to non-sensitive metadata (`src/dispatch.rs:453-465`).
- **Help Output Protection**: `hide_credential_env_values` (`src/cli.rs:503-517,636-663`) recursively removes default environment variable secret values from `--help` text across all subcommands. Process-level tests (`tests/cli_help.rs:611-737`) confirm credential flag names display without leaking environment variable contents.
- **Batch Log Redaction**: `src/batch_run/redact.rs` sanitizes inline credentials (access keys, secret keys, session tokens, presigned URLs) in both `--flag value` and `--flag=value` syntax before logging or writing JSON trace records. Unparseable lines use fallback regex/whitespace scrubbing (`tests/batch_run.rs:1309-1410`).

#### Supply Chain & Build Pipeline Security

- **Pinned Engine Dependencies**: `Cargo.toml` pins engine dependencies to exact versions (`s3sync = "=1.60.0"`, `s3util-rs = "=1.8.0"`, `s3rm-rs = "=1.4.0"`, `s3ls-rs = "=1.1.0"`). `Cargo.lock` is committed; build and publication workflows enforce `--locked`.
- **TLS & Crypto Stack**: Cryptographic transport uses `rustls 0.23.42` with `aws-lc-rs 1.17.3` and OS trust anchors. `openssl-sys` is excluded from the dependency tree and explicitly banned in `deny.toml`. `ring` is restricted to `ureq` test dependencies.
- **Dependency Auditing**: `cargo deny -L error check` runs on all pushes/PRs (`ci.yml`) and daily schedules (`cargo-deny.yml`). `deny.toml` maintains `advisories.ignore = []` and enforces license allowlists.
- **Release Provenance**: Release builds (`cd.yml`) compile with `--locked`, produce SHA-256 digests, generate GitHub Actions build provenance attestations, and publish to crates.io via OIDC trusted publishing.
- **Embedded Lua Interpreter**: `s3sync` includes `lua_support` (mlua 0.12.0) by default to support Lua filter callbacks (`Cargo.toml:14-18`). While intentional, embedding Lua increases binary attack surface.

#### Test Corpus & Coverage Evidence

s7cmd includes 1,427 test annotations across three tiers:
1. **Embedded Unit Tests**: 477 tests in `src/` (covering `dispatch.rs`, `executor.rs`, `redact.rs`, `mv.rs`, `get_object_annotation.rs`, etc.).
2. **Offline Integration Tests**: 692 tests across 63 files (`tests/cli_*.rs` and `tests/batch_run.rs`) executing CLI binaries against an in-process loopback mock server (`127.0.0.1:0`).
3. **Live AWS E2E Suites**: 258 tests across 28 files (`tests/e2e_*.rs`) gated behind `cfg(e2e_test)`.

`llvm-cov-report.txt` and `lcov.info` present combined test coverage (unit + CLI + live-AWS E2E run):
- **Line Coverage**: 98.55% (10,297 / 10,448 executable lines; 151 missed)
- **Function Coverage**: 96.94% (1,110 / 1,145 functions; 35 missed)
- **Region Coverage**: 97.63% (14,282 / 14,629 regions; 347 missed)
- **Branch Coverage**: Not measured by standard llvm-cov instrumentation in Rust.

Core module coverage highlights: `batch_run/executor.rs` (98.73% lines), `batch_run/mod.rs` (98.87% lines), `batch_run/redact.rs` (100% lines), `util_bin/cli/mv.rs` (100% lines), `util_bin/cli/get_object_annotation.rs` (99.26% lines).

#### Identified Technical Findings & Operational Limitations

1. **Non-Canonical Endpoint Comparison in `mv` Self-Move Guard**: `check_not_self_move` (`src/util_bin/cli/mv.rs:67-77`) compares endpoint URLs using string equality (`source_endpoint != target_endpoint`). Syntactically distinct spellings of the same S3 endpoint (e.g., HTTP vs HTTPS, IP vs hostname) bypass the self-move check, relying on bucket versioning to prevent data loss.
2. **Unbounded Aggregate Batch Memory**: While single lines are capped at 16 KiB, standard `batch-run` buffers all parsed lines in a `Vec` (`src/batch_run/parser.rs:61`), and `--streaming` mode uses an `unbounded_channel` (`src/batch_run/mod.rs:361`). Scripts with millions of lines can consume substantial memory.
3. **Inconsistent Cancellation Exit Reporting**: Cancellation handling varies by subcommand: `ls` maps cancellation to exit 0 (`src/ls_bin/mod.rs:75-78`); `clean` returns exit 0 upon encountering cancellation even if earlier object deletion errors occurred (`src/clean_bin/mod.rs:98-124`); `sync` maps cancellation to exit 0.
4. **Streaming Stdin Cancellation Delay**: In `batch-run --streaming -`, reading from an open stdin pipe can remain blocked on Tokio's stdin reader after SIGINT or early error failure until EOF is received (`tests/cli_sigint.rs:72-80`).
5. **Parallel Executor Queue Signal Window**: In parallel execution, the interrupt check occurs before awaiting a semaphore permit (`src/batch_run/executor.rs:399-430`). A SIGINT arriving during permit await can allow one previously queued command to start.
6. **Severity Code Aggregation Edge Cases**: Exit codes outside 1–4 (including panic exit 101) rank below exit 4 in batch severity ranking (`src/batch_run/executor.rs:289-313`), causing a batch containing a panic and a warning to exit 3 or 4.
7. **Uncapped JSON Configuration Reads**: Nine bucket configuration commands (policy, CORS, lifecycle, encryption, website, logging, notification, replication, public-access-block) read input files using unbounded `read_to_string`, unlike annotation payloads which enforce a 1 MiB cap.
8. **Network Activity During Dry Runs**: Subcommands with `--dry-run` perform client configuration and read-only S3 checks (e.g. `cp --skip-existing` issuing HeadObject, `create-bucket --if-not-exists` issuing HeadBucket) before suppressing mutating calls.

#### Reliability Summary

s7cmd exhibits robust defensive engineering for Amazon S3 management. Subcommand routing is complete, non-exiting dispatch guarantees process persistence during batch runs, panic boundaries prevent process crashes, credential redaction is systematically implemented, and object annotation workflows provide strong atomic verification.

The binary is **conditionally reliable**:
- Destructive operations should be previewed using `--dry-run` (and `batch-run --check-format` for batch scripts).
- S3 bucket versioning should be enabled for critical datasets.
- Automated workflows should monitor structured error logs in addition to numeric exit status.
- Source and target endpoints in `mv` scripts should use canonical, identical strings.

</details>

### Scope

s7cmd is designed to cover **Amazon S3 object operations and bucket
management** — listing (`ls`), single- and bulk-object transfers
(`cp` / `mv` / `rm`), atomic server-side rename (`rename`), recursive
synchronization (`sync`), bulk delete (`clean`), archive restoration
(`restore-object`), pre-signed URL generation (`presign`), and the
common bucket-level configurations
(tagging, versioning, policy, policy-status, lifecycle, encryption,
CORS, public-access-block, website, logging, notification,
replication, transfer acceleration, request payment). For any S3 use
case outside that scope, use a more comprehensive tool such as the
[AWS CLI](https://aws.amazon.com/cli/) (`aws s3api`).

s7cmd targets **Amazon S3** as its only supported platform and is
optimized for Amazon S3 performance.

> Note that "optimized for Amazon S3 performance" includes the
> allowed request rate: Amazon S3 supports at least 3,500
> PUT/COPY/POST/DELETE or 5,500 GET/HEAD requests per second per
> partitioned prefix, and s7cmd's parallel `ls`, `clean`, and
> `sync` engines assume that capacity. The request rate allowed by
> S3-compatible storage may differ significantly, so running `ls`,
> `clean`, or `sync` against S3-compatible storage can exceed the
> service's rate limit. The request rate can be capped with each
> command's rate-limit option (`--rate-limit-api` for `ls`,
> `--rate-limit-objects` for `clean` and `sync`).

S3-compatible storage (MinIO, Cloudflare R2, Backblaze B2, Wasabi,
Ceph RGW, DigitalOcean Spaces, IBM COS, and similar) is provided
strictly **as-is**, with **absolutely no support or assistance** —
such services may work via `--endpoint-url` (and
`--source-force-path-style` / `--target-force-path-style` when
path-style addressing is required), but they are not part of the
official test matrix and behavior may change between releases. This
is a structural consequence of building on `aws-sdk-rust`, which is
generated from AWS service models and assumes Amazon S3 semantics
(checksum headers, endpoint resolution, signing variants, response
schemas); features that depend on AWS-specific semantics, such as
CRC64NVME checksums or newer S3 API additions, may not work against
non-AWS endpoints. In practice, most s7cmd subcommands are not
supported on S3-compatible storage and are therefore unlikely to
work: the bucket-configuration family drives management APIs that
such services implement partially or not at all, and `rename`,
`restore-object`, and the object-annotation subcommands depend on
Amazon-S3-only APIs and checksum semantics. Bug reports, questions,
and assistance requests regarding S3-compatible storage will not be
addressed.

s7cmd is **not** intended to be a drop-in replacement for, or
behaviorally compatible with, any other S3 client — including the
AWS CLI (`aws s3`, `aws s3api`) and tools such as `s3cmd`, `s4cmd`,
`s5cmd`, `s6cmd`, and `rclone`. Its command-line flags, transfer semantics,
verification rules, and exit codes are designed around the
underlying libraries' own scope and design principles — not
interoperability with another tool's interface. Output formats and
flag names will not be adjusted to match any external tool, and
scripts written against another S3 client should not be expected to
work with `s7cmd` unmodified. The numeric progression in the name
(`s3cmd` → `s4cmd` → `s5cmd` → `s6cmd` → `s7cmd`) does **not** imply
succession or compatibility.

## Non-Goals

The following are explicitly out of scope and will not be added,
regardless of demand:

- Support, testing, or guaranteed compatibility for any
  storage service other than Amazon S3. S3-compatible storage is
  provided strictly as-is, with no support or assistance, as
  described in the Scope section above; adding dedicated code
  paths, provider-specific workarounds, or backends for services
  such as MinIO, Cloudflare R2, Backblaze B2, Wasabi, Ceph RGW,
  DigitalOcean Spaces, IBM COS, Tencent COS, Alibaba OSS, Azure
  Blob Storage, or Google Cloud Storage is out of scope.
- Feature parity with, or porting features from, other S3 clients.
  Feature requests of the form "tool X has feature Y, please add
  it to s7cmd" — including variants such as "feature Y would also
  be useful in s7cmd," "many users expect Y because tool X has it,"
  or "Y is missing compared to tool X" — will be closed without
  further discussion. The existence of a feature, flag, command,
  output format, or behavior in `aws s3`, `aws s3api`, `s3cmd`,
  `s4cmd`, `s5cmd`, `s6cmd`, `rclone`, or any other S3 tool carries no weight
  in s7cmd's design decisions, regardless of how the request is
  framed. Each feature is evaluated solely against s7cmd's own
  scope and the design principles of its underlying libraries. If
  the feature you need exists in another tool, use that tool.
- Outperforming other S3 tools on raw speed or memory usage.
  Performance and resource consumption are addressed only when they
  compromise practical workflows — not for edge cases or benchmark
  wins. Issues of the form "tool X transfers Y MB/s faster",
  "tool X transfers Y objects/second faster", or
  "tool X uses less RAM than s7cmd in benchmark Z" will be closed.
  If raw throughput is your top criterion, use a tool optimized
  for it.
- FUSE filesystem mounting, daemon mode, or any persistent
  background process. s7cmd is a one-shot CLI; it runs, transfers,
  and exits.
- Workflow orchestration features — scheduling, cross-run state
  databases, retry queues that survive process restart, or DAG
  execution. Use a workflow engine such as Airflow, Argo Workflows,
  or AWS Step Functions for orchestration.
- A graphical user interface, a TUI, or an interactive shell mode.
- A plugin or extension mechanism.
- AWS service coverage beyond S3. s7cmd will not add subcommands for
  IAM, KMS, CloudFront, or any other AWS service, even when they
  interact closely with S3.
- Edge cases that are more reasonably addressed by using the AWS
  SDK directly, shell scripting, or other purpose-built tooling.
  s7cmd is not intended to cover every conceivable S3 use case;
  niche or one-off requirements that can be straightforwardly
  handled by combining the AWS SDK, shell pipelines, or existing
  tools fall outside its scope.
- Changes to flag names, subcommand names, default values, output
  formats, log formats, or exit code assignments based on subjective
  preference. Such interfaces are stabilized once shipped; breaking
  changes are made only when required by an underlying library, an
  upstream SDK, or a clear correctness bug.
- Additional platform targets, distribution channels, or package
  manager registrations beyond those listed in Requirements and
  Installation. Community-maintained packages are welcome but will
  not be endorsed or supported.

Issues and pull requests requesting any of the above will be closed.

## Maintenance Model

s7cmd is maintained as a personal project. Dependency updates and
critical bug fixes are applied on a best-effort basis. New features
are not actively solicited. If you need guaranteed enterprise
support, this is not the tool for you.

## Intended Audience and Issue Tracker Scope

s7cmd assumes operational familiarity with Amazon S3 and the AWS
SDK. It is aimed at engineers who already run S3 workloads — not
at learners or general AWS users.

The issue tracker accepts:

- Reproducible defects in s7cmd's own behavior (with version,
  exact command, and observed vs. expected output).
- Scope-aligned feature discussion, subject to the Non-Goals
  section above.

The issue tracker does **not** accept:

- General questions about S3, IAM, AWS credentials, or AWS
  account configuration. See the [AWS documentation](https://docs.aws.amazon.com/s3/).
- Usage questions about other S3 clients.
- Help with user shell scripts, pipelines, or CI configurations
  that do not isolate an s7cmd-specific defect.
- Tutorials or design consulting.
- Diagnosing or fixing performance degradation, resource exhaustion,
  or errors caused by raising concurrency settings.
- Questions and issues that belong with AWS, with the operator
  of an S3-compatible storage service, or with the operating
  system vendor rather than with s7cmd — including general S3,
  IAM, KMS, networking, and account-configuration questions;
  S3 (or S3-compatible) service behavior such as request rate
  limits, 503 SlowDown, consistency semantics, or regional
  availability; operating-system configuration and behavior such
  as `ulimit` and file-descriptor limits, kernel networking
  parameters, filesystem quirks, shell quoting, path-length
  limits, code signing, or antivirus interference; and anything
  that reproduces with the AWS CLI, the AWS SDK, or the vendor's
  own client directly. Refer to the [AWS documentation](https://docs.aws.amazon.com/s3/),
  AWS Support, your storage vendor's documentation, or your OS
  vendor's documentation. If the issue is not specific to s7cmd's
  own code, it belongs there, not here.

Out-of-scope issues will be closed without further discussion.

## Contributing

- Bug reports are welcome, but responses are not guaranteed.
- Since this project is considered functionally complete, I will not accept any feature requests.
- If you find this project useful, feel free to fork and modify it as you wish.

🔒 I consider this project “complete” and will maintain it only minimally going forward.
However, I intend to keep the AWS SDK for Rust and other dependencies up to date monthly.

**Issue and PR lifecycle**

To keep the tracker focused, an issue or PR with no activity for 30 days is labeled `stale` and closed 7 days later unless a new comment (or, for PRs, a new commit) is added. Items labeled `pinned` or `security` are exempt; PRs are also exempt from `pinned`. Closed items can always be reopened.

## License

Apache-2.0
