# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.8.0] - 2026-08-07

Bug-fix release, taken as a minor version because the fix changes an observable exit code. No library pins move;
the fix lands in s7cmd's vendored command frontends.

**Upgrade notes:**

- [Breaking change] `sync`, `ls`, and `clean` runs interrupted by Ctrl+C (SIGINT) now exit with code `130` instead
  of `0`. Scripts and automation that test the exit status should treat `130` as user interruption rather than
  success.

### Fixed

#### sync / ls / clean

- [Breaking change] A run interrupted by Ctrl+C (SIGINT) now exits with code 130 (128 + SIGINT, the conventional
  shell encoding for termination by signal) instead of 0. Previously an interrupted run was indistinguishable from
  a successful one by its exit code, and the interruption exit code was inconsistent across subcommands (`cp` and
  `mv` already exited 130). The Ctrl+C handler records the signal in a process-global flag before cancelling the
  pipeline, and the frontend checks it once the pipeline has stopped — the interruption takes precedence over
  whatever the forced shutdown recorded, so in particular a Ctrl+C'd `clean` that had already collected genuine
  deletion errors no longer exits 0. Uninterrupted runs are unchanged, and so is `clean`'s confirmation prompt,
  where Ctrl+C still terminates the process immediately through the default OS handler. 

#### batch-run

- A Ctrl+C'd `sync`, `ls`, or `clean` line inside a batch is now bucketed `skipped (exit 130)` — matching `cp` and
  `mv` — where it was previously bucketed `succeeded`. The engines return the interruption code instead of exiting
  the process, so batch-run itself keeps running exactly as before; the batch exit code follows the existing
  severity ranking, under which an interrupted batch with no other failures exits 130.

### Underlying libraries

```toml
s3sync = "=1.61.2"
s3util-rs = "=1.9.2"
s3rm-rs = "=1.5.2"
s3ls-rs = "=1.2.2"
```

## [1.7.2] - 2026-08-04

Bug-fix release. Writing to a pipe whose reader has already exited crashed s7cmd on several output paths — piping to
`head 1` (where `head` treats `1` as a file name, fails to open it, and never reads its input), `head -1` when the
output is larger than the pipe buffer, `grep -q`, or a pager closed early. This release makes every such path
pipe-safe, in s7cmd's own code and in the four underlying libraries, which shipped the same fixes and are updated
here. There are no changes to any subcommand's interface or to what is printed when the pipe stays open; upgrading
requires nothing beyond installing the new binary.

### Fixed

#### All subcommands

- Generating a shell completion script into a closed pipe (`s7cmd --auto-complete-shell bash | head 1`) panicked
  inside clap_complete with `failed to write completion file: ... Broken pipe`. The script is now rendered to an
  in-memory buffer and written pipe-safely: a closed pipe is treated as the normal end of a pipeline and the command
  exits 0 — note that under `set -o pipefail` such pipelines now succeed where the panic previously failed them. Any
  other stdout write failure (e.g. disk full on a redirect) now exits 1 with an error message instead of panicking.

#### Util subcommands that print a report

- Piping report output to a consumer that stops reading before the output ends no longer panics with
  `failed printing to stdout: Broken pipe (os error 32)`. Every subcommand that prints a JSON report or a URL to
  stdout was affected: the `get-bucket-*` family, `get-public-access-block`, `head-bucket`, `head-object`,
  `get-object-tagging`, `list-object-annotations`, `get-object-annotation`, `put-object-annotation`, and `presign`.
  A closed pipe is now treated as the normal end of a pipeline: the S3 operation has already completed, so the
  command exits 0; any other stdout write failure exits 1 with an error message. `cp` streaming an object to stdout
  (and `get-object-annotation` writing its payload to `-`) is unchanged: a download truncated by a vanished reader
  is still reported as a failure, because there the bytes are the object itself, not a report about a completed
  operation. `ls` and the tracing output already handled closed pipes and are unchanged.

#### cp / mv

- The transfer result lines printed to stderr on completion (`-> s3://...` and `Transferred: ...`) panicked when
  stderr was a closed pipe (e.g. `2>&1 | head`). They are now written best-effort, matching the tracing output,
  which already ignored a closed stderr.

#### batch-run

- The end-of-run summary (both the human-readable line and the `--json-tracing` JSON object) panicked when stderr
  was a closed pipe, turning an otherwise fully successful run into exit 101 (abnormal termination) after every
  line had already executed. The summary is now written best-effort.

### Changed

- s3sync `v1.61.1 -> v1.61.2`
- s3util-rs `v1.9.1 -> v1.9.2`
- s3rm-rs `v1.5.1 -> v1.5.2`
- s3ls-rs `v1.2.1 -> v1.2.2`

These are the upstream releases of the same broken-pipe fixes; s7cmd vendors its command frontends, so the fixes
are ported into the vendored code as well.

