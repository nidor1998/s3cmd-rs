//! Process-level CLI tests for the `--dry-run` flag.
//!
//! These spawn the s7cmd binary as a subprocess and assert that
//! `--dry-run` short-circuits *before* any AWS call, exits 0, and emits
//! a `[dry-run]` log line. Because the API call is skipped, no AWS
//! credentials or network access are required — the bucket name need
//! only be syntactically valid.

use std::process::{Command, Stdio};

fn s7cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_s7cmd"))
}

fn run(cmd: &mut Command) -> (bool, String, String, Option<i32>) {
    let output = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("failed to spawn s7cmd binary");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (
        output.status.success(),
        stdout,
        stderr,
        output.status.code(),
    )
}

const FAKE_BUCKET: &str = "s3://nonexistent-bucket-for-s7cmd-dry-run-tests";

// ---------- cp / mv (CommonTransferArgs path) ----------

#[test]
fn cp_dry_run_exits_zero_and_logs_message() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "cp",
        "--dry-run",
        &format!("{FAKE_BUCKET}/src"),
        &format!("{FAKE_BUCKET}/dst"),
    ]));
    assert!(ok, "cp --dry-run must exit 0; stderr={stderr}");
    assert_eq!(code, Some(0));
    assert!(stderr.contains("[dry-run]"));
    assert!(stderr.contains("would copy"));
}

#[test]
fn mv_dry_run_exits_zero_and_logs_messages() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "mv",
        "--dry-run",
        &format!("{FAKE_BUCKET}/src"),
        &format!("{FAKE_BUCKET}/dst"),
    ]));
    assert!(ok, "mv --dry-run must exit 0; stderr={stderr}");
    assert_eq!(code, Some(0));
    assert!(stderr.contains("[dry-run]"));
    assert!(stderr.contains("would copy"));
    assert!(stderr.contains("would delete source object"));
}

// ---------- cp --skip-existing under --dry-run (no AWS — local target) ----------

#[test]
fn cp_dry_run_skip_existing_with_existing_local_target_logs_skip() {
    // Existing local target → target_exists returns true → skip branch
    // logs "[dry-run] would skip: target exists." and exits 0. The skip
    // branch returns before any AWS traffic is initiated, so this runs
    // with no credentials. Verbosity is bumped to -v so the info-level
    // skip log line is captured (default verbosity drops info events).
    let dst = tempfile::NamedTempFile::new().unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "cp",
        "-v",
        "--dry-run",
        "--skip-existing",
        &format!("{FAKE_BUCKET}/key"),
        dst.path().to_str().unwrap(),
    ]));
    assert!(
        ok,
        "cp --dry-run --skip-existing must exit 0; stderr={stderr}"
    );
    assert_eq!(code, Some(0));
    assert!(
        stderr.contains("[dry-run] would skip"),
        "missing '[dry-run] would skip' in stderr: {stderr}"
    );
}

#[test]
fn cp_dry_run_skip_existing_with_missing_local_target_logs_would_copy() {
    // Missing local target → target_exists returns false → skip branch
    // does not fire → existing dry-run "would copy" line is logged. -v
    // makes both branches' info events visible.
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("missing-target.dat");
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "cp",
        "-v",
        "--dry-run",
        "--skip-existing",
        &format!("{FAKE_BUCKET}/key"),
        dst.to_str().unwrap(),
    ]));
    assert!(
        ok,
        "cp --dry-run --skip-existing must exit 0; stderr={stderr}"
    );
    assert_eq!(code, Some(0));
    assert!(
        stderr.contains("[dry-run]"),
        "missing [dry-run] marker in stderr: {stderr}"
    );
    assert!(
        stderr.contains("would copy"),
        "missing 'would copy' in stderr: {stderr}"
    );
    assert!(
        !stderr.contains("would skip"),
        "missing-target case must not emit 'would skip': {stderr}"
    );
}

#[test]
fn cp_skip_existing_with_existing_local_target_no_dry_run_short_circuits() {
    // Same scenario as the dry-run skip test but without --dry-run: the
    // non-dry-run log ("Target exists; skipping copy.") and exit 0 are
    // expected. Confirms the skip branch fires identically off the
    // dry-run path. -v surfaces the info-level message.
    let dst = tempfile::NamedTempFile::new().unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "cp",
        "-v",
        "--skip-existing",
        &format!("{FAKE_BUCKET}/key"),
        dst.path().to_str().unwrap(),
    ]));
    assert!(ok, "cp --skip-existing must exit 0; stderr={stderr}");
    assert_eq!(code, Some(0));
    assert!(
        stderr.contains("Target exists; skipping copy."),
        "missing 'Target exists; skipping copy.' in stderr: {stderr}"
    );
}

