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
bucket administration (lifecycle, policy, encryption, CORS, public
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
| `--parallel N` | Run up to *N* commands concurrently. Completion order is not guaranteed. |
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
is error level, all three visible without `-v`.

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

s7cmd targets **Amazon S3** as its only supported platform.
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
non-AWS endpoints. Bug reports, questions, and assistance requests
regarding S3-compatible storage will not be addressed.

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