### Underlying libraries

```toml
s3sync = "=1.61.2"
s3util-rs = "=1.9.2"
s3rm-rs = "=1.5.2"
s3ls-rs = "=1.2.2"
```

## [1.7.1] - 2026-07-26

Dependency-refresh release. All four underlying libraries are updated to their 2026-07-26 releases, which are
themselves dependency updates carrying a newer AWS SDK for Rust. There are no behavior changes in s7cmd's own code
and no changes to any subcommand's interface or output; upgrading requires nothing beyond installing the new binary.

### Changed

- s3sync `v1.61.0 -> v1.61.1`
- s3util-rs `v1.9.0 -> v1.9.1`
- s3rm-rs `v1.5.0 -> v1.5.1`
- s3ls-rs `v1.2.0 -> v1.2.1`
- aws-sdk-s3 `v1.138.1 -> v1.140.0`
- Updated other dependencies

### Underlying libraries

```toml
s3sync = "=1.61.1"
s3util-rs = "=1.9.1"
s3rm-rs = "=1.5.1"
s3ls-rs = "=1.2.1"
```

## [1.7.0] - 2026-07-25

Bug-fix release. 

### Fixed

#### All subcommands

- [Breaking change] Exported `SOURCE` / `TARGET` environment variables no longer silently supply positional arguments. The underlying
  libraries declared every positional source/target argument with clap's `env` attribute, so an unrelated exported
  variable named after a positional was parsed as if it had been passed on the command line: `TARGET=s3://bucket
  s7cmd clean` proceeded toward a real deletion pipeline against the env-named bucket, `SOURCE`/`TARGET` satisfied
  `sync` and `cp` paths, an exported `TARGET` satisfied every util subcommand's otherwise-required target (e.g.
  `head-bucket`, `get-bucket-versioning`), and `ls` listed the env-named target instead of falling back to
  bucket-listing mode. The updated libraries drop the `env` attribute from every positional argument; positionals now
  come only from the command line. Explicitly passed positionals were never affected (command-line values always took
  precedence).

#### s3util-rs

- `put-bucket-lifecycle-configuration` now accepts lifecycle `Date` values carrying an ISO 8601 numeric UTC offset
  (e.g. `2030-01-02T03:04:05+00:00`) in addition to `Z`; a non-zero offset is converted to UTC.
  `get-bucket-lifecycle-configuration` emits dates with `+00:00`, so feeding its output back into
  `put-bucket-lifecycle-configuration` failed with `invalid ISO 8601 timestamp` for any date-based rule.

### Underlying libraries

```toml
s3sync = "=1.61.0"
s3util-rs = "=1.9.0"
s3rm-rs = "=1.5.0"
s3ls-rs = "=1.2.0"
```

## [1.6.0] - 2026-07-20

Monthly update.

Security and bug-fix release. It rolls up a large batch of security and correctness fixes across s7cmd and its four
underlying libraries: the pinned libraries are updated to their 2026-07-20 releases — s3sync `v1.60.0`, s3util-rs
`v1.8.0`, s3rm-rs `v1.4.0`, s3ls-rs `v1.1.0` — and issues found in s7cmd's own CLI and `batch-run` code by a
security review are fixed as well (see **Security** and **Fixed** below). `put-bucket-lifecycle-configuration`
gains one new option (see **Added**).

**Upgrade notes:**

- **IAM policies may need updating.** `cp`, `mv`, and S3-to-stdout downloads now pin reads to the object version
  observed at the start of the transfer, and S3 authorizes version-pinned reads against `s3:GetObjectVersion`
  instead of `s3:GetObject`. On buckets that have (or ever had) versioning enabled, least-privilege policies that
  grant only the unversioned actions will start failing with `AccessDenied` — see **Changed** below for the full
  list of required actions. Unversioned buckets are unaffected.
- **Building from source** now requires Rust `1.94.1` or later. Pre-built binaries are unaffected.

### Added

#### put-bucket-lifecycle-configuration

- New `--transition-default-minimum-object-size` option (`varies_by_storage_class` or `all_storage_classes_128K`),
  sent to S3 as the request parameter of the same name. S3 accepts the value only as a request parameter — never
  inside the lifecycle configuration document — so this option is the only way to preserve a non-default setting
  across a get-edit-put roundtrip (see **Fixed › put-bucket-lifecycle-configuration** below).

### Security

- Credential-related command line options no longer display their values in `--help` output when set via environment
  variables, on every subcommand: access keys, secret access keys, session tokens (`*_ACCESS_KEY`,
  `*_SECRET_ACCESS_KEY`, `*_SESSION_TOKEN`), and SSE-C key material (`*_SSE_C_KEY`, `*_SSE_C_KEY_MD5`). Previously
  the exported secret itself appeared in the help text — and from there in terminal scrollback or captured CI logs;
  now only the environment-variable name is shown.