// ---------- rm (CommonClientArgs path) ----------

#[test]
fn rm_dry_run_exits_zero_and_logs_message() {
    let (ok, _stdout, stderr, code) =
        run(s7cmd().args(["rm", "--dry-run", &format!("{FAKE_BUCKET}/key")]));
    assert!(ok, "rm --dry-run must exit 0; stderr={stderr}");
    assert_eq!(code, Some(0));
    assert!(stderr.contains("[dry-run]"));
    assert!(stderr.contains("would delete object"));
}

// ---------- create-bucket (special: 2 API calls) ----------

#[test]
fn create_bucket_dry_run_exits_zero_and_logs_message() {
    let (ok, _stdout, stderr, code) =
        run(s7cmd().args(["create-bucket", "--dry-run", FAKE_BUCKET]));
    assert!(ok, "create-bucket --dry-run must exit 0; stderr={stderr}");
    assert_eq!(code, Some(0));
    assert!(stderr.contains("[dry-run]"));
    assert!(stderr.contains("would create bucket"));
}

#[test]
fn create_bucket_dry_run_with_tagging_logs_both_lines() {
    let (ok, _stdout, stderr, _code) = run(s7cmd().args([
        "create-bucket",
        "--dry-run",
        "--tagging",
        "env=test&team=sre",
        FAKE_BUCKET,
    ]));
    assert!(ok);
    assert!(
        stderr.contains("would create bucket"),
        "missing 'would create bucket': {stderr}"
    );
    assert!(
        stderr.contains("would put bucket tagging"),
        "missing 'would put bucket tagging': {stderr}"
    );
}

#[test]
fn create_bucket_dry_run_account_regional_logs_namespace_line() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "create-bucket",
        "--dry-run",
        "--bucket-namespace",
        "account-regional",
        "--create-bucket-configuration",
        "LocationConstraint=ap-northeast-1",
        FAKE_BUCKET,
    ]));
    assert!(ok, "create-bucket --dry-run must exit 0; stderr={stderr}");
    assert_eq!(code, Some(0));
    assert!(
        stderr.contains("would create bucket"),
        "missing 'would create bucket': {stderr}"
    );
    assert!(
        stderr.contains("account-regional bucket namespace"),
        "missing account-regional namespace dry-run line: {stderr}"
    );
    assert!(
        stderr.contains("ap-northeast-1"),
        "expected the location constraint in the dry-run line: {stderr}"
    );
}

// ---------- put-* (representative: put-bucket-cors with JSON config) ----------

#[test]
fn put_bucket_cors_dry_run_exits_zero_and_logs_message() {
    let cfg = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(cfg.path(), r#"{"CORSRules":[]}"#).unwrap();
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "put-bucket-cors",
        "--dry-run",
        FAKE_BUCKET,
        cfg.path().to_str().unwrap(),
    ]));
    assert!(ok, "put-bucket-cors --dry-run must exit 0; stderr={stderr}");
    assert_eq!(code, Some(0));
    assert!(stderr.contains("[dry-run]"));
    assert!(stderr.contains("would put bucket CORS"));
}

#[test]
fn put_bucket_cors_dry_run_validates_json_before_short_circuit() {
    // Per spec: pre-flight (including JSON parsing) runs to completion. A
    // malformed JSON config must surface the parse error even under
    // --dry-run, not silently succeed.
    let cfg = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(cfg.path(), b"not valid json {").unwrap();
    let (ok, _stdout, stderr, _code) = run(s7cmd().args([
        "put-bucket-cors",
        "--dry-run",
        FAKE_BUCKET,
        cfg.path().to_str().unwrap(),
    ]));
    assert!(!ok, "malformed JSON must still fail under --dry-run");
    assert!(
        stderr.to_lowercase().contains("json") || stderr.to_lowercase().contains("parse"),
        "expected JSON parse error; got: {stderr}"
    );
}

// ---------- delete-* (representative: delete-bucket-cors) ----------