- `batch-run` no longer echoes inline credential values from a script line into its logs. A per-line command may
  legally carry a credential on the command line (`--*-access-key`, `--*-secret-access-key`, `--*-session-token`,
  `--*-sse-c-key`, `--*-sse-c-key-md5`), and batch-run logs each line's raw text on its per-line events — the
  `warning` / `failure` / `invalid` / `panicked` events at the default verbosity and, with `--json-tracing`, as
  machine-readable JSON. The value of every such credential option — the same set hidden from `--help` above — is
  now masked to `****` before the line is logged; a line carrying no credential is logged unchanged.

### Fixed

#### All subcommands

- An exported `AUTO_COMPLETE_SHELL` environment variable no longer alters how subcommands parse and run. The
  underlying libraries attach a hidden per-subcommand `--auto-complete-shell` option that also reads this
  environment variable, so exporting it (for example, for one of the standalone upstream tools) armed the hidden
  option on every s7cmd subcommand. That silently rewrote argument handling — `sync` / `clean` source and target
  paths defaulted to a placeholder bucket, and otherwise-required arguments on other subcommands stopped being
  required — and the command then went on to execute instead of printing shell completions the way the standalone
  tools do; destructive subcommands such as `rm` and `clean` could run where a completion listing was expected.
  Subcommands now ignore the variable entirely; shell completions are generated by s7cmd's own top-level
  `--auto-complete-shell` option, which is unchanged.

#### cp / mv

- A transfer-worker failure that cancels the internal pipeline (e.g. a failed chunk download or a stdout write error
  during parallel S3-to-stdout downloads) is now reported as a failure (exit code 1) with its error logged, instead of
  being misreported as a user cancellation (exit code 130) with the error message suppressed. A genuine SIGINT still
  exits 130.
- A local target directory spelled with a trailing forward slash (e.g. `out/`) now resolves to `out/<basename>` on
  Windows as well. Previously the `/` form was only recognized on Unix, so on Windows the literal path `out/` was
  treated as a directory and nothing was written (and `mv` would then delete the source).
- `mv` now rejects moving an S3 object onto itself (`mv s3://b/k s3://b/k`, including directory-style and bucket-only
  spellings that resolve to the source key). `mv` is copy-then-delete, so a self-move deleted the object it had just
  written on an unversioned bucket — and still exited 0. Moves between different endpoints with equal bucket/key names
  are still allowed, and an explicit `--source-version-id` (other than `null`) is still accepted as a
  version-promotion operation.

#### put-bucket-lifecycle-configuration

- A top-level `TransitionDefaultMinimumObjectSize` key in the input JSON — as produced by
  `get-bucket-lifecycle-configuration` — is now rejected with guidance instead of being silently dropped. S3 accepts
  the value only as a request parameter, never inside the configuration document, so silently dropping it reset a
  bucket configured with `varies_by_storage_class` back to S3's default of `all_storage_classes_128K` on a
  get-edit-put roundtrip. The error points to the new `--transition-default-minimum-object-size` option (see
  **Added**), which sends the value the way S3 accepts it. More generally, every `put-*` JSON input now rejects
  unknown fields (see **Changed**).

#### batch-run

- `--parallel` now rejects a value greater than 1024 at parse time (a clean exit 2) instead of accepting an
  arbitrarily large worker count, and the `--help` text documents the limit. An extremely large value previously
  panicked and aborted the whole run with exit 101 before any line executed; the bound also stops an
  oversized-but-valid value from spawning a runaway number of concurrent commands (file-descriptor / connection
  exhaustion). `--parallel 0` (use all logical CPUs) is still accepted.
- `put-bucket-versioning`, `put-bucket-accelerate-configuration`, and `put-bucket-request-payment` lines missing
  their required state flag (`--enabled` / `--suspended`, `--requester` / `--bucket-owner`) no longer terminate the
  whole batch. The validation error previously exited the batch-run process itself mid-run — bypassing
  `--continue-on-error` and `--max-errors`, skipping every later line, and suppressing the summary. It is now an
  ordinary per-line failure (same message, same exit code 2) governed by the failure-policy flags like any other
  invalid line. Standalone invocations of the three subcommands are unchanged.

#### clean

- `clean` no longer panics with exit code 101 when stderr is closed early by a downstream pipe (e.g.
  `s7cmd clean ... 2>&1 | head`). Writing the final summary or the `Deletion cancelled.` message to the closed pipe
  aborted the process even though the deletion itself had already succeeded; these writes now tolerate a closed
  stderr, matching `sync`.

### Documentation

- Added a "Security assumptions" section to the README describing the trust model s7cmd is built on, mirroring the
  equivalent sections in the s3sync / s3util-rs / s3rm-rs / s3ls-rs READMEs.

### Changed

- s3sync `v1.59.0 -> v1.60.0`, s3util-rs `v1.7.1 -> v1.8.0`, s3rm-rs `v1.3.8 -> v1.4.0`, s3ls-rs
  `v1.0.3 -> v1.1.0` (all released 2026-07-20). The user-visible changes these updates bring, beyond the sections
  above:
  - **cp/mv/s3-to-stdout reads are version-pinned (s3util-rs).** Reads of a source object are pinned to the version
    observed by the initial `HeadObject`, so an overwrite landing mid-download can no longer produce a silently
    truncated copy or interleaved stdout bytes. **IAM impact:** the pinned reads carry a `versionId`, which S3
    authorizes against `s3:GetObjectVersion` instead of `s3:GetObject`. Reading from a bucket that has (or ever had)
    versioning enabled now requires `s3:GetObjectVersion` (plus `s3:GetObjectVersionTagging` where tags are read, and
    `s3:DeleteObjectVersion` for `mv`); least-privilege policies granting only the unversioned actions will start
    failing with `AccessDenied`. Unversioned buckets are unaffected; `--source-version-id` still takes precedence.
  - **`--target-request-payer` is now actually sent** by `rm`, `head-object`, `get-object-tagging`,
    `put-object-tagging`, `restore-object`, `presign`, and the target-exists probe of `cp --skip-existing` (the flag
    was previously parsed and discarded, yielding `403` on Requester Pays buckets). The vendored runners wire the
    parameter through the updated s3util-rs API signatures. For `presign`, `x-amz-request-payer` becomes part of the
    URL's signature, so whoever fetches the URL must send `x-amz-request-payer: requester` as well.
  - **`put-*` JSON inputs reject unknown fields (s3util-rs `deny_unknown_fields`),** matching the AWS CLI's "Unknown
    parameter" behavior. A misspelled or wrongly nested key previously vanished silently and the truncated remainder
    replaced the whole bucket configuration — most severely, piping `get-public-access-block` output (wrapped in a
    `PublicAccessBlockConfiguration` key) straight back into `put-public-access-block` parsed as "all four flags
    absent" and disabled every public-access protection on the bucket.
  - **`clean` versioning semantics (s3rm-rs).** Versioning-*suspended* buckets are treated as versioned, so
    `--delete-all-versions` permanently removes historical versions and delete markers instead of degrading to
    versionless deletes; `--keep-latest-only` and `--filter-delete-marker-only` no longer reject suspended buckets.
    Delete markers are excluded from the size/content-type/metadata/tag filters (a marker has none of those
    attributes, and deleting a latest marker resurrects the object it hides); `--filter-delete-marker-only` now
    conflicts with those filters, and markers no longer abort filtered `--delete-all-versions` runs (the
    `HeadObject`/`GetObjectTagging` HTTP 405 on a marker version is skipped rather than cancelling the pipeline).
  - **`ls` listing fidelity (s3ls-rs).** `--max-parallel-listings` is enforced for deep-prefix (leaf) listings —
    previously the permit was released before each leaf scan, allowing unbounded concurrent `ListObjectsV2` calls —
    and `--rate-limit-api` enforces the exact requested rate instead of rounding down to a multiple of 10.
  - **`sync` hardening (s3sync).** `ONEZONE_IA` naming in `--storage-class`/`--annotation-storage-class`; stable
    object-version ordering when versions share a last-modified second; `--check-etag` with SSE-C uses the *source*
    SSE-C parameters for the source ETag; `--force-retry-count` applies to `HeadObject` and the object-annotation
    APIs; a `DeleteMarker` reports size 0 / no ETag instead of panicking; invalid `Expiration`/`Expires` values warn
    instead of panicking; stricter S3-path-prefix and `--metadata` validation; downloads verify on the temporary file
    before it is persisted (an object failing verification never becomes visible at the destination).
  - **`cp`/`mv` transfer verification hardening (s3util-rs).** Buffer sizes derived from server-reported part sizes
    are validated (a hostile or non-compliant endpoint can no longer force an allocation abort); two reachable panics
    in the transfer paths now return errors; single-part server-side copies no longer double-count progress bytes;
    S3-to-stdout ETag/checksum verification computes correct digests incrementally (no spurious exit-3 mismatches, no
    whole-object buffering); ETag-shape mismatches are explained (`--auto-chunksize` hint) instead of being reported
    as corruption; `--disable-additional-checksum-verify` is honoured on downloads; `--if-none-match` and the
    metadata/content-header flags now apply to stdin uploads on both the buffered and multipart paths; downloads
    verify on the temporary file before persisting, as in `sync`.