#[test]
fn delete_bucket_cors_dry_run_exits_zero_and_logs_message() {
    let (ok, _stdout, stderr, code) =
        run(s7cmd().args(["delete-bucket-cors", "--dry-run", FAKE_BUCKET]));
    assert!(ok, "stderr={stderr}");
    assert_eq!(code, Some(0));
    assert!(stderr.contains("[dry-run]"));
    assert!(stderr.contains("would delete bucket CORS"));
}

// ---------- coverage smoke tests for every remaining mutating command ----------
//
// One test per command, each shaped to exercise the dry-run short-circuit
// (the `if args.dry_run { ... }` block). Required positional arguments are
// supplied with a syntactically valid value; commands taking a JSON config
// file get the minimum body each parser accepts (verified empirically).
// All tests assert exit 0 and `[dry-run]` in stderr — a regression in any
// run_X function would surface here without needing AWS access.

const FAKE_OBJECT: &str = "s3://nonexistent-bucket-for-s7cmd-dry-run-tests/key";

fn write_json(body: &str) -> tempfile::NamedTempFile {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), body).unwrap();
    tmp
}

fn assert_dry_run(stderr: &str, ok: bool, code: Option<i32>, phrase: &str) {
    assert!(ok, "stderr: {stderr}");
    assert_eq!(code, Some(0));
    assert!(
        stderr.contains("[dry-run]"),
        "missing [dry-run] in stderr: {stderr}"
    );
    assert!(
        stderr.contains(phrase),
        "stderr missing '{phrase}': {stderr}"
    );
}

// delete-* family (8 commands beyond delete-bucket-cors)

#[test]
fn delete_bucket_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args(["delete-bucket", "--dry-run", FAKE_BUCKET]));
    assert_dry_run(&stderr, ok, code, "would delete bucket");
}

#[test]
fn delete_bucket_encryption_dry_run_smoke() {
    let (ok, _, stderr, code) =
        run(s7cmd().args(["delete-bucket-encryption", "--dry-run", FAKE_BUCKET]));
    assert_dry_run(&stderr, ok, code, "would delete bucket encryption");
}

#[test]
fn delete_bucket_lifecycle_configuration_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "delete-bucket-lifecycle-configuration",
        "--dry-run",
        FAKE_BUCKET,
    ]));
    assert_dry_run(&stderr, ok, code, "would delete bucket lifecycle");
}

#[test]
fn delete_bucket_policy_dry_run_smoke() {
    let (ok, _, stderr, code) =
        run(s7cmd().args(["delete-bucket-policy", "--dry-run", FAKE_BUCKET]));
    assert_dry_run(&stderr, ok, code, "would delete bucket policy");
}

#[test]
fn delete_bucket_tagging_dry_run_smoke() {
    let (ok, _, stderr, code) =
        run(s7cmd().args(["delete-bucket-tagging", "--dry-run", FAKE_BUCKET]));
    assert_dry_run(&stderr, ok, code, "would delete bucket tagging");
}

#[test]
fn delete_bucket_website_dry_run_smoke() {
    let (ok, _, stderr, code) =
        run(s7cmd().args(["delete-bucket-website", "--dry-run", FAKE_BUCKET]));
    assert_dry_run(&stderr, ok, code, "would delete bucket website");
}

#[test]
fn delete_object_tagging_dry_run_smoke() {
    let (ok, _, stderr, code) =
        run(s7cmd().args(["delete-object-tagging", "--dry-run", FAKE_OBJECT]));
    assert_dry_run(&stderr, ok, code, "would delete object tagging");
}

#[test]
fn delete_object_tagging_dry_run_with_version_id_includes_version_field() {
    // Exercises the `version_id = %args.source_version_id...` field of the
    // dry-run info!() — distinct from the no-version-id smoke above.
    let (ok, _, stderr, code) = run(s7cmd().args([
        "delete-object-tagging",
        "--dry-run",
        "--source-version-id",
        "abc123",
        FAKE_OBJECT,
    ]));
    assert_dry_run(&stderr, ok, code, "would delete object tagging");
    assert!(
        stderr.contains("abc123"),
        "version_id should appear in dry-run log: {stderr}"
    );
}

#[test]
fn delete_public_access_block_dry_run_smoke() {
    let (ok, _, stderr, code) =
        run(s7cmd().args(["delete-public-access-block", "--dry-run", FAKE_BUCKET]));
    assert_dry_run(&stderr, ok, code, "would delete public access block");
}