- aws-sdk-s3 `v1.137.0 -> v1.138.1`; the re-exported AWS SDK minors are synced to match the new library releases
  (aws-config `1.9`, aws-smithy-runtime-api `1.13`, aws-smithy-types `1.6`), keeping the dependency tree unified.
- MSRV `1.91.1 -> 1.94.1` (required by the updated libraries).

### Underlying libraries

```toml
s3sync = "=1.60.0"
s3util-rs = "=1.8.0"
s3rm-rs = "=1.4.0"
s3ls-rs = "=1.1.0"
```

## [1.5.0] - 2026-07-11

### Added

#### s3util-rs

- `create-bucket` gains account-level regional bucket support: pass `--bucket-namespace account-regional` together with
  `--create-bucket-configuration LocationConstraint=<region>` to create a bucket in your account's regional namespace
  (name shape `<prefix>-<accountid>-<region>-an`). The two options are required together — `account-regional` is the
  only accepted `--bucket-namespace` value and `LocationConstraint=<region>` the only accepted
  `--create-bucket-configuration` value — and when both are supplied they are sent to `CreateBucket` verbatim, bypassing
  the region/name-derived configuration.

### Fixed

#### s3util-rs

- `get-object-annotation`: an object whose additional checksum uses an algorithm s3util cannot recompute (e.g. `SHA512`,
  `MD5`, `XXHASH*`) now fails with a clear integrity error instead of panicking. The unsupported algorithm is detected
  up front and rejected rather than reaching the checksum constructor.

### Security