// put-* family (10 commands beyond put-bucket-cors)

#[test]
fn put_bucket_encryption_dry_run_smoke() {
    let cfg = write_json(r#"{"Rules":[]}"#);
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-encryption",
        "--dry-run",
        FAKE_BUCKET,
        cfg.path().to_str().unwrap(),
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket encryption");
}

#[test]
fn put_bucket_lifecycle_configuration_dry_run_smoke() {
    let cfg = write_json(r#"{"Rules":[]}"#);
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-lifecycle-configuration",
        "--dry-run",
        FAKE_BUCKET,
        cfg.path().to_str().unwrap(),
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket lifecycle");
}

#[test]
fn put_bucket_logging_dry_run_smoke() {
    let cfg = write_json("{}");
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-logging",
        "--dry-run",
        FAKE_BUCKET,
        cfg.path().to_str().unwrap(),
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket logging");
}

#[test]
fn put_bucket_notification_configuration_dry_run_smoke() {
    let cfg = write_json("{}");
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-notification-configuration",
        "--dry-run",
        FAKE_BUCKET,
        cfg.path().to_str().unwrap(),
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket notification");
}

#[test]
fn put_bucket_policy_dry_run_smoke() {
    // put-bucket-policy passes the body verbatim — no JSON validation in
    // s7cmd — so any UTF-8 file body is accepted at the parse stage.
    let cfg = write_json(r#"{"Version":"2012-10-17","Statement":[]}"#);
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-policy",
        "--dry-run",
        FAKE_BUCKET,
        cfg.path().to_str().unwrap(),
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket policy");
}

#[test]
fn put_bucket_tagging_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-tagging",
        "--dry-run",
        FAKE_BUCKET,
        "--tagging",
        "env=test",
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket tagging");
}

#[test]
fn put_bucket_versioning_enabled_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-versioning",
        "--dry-run",
        "--enabled",
        FAKE_BUCKET,
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket versioning");
    assert!(stderr.contains("Enabled"), "stderr: {stderr}");
}

#[test]
fn put_bucket_versioning_suspended_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-versioning",
        "--dry-run",
        "--suspended",
        FAKE_BUCKET,
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket versioning");
    assert!(stderr.contains("Suspended"), "stderr: {stderr}");
}

#[test]
fn put_bucket_website_dry_run_smoke() {
    let cfg = write_json("{}");
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-website",
        "--dry-run",
        FAKE_BUCKET,
        cfg.path().to_str().unwrap(),
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket website");
}

#[test]
fn put_object_tagging_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-object-tagging",
        "--dry-run",
        FAKE_OBJECT,
        "--tagging",
        "env=test",
    ]));
    assert_dry_run(&stderr, ok, code, "would put object tagging");
}

#[test]
fn put_object_tagging_dry_run_with_version_id_includes_version_field() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-object-tagging",
        "--dry-run",
        FAKE_OBJECT,
        "--tagging",
        "env=test",
        "--source-version-id",
        "ver1",
    ]));
    assert_dry_run(&stderr, ok, code, "would put object tagging");
    assert!(
        stderr.contains("ver1"),
        "version_id should appear in dry-run log: {stderr}"
    );
}

#[test]
fn put_public_access_block_dry_run_smoke() {
    let cfg = write_json("{}");
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-public-access-block",
        "--dry-run",
        FAKE_BUCKET,
        cfg.path().to_str().unwrap(),
    ]));
    assert_dry_run(&stderr, ok, code, "would put public access block");
}

// rm with --source-version-id covers the version_id field of the dry-run log
#[test]
fn rm_dry_run_with_version_id_includes_version_field() {
    let (ok, _, stderr, code) =
        run(s7cmd().args(["rm", "--dry-run", "--source-version-id", "v42", FAKE_OBJECT]));
    assert_dry_run(&stderr, ok, code, "would delete object");
    assert!(
        stderr.contains("v42"),
        "version_id must appear in dry-run log: {stderr}"
    );
}

// v1.3.0 mutating subcommands (s3util-rs 1.3.0)

#[test]
fn delete_bucket_replication_dry_run_smoke() {
    let (ok, _, stderr, code) =
        run(s7cmd().args(["delete-bucket-replication", "--dry-run", FAKE_BUCKET]));
    assert_dry_run(&stderr, ok, code, "would delete bucket replication");
}

#[test]
fn put_bucket_replication_dry_run_smoke() {
    let cfg = write_json(r#"{"Role":"arn:aws:iam::000000000000:role/r","Rules":[]}"#);
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-replication",
        "--dry-run",
        FAKE_BUCKET,
        cfg.path().to_str().unwrap(),
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket replication");
}

#[test]
fn put_bucket_accelerate_configuration_enabled_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-accelerate-configuration",
        "--dry-run",
        "--enabled",
        FAKE_BUCKET,
    ]));
    assert_dry_run(
        &stderr,
        ok,
        code,
        "would put bucket accelerate configuration",
    );
    assert!(stderr.contains("Enabled"), "stderr: {stderr}");
}

#[test]
fn put_bucket_accelerate_configuration_suspended_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-accelerate-configuration",
        "--dry-run",
        "--suspended",
        FAKE_BUCKET,
    ]));
    assert_dry_run(
        &stderr,
        ok,
        code,
        "would put bucket accelerate configuration",
    );
    assert!(stderr.contains("Suspended"), "stderr: {stderr}");
}

#[test]
fn put_bucket_request_payment_requester_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-request-payment",
        "--dry-run",
        "--requester",
        FAKE_BUCKET,
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket request payment");
    assert!(stderr.contains("Requester"), "stderr: {stderr}");
}

#[test]
fn put_bucket_request_payment_bucket_owner_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-bucket-request-payment",
        "--dry-run",
        "--bucket-owner",
        FAKE_BUCKET,
    ]));
    assert_dry_run(&stderr, ok, code, "would put bucket request payment");
    assert!(stderr.contains("BucketOwner"), "stderr: {stderr}");
}

#[test]
fn restore_object_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "restore-object",
        "--dry-run",
        "--days",
        "7",
        "--tier",
        "Standard",
        FAKE_OBJECT,
    ]));
    assert_dry_run(&stderr, ok, code, "would restore object");
}

#[test]
fn restore_object_dry_run_with_version_id_includes_version_field() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "restore-object",
        "--dry-run",
        "--source-version-id",
        "v9",
        FAKE_OBJECT,
    ]));
    assert_dry_run(&stderr, ok, code, "would restore object");
    assert!(
        stderr.contains("v9"),
        "version_id must appear in dry-run log: {stderr}"
    );
}

// object annotation (s3util-rs 1.6.0) — put/delete are mutating

#[test]
fn put_object_annotation_dry_run_smoke() {
    // put-object-annotation reads the payload file before the dry-run
    // short-circuit, so seed a real file with the payload bytes.
    let payload = write_json("annotation payload");
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-object-annotation",
        "--dry-run",
        "--annotation-name",
        "note",
        "--annotation-payload",
        payload.path().to_str().unwrap(),
        FAKE_OBJECT,
    ]));
    assert_dry_run(&stderr, ok, code, "would put object annotation");
}

#[test]
fn put_object_annotation_dry_run_with_version_id_includes_version_field() {
    let payload = write_json("annotation payload");
    let (ok, _, stderr, code) = run(s7cmd().args([
        "put-object-annotation",
        "--dry-run",
        "--annotation-name",
        "note",
        "--annotation-payload",
        payload.path().to_str().unwrap(),
        "--target-version-id",
        "ver1",
        FAKE_OBJECT,
    ]));
    assert_dry_run(&stderr, ok, code, "would put object annotation");
    assert!(
        stderr.contains("ver1"),
        "version_id should appear in dry-run log: {stderr}"
    );
}

#[test]
fn delete_object_annotation_dry_run_smoke() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "delete-object-annotation",
        "--dry-run",
        "--annotation-name",
        "note",
        FAKE_OBJECT,
    ]));
    assert_dry_run(&stderr, ok, code, "would delete object annotation");
}

#[test]
fn delete_object_annotation_dry_run_with_version_id_includes_version_field() {
    let (ok, _, stderr, code) = run(s7cmd().args([
        "delete-object-annotation",
        "--dry-run",
        "--annotation-name",
        "note",
        "--target-version-id",
        "abc123",
        FAKE_OBJECT,
    ]));
    assert_dry_run(&stderr, ok, code, "would delete object annotation");
    assert!(
        stderr.contains("abc123"),
        "version_id should appear in dry-run log: {stderr}"
    );
}

// ---------- read-only commands must NOT expose --dry-run ----------