- Bump `crossbeam-epoch` `v0.9.18 -> v0.9.20` to address [RUSTSEC-2026-0204](https://rustsec.org/advisories/RUSTSEC-2026-0204).
  Transitive dependency (pulled in via `s3ls-rs` → `rayon`); `Cargo.lock`-only, with no public API or behavior change.

### Changed

- s3util-rs `v1.6.0 -> v1.7.1`

### Underlying libraries

```toml
s3sync = "=1.59.0"
s3util-rs = "=1.7.1"
s3rm-rs = "=1.3.8"
s3ls-rs = "=1.0.3"
```

## [1.4.0] - 2026-07-05

Adds S3 object-annotation support.

### Added

#### s3util-rs

- `get-object-annotation`, `put-object-annotation`, `delete-object-annotation`, and `list-object-annotations`
  subcommands: download, attach, delete, and list named annotation payloads on an S3 object.
  `put-object-annotation` sends a Content-MD5 and an explicit CRC64NVME and verifies the value S3 returns;
  `get-object-annotation` verifies content length, the AES256 ETag/MD5, and any additional checksum, then writes the
  payload to a file (atomic rename, re-verified on disk) or to stdout. All four surface a missing bucket, object,
  version, or annotation as exit code 4.
- `cp`/`mv` gain object-annotation sync options (`--enable-sync-object-annotations`, `--disable-check-annotation-etag`).

#### s3sync

- `sync` gains object-annotation options `--enable-sync-object-annotations`, `--disable-check-annotation-etag`,
  `--sync-latest-object-annotations`, and `--report-annotations-sync-status`. These flow through automatically from the
  updated s3sync dependency; the `--report-sync-status` summary now also reports annotation match/mismatch counts.

### Changed

- s3sync `v1.58.9 -> v1.59.0`
- s3util-rs `v1.5.3 -> v1.6.0`
- Updated other dependencies

### Underlying libraries

```toml
s3sync = "=1.59.0"
s3util-rs = "=1.6.0"
s3rm-rs = "=1.3.8"
s3ls-rs = "=1.0.3"
```

## [1.3.1] - 2026-06-27

Monthly update.

### Security

#### s3sync

- Harden directory traversal check used when saving S3 objects to local files: reject `.` and `..` path segments (
  previously only `../` and`..\` were caught), and detect separators on both `/` and `\`. Does not affect S3 access
  itself.

### Fixed

#### s3util-rs

- S3 keys are now taken verbatim from `s3://` paths. Previously `.` and `..` segments were resolved away as if the key
  were a filesystem path (e.g. `cp /etc/hosts s3://bucket/..` uploaded to key `hosts`), and `%XX` sequences were
  percent-decoded. Keys are now stored exactly as written, matching the AWS CLI.
- Downloading to a bare filename in the current directory (e.g. `cp s3://bucket/key xyz`) no longer fails with
  `parent directory does not exist: ''`. Previously this required an explicit `./xyz`; the current directory is now used
  correctly when the target has no directory component.

### Changed

- aws-sdk-s3 `v1.133.0 -> v1.137.0`
- Updated other dependencies

### Underlying libraries

```toml
s3sync = "=1.58.9"
s3util-rs = "=1.5.3"
s3rm-rs = "=1.3.8"
s3ls-rs = "=1.0.3"
```

## [1.3.0] - 2026-05-24

Monthly update.

### Added

#### s3util-rs

- `rename` subcommand: atomically rename an object within the same S3 Express One Zone directory bucket using the
  RenameObject API.
  Both source and target must be in the same bucket (name must end with --x-s3). Supports optional conditional checks.

### Changed

- aws-sdk-s3 `v1.131.0 -> v1.133.0`
- Updated other dependencies

### Underlying libraries

```toml
s3sync = "=1.58.8"
s3util-rs = "=1.5.2"
s3rm-rs = "=1.3.7"
s3ls-rs = "=1.0.2"
```

## [1.2.4] - 2026-05-17

### Changed

#### s3sync

- AWS SDK for Rust does not support the new checksums XXHash64/3/128, MD5, and SHA-512, so an error check has been added
  to prevent these from being specified as additional checksums. We plan to remove this restriction when AWS SDK for
  Rust supports these new checksums.

### Underlying libraries

```toml
s3sync = "=1.58.7"
s3util-rs = "=1.4.0"
s3rm-rs = "=1.3.6"
s3ls-rs = "=1.0.1"
```

## [1.2.3] - 2026-05-09

### Fixed

- `batch-run` now catches panics in dispatched subcommands. A
  panicked subcommand surfaces as exit code `101` with a structured
  `event = "panicked"` log entry carrying `line`, `raw`, `command`,
  and `panic` fields, is recorded as a failure in the summary, and
  counts toward `--max-errors` (so `--continue-on-error` and the
  failure-threshold flags apply to panics like any other failure).
  Previously, in the default sequential mode (`--parallel 1`), a
  panic in any subcommand crashed the entire `batch-run` process
  with no summary line, no structured log entry, and no chance to
  apply the failure-policy flags; in parallel mode the panic was
  caught but the recovery log did not identify which line panicked.

### Changed

- All build profiles now use `panic = "unwind"`. The
  `release-min-size` profile previously set `panic = "abort"`, which
  would have suppressed the new `batch-run` panic recovery for
  binaries built under that profile.

### Underlying libraries

Pinned versions are unchanged from 1.2.2:

```toml
s3sync = "=1.58.6"
s3util-rs = "=1.4.0"
s3rm-rs = "=1.3.6"
s3ls-rs = "=1.0.1"
```

## [1.2.2] - 2026-05-09

### Changed

- `batch-run` per-line and `--check-format` logs now emit their
  details as structured `tracing` fields rather than packing
  everything into the message string.

### Underlying libraries

Pinned versions are unchanged from 1.2.1:

```toml
s3sync = "=1.58.6"
s3util-rs = "=1.4.0"
s3rm-rs = "=1.3.6"
s3ls-rs = "=1.0.1"
```

## [1.2.1] - 2026-05-09

### Changed

- Documentation: clarified that **Amazon S3 is the only supported
  platform**. S3-compatible storage (MinIO, Cloudflare R2,
  Backblaze B2, Wasabi, Ceph RGW, DigitalOcean Spaces, IBM COS,
  and similar) is provided strictly **as-is**, with **absolutely
  no support or assistance**. Bug reports, questions, and
  assistance requests regarding S3-compatible storage will not be
  addressed.
- Bug report template (`.github/ISSUE_TEMPLATE/bug_report.md`):
  tightened the Storage line to state that issues regarding
  S3-compatible services will be closed automatically,
  unconditionally, and without exception; added a Region field;
  added a notice that only clear, reproducible bugs in s7cmd
  itself are accepted (no support, questions, feature requests,
  or usage help).

### Underlying libraries

Pinned versions are unchanged from 1.2.0:

```toml
s3sync = "=1.58.6"
s3util-rs = "=1.4.0"
s3rm-rs = "=1.3.6"
s3ls-rs = "=1.0.1"
```

## [1.2.0] - 2026-05-07

### Added

One new subcommand sourced from `s3util-rs` 1.4.0:

- **Pre-signed URLs** — `presign s3://<BUCKET>/<KEY> [--expires-in N]`
  generates a SigV4-signed `GetObject` URL locally and prints it to
  stdout. Default `--expires-in` is 3600 seconds; maximum is 604800
  seconds (one week). Zero, negative, non-numeric, and over-max
  values are rejected at parse time. Bucket-only paths
  (`s3://<BUCKET>` or `s3://<BUCKET>/`) and local-path targets are
  rejected post-parse and exit 1; unsupported URL schemes (e.g.
  `http://...`) are rejected by clap's value-parser and exit 2.
  presign is GET-only (no `--source-version-id`) and has no
  `--dry-run` (signing is local — no S3 API call is made), matching
  `aws s3 presign`.

### Changed

- Bumped `s3util-rs` from 1.3 to 1.4 (vendored CLI sources synced
  to match) — this is what brings `presign`.
- Top-level `--help` reorganization: `restore-object` and `presign`
  now appear inside the "Object Operations" group (right after
  `rm`); the standalone "Object Restore" / "Object Presign"
  sections were removed. Per-subcommand `--help` is unchanged.

### Underlying libraries

This release pins the following exact versions of the underlying
libraries:

```toml
s3sync = "=1.58.6"
s3util-rs = "=1.4.0"
s3rm-rs = "=1.3.6"
s3ls-rs = "=1.0.1"
```

## [1.1.0] - 2026-05-06

### Added

Nine new subcommands sourced from `s3util-rs` 1.3.0. Each one
mirrors the upstream behavior (argument names, log messages,
exit codes, output JSON shape) and respects s7cmd's
`--dry-run`, `--target-profile`, `batch-run`, and exit-code
conventions.

- **Bucket Replication** — `get-bucket-replication`,
  `put-bucket-replication`, `delete-bucket-replication` for
  managing cross-region and same-region replication rules.
  `put-bucket-replication` accepts the AWS-CLI shape
  (top-level `Role` + `Rules`) from a file path or `-` for
  stdin (file path only inside `batch-run`, matching the
  other `put-*` family).
- **Transfer Acceleration** — `get-bucket-accelerate-configuration`,
  `put-bucket-accelerate-configuration` to read and toggle S3
  Transfer Acceleration. `put-` takes mutually-exclusive
  `--enabled` / `--suspended` flags.
- **Requester Pays** — `get-bucket-request-payment`,
  `put-bucket-request-payment` for switching between owner-pays
  and requester-pays billing. `put-` takes mutually-exclusive
  `--requester` / `--bucket-owner` flags.
- **Policy Status** — `get-bucket-policy-status` to report whether
  a bucket policy makes the bucket public
  (`{"PolicyStatus": {"IsPublic": …}}`).
- **Object Restore** — `restore-object` to initiate restoration
  of S3 Glacier-class archived objects with `--days N` and
  `--tier {Standard,Bulk,Expedited}`. Supports
  `--source-version-id` for version-targeted restores.

### Changed

- Bumped `s3util-rs` from 1.2 to 1.3 (vendored CLI sources
  synced to match). This release also picks up the upstream
  bug fixes for output formatting (object-size filters and
  version-related fields in lifecycle output, encryption
  blocking rules, target grants in logging, `ChecksumSHA512` /
  `ChecksumMD5` in object metadata, replication metrics and
  RTC time containers) and accepts ISO 8601 (`YYYY-MM-DD`)
  dates in lifecycle rules.

### Underlying libraries

This release pins the following exact versions of the underlying
libraries:

```toml
s3sync = "=1.58.6"
s3util-rs = "=1.3.0"
s3rm-rs = "=1.3.6"
s3ls-rs = "=1.0.1"
```

## [1.0.0] - 2026-05-04

Initial release.

## [0.3.0] - 2026-05-03

### Added

- **`cp --skip-existing`**: when the target object or file already
  exists, skip the copy instead of overwriting. Lets you resume
  partial bulk transfers (uploads, downloads, S3-to-S3) without
  re-sending objects that already landed at the destination.
  Combine with `--dry-run` to preview which objects would be
  skipped before running for real.
- **`create-bucket --if-not-exists`**: if the bucket already exists
  (and you own it), exit 0 without re-creating. Makes provisioning
  scripts idempotent — re-running the same
  `create-bucket --if-not-exists s3://my-bucket` is safe whether
  or not the bucket is already there. When combined with
  `--tagging`, the tagging step is also skipped on the
  existing-bucket path.

### Changed

- Bumped `s3util-rs` from 1.1 to 1.2 (vendored CLI sources synced
  to match) — this is what brings the two new flags above.
- Expanded the README's *Intended Audience and Issue Tracker Scope*
  section: questions about concurrency-induced performance or
  resource exhaustion, and questions that belong with AWS, with
  the operator of an S3-compatible storage service, or with the
  operating system vendor, are now explicitly out of scope for
  the issue tracker.
- Added a note in the `batch-run` section of the README clarifying
  that while batch-run avoids per-command process startup, it
  still constructs a fresh AWS client (credential, region, HTTP
  client setup) per command — so it is not intended for
  high-throughput parallel processing of large workloads.

### Underlying libraries

This release pins the following exact versions of the underlying
libraries:

```toml
s3sync = "=1.58.6"
s3util-rs = "=1.2.0"
s3rm-rs = "=1.3.6"
s3ls-rs = "=1.0.0"
```

## [0.2.0] - 2026-05-02

### Added

- **`batch-run` subcommand** for running many s7cmd commands from a
  script file (or `-` for stdin). Choose sequential or parallel
  execution (`--parallel N`, `0` picks the CPU count) and pick how
  failures are handled:
    - default: stop on the first non-zero exit (sequential) or stop
      spawning new commands (parallel; in-flight lines finish);
    - `--continue-on-error`: run every line regardless of outcome;
    - `--continue-on-warning`: keep running past warnings (exit codes
      `3` and `4`) but still stop on true failures;
    - `--max-errors N`: stop once `N` failures have been recorded.
      Parse/validation errors (typos, unknown subcommands, bad
      arguments) count the same as runtime failures, so
      `--max-errors 5` tolerates up to 5 broken lines anywhere.

  The final exit code is the worst seen across the whole batch,
  ranked by severity (`1` > `2` > `3` > `4` > any other non-zero >
  `0`) rather than by numeric value, so an actionable error always
  wins over a SIGINT skip or a "not found". A trailing summary
  `N succeeded, N failed, N warnings, N skipped, elapsed Ts` is
  written to stderr; suppress with `--no-summary`, or pass
  `--json-tracing` to emit it as a single-line JSON object instead.
- **Live progress bar in `batch-run`** drawn on TTY stderr in
  read-all mode while the run is in progress. Suppress with
  `--no-progress` (useful inside terminal multiplexers, `script(1)`,
  or CI runners). Streaming mode, non-TTY stderr, and
  `--json-tracing` already suppress the bar.
- **`--check-format`** validates a `batch-run` script without
  executing anything. It stops at the first problematic line —
  prefixed with the script source (file path, or `stdin` for `-`)
  and the line number — and exits 1; a clean script logs `format OK`.
- **Per-line tracing in `batch-run`**: each line logs a `start` event
  and a matching outcome — `success`, `warning (exit N)`,
  `skipped (exit 130)` (Ctrl-C / SIGINT), or `failure (exit N)` —
  prefixed with the line number and the original command. `start`
  and `success` are info level (silent at the default warn level —
  pass `-v`); `warning`, `skipped`, and `failure` are visible
  without `-v`. Per-line outcome buckets: `0` → succeeded;
  `3` / `4` → warnings; `130` → skipped (never counts toward
  `--max-errors`); other non-zero → failed.
- **`--dry-run` on every state-mutating subcommand** (`cp`, `mv`,
  `rm`, `create-bucket`, `delete-bucket`, all `put-*` and
  `delete-*`). Arguments and inputs are still validated, an
  `[dry-run]` log line describes what would have happened, and the
  command exits 0 without touching AWS. Read-only commands (`get-*`,
  `head-*`, `ls`) intentionally do not accept this flag.

### Changed

- `sync`, `clean`, and `ls` no longer terminate the process on their
  own — required so a single failing line inside `batch-run` does not
  kill the rest of the batch. Behavior of standalone invocations is
  unchanged.
- Bumped `s3util-rs` from 1.0 to 1.1 (vendored CLI sources synced to
  match).

### Fixed

- Dispatching `cp` / `mv` could overflow the thread stack on
  platforms with small default stacks; the inner futures are now
  boxed so dispatch is safe regardless of stack size.

## [0.1.3] - 2026-04-29

### Changed

- Documentation updates.

## [0.1.2] - 2026-04-29

### Added

- `CHANGELOG.md` documenting changes per Keep a Changelog format.

### Changed

- Bumped `s3util-rs` to `1.0.0` and synced vendored CLI sources to upstream `4edffac`. Under `--show-progress`, the
  destination line (`-> <path>`) is now printed unconditionally on success.
- Expanded README: scope, non-goals, requirements, installation, and AI-development disclosure.

### Removed

- Dropped Windows UAC manifest infrastructure (`s7cmd.manifest`, `s7cmd.rc`, `embed-resource` build dependency).

## [0.1.1] - 2026-04-29

### Changed

- Bumped `nix` from `0.30.1` to `0.31.2`.
- Dropped `windows-11-arm` runner from CI/CD; pre-built `aarch64-pc-windows-msvc` binaries are not available while the
  `LNK1322` (Cortex-A53 erratum #843419) build failure is unresolved.

## [0.1.0] - 2026-04-29

Initial preview release.

### Added

- Object operations: `ls`, `cp`, `mv`, `rm`, `sync`, `clean`.
- Object metadata: `head-object`, `get-object-tagging`, `put-object-tagging`, `delete-object-tagging`.
- Bucket operations: `create-bucket`, `delete-bucket`, `head-bucket`.
- Bucket-level configuration subcommands: tagging, policy, versioning, lifecycle, encryption, CORS, public-access-block,
  website, logging, notification.
- Shell completion generation (`--auto-complete-shell`) for bash, elvish, fish, powershell, and zsh.
- E2E test suite covering object/bucket operations against live AWS S3.
- Pre-built binaries for Linux (x86_64, aarch64), macOS (aarch64), and Windows (x86_64).