#[test]
fn get_bucket_cors_help_does_not_expose_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["get-bucket-cors", "--help"]));
    assert!(ok);
    assert!(
        !stdout.contains("--dry-run"),
        "get-* must not expose --dry-run; help: {stdout}"
    );
}

#[test]
fn head_bucket_help_does_not_expose_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["head-bucket", "--help"]));
    assert!(ok);
    assert!(
        !stdout.contains("--dry-run"),
        "head-* must not expose --dry-run; help: {stdout}"
    );
}

#[test]
fn head_object_help_does_not_expose_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["head-object", "--help"]));
    assert!(ok);
    assert!(!stdout.contains("--dry-run"));
}

#[test]
fn get_object_annotation_help_does_not_expose_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["get-object-annotation", "--help"]));
    assert!(ok);
    assert!(
        !stdout.contains("--dry-run"),
        "read-only get-object-annotation must not expose --dry-run; help: {stdout}"
    );
}

#[test]
fn list_object_annotations_help_does_not_expose_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["list-object-annotations", "--help"]));
    assert!(ok);
    assert!(
        !stdout.contains("--dry-run"),
        "read-only list-object-annotations must not expose --dry-run; help: {stdout}"
    );
}

// ---------- mutating commands DO expose --dry-run ----------

#[test]
fn cp_help_lists_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["cp", "--help"]));
    assert!(ok);
    assert!(stdout.contains("--dry-run"), "cp --help missing --dry-run");
}

#[test]
fn cp_help_lists_skip_existing() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["cp", "--help"]));
    assert!(ok);
    assert!(
        stdout.contains("--skip-existing"),
        "cp --help missing --skip-existing"
    );
}

#[test]
fn create_bucket_help_lists_if_not_exists() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["create-bucket", "--help"]));
    assert!(ok);
    assert!(
        stdout.contains("--if-not-exists"),
        "create-bucket --help missing --if-not-exists"
    );
}

#[test]
fn rm_help_lists_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["rm", "--help"]));
    assert!(ok);
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn delete_bucket_help_lists_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["delete-bucket", "--help"]));
    assert!(ok);
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn put_object_annotation_help_lists_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["put-object-annotation", "--help"]));
    assert!(ok);
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn delete_object_annotation_help_lists_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["delete-object-annotation", "--help"]));
    assert!(ok);
    assert!(stdout.contains("--dry-run"));
}

// v1.3.0 read-only subcommands must NOT expose --dry-run

#[test]
fn get_bucket_replication_help_does_not_expose_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["get-bucket-replication", "--help"]));
    assert!(ok);
    assert!(
        !stdout.contains("--dry-run"),
        "get-* must not expose --dry-run; help: {stdout}"
    );
}

#[test]
fn get_bucket_accelerate_configuration_help_does_not_expose_dry_run() {
    let (ok, stdout, _stderr, _code) =
        run(s7cmd().args(["get-bucket-accelerate-configuration", "--help"]));
    assert!(ok);
    assert!(
        !stdout.contains("--dry-run"),
        "get-* must not expose --dry-run; help: {stdout}"
    );
}

#[test]
fn get_bucket_request_payment_help_does_not_expose_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["get-bucket-request-payment", "--help"]));
    assert!(ok);
    assert!(!stdout.contains("--dry-run"));
}

#[test]
fn get_bucket_policy_status_help_does_not_expose_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["get-bucket-policy-status", "--help"]));
    assert!(ok);
    assert!(!stdout.contains("--dry-run"));
}

// v1.3.0 mutating subcommands DO expose --dry-run

#[test]
fn delete_bucket_replication_help_lists_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["delete-bucket-replication", "--help"]));
    assert!(ok);
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn put_bucket_replication_help_lists_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["put-bucket-replication", "--help"]));
    assert!(ok);
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn put_bucket_accelerate_configuration_help_lists_dry_run() {
    let (ok, stdout, _stderr, _code) =
        run(s7cmd().args(["put-bucket-accelerate-configuration", "--help"]));
    assert!(ok);
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn put_bucket_request_payment_help_lists_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["put-bucket-request-payment", "--help"]));
    assert!(ok);
    assert!(stdout.contains("--dry-run"));
}

#[test]
fn restore_object_help_lists_dry_run() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["restore-object", "--help"]));
    assert!(ok);
    assert!(stdout.contains("--dry-run"));
}
