//! End-to-end tests for the `--dry-run` flag.
//!
//! For each representative mutating command, these tests:
//!   1. Create a real bucket on AWS (via the `s7cmd-e2e-test` profile).
//!   2. Set up any pre-existing state the command would otherwise mutate.
//!   3. Invoke `s7cmd <cmd> --dry-run`.
//!   4. Assert exit 0 and `[dry-run]` in stderr.
//!   5. Verify the AWS-side state is **unchanged** (the central guarantee
//!      of `--dry-run`: the API call must not have been issued).
//!   6. Tear down the bucket.
//!
//! Run with:
//! ```sh
//! RUSTFLAGS="--cfg e2e_test" cargo test --test e2e_dry_run -- --test-threads=1
//! ```
//!
//! Requires the `s7cmd-e2e-test` AWS profile to be configured.

#![cfg(e2e_test)]

mod common;

use common::{REGION, TestHelper, create_temp_dir, create_test_file, generate_bucket_name};

use std::process::{Command, Stdio};

fn run_s7cmd(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_s7cmd"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn s7cmd")
}

fn assert_dry_run_success(output: &std::process::Output, expected_phrase: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "dry-run must exit 0; stderr: {stderr}"
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(
        stderr.contains("[dry-run]"),
        "stderr must contain [dry-run] prefix; got: {stderr}"
    );
    assert!(
        stderr.contains(expected_phrase),
        "stderr must contain '{expected_phrase}'; got: {stderr}"
    );
}

fn sample_cors_json() -> &'static str {
    r#"{
      "CORSRules": [
        {
          "AllowedMethods": ["GET"],
          "AllowedOrigins": ["*"]
        }
      ]
    }"#
}

// -----------------------------------------------------------------
// cp dry-run: target object must NOT be created
// -----------------------------------------------------------------

#[tokio::test]
async fn cp_dry_run_does_not_create_target_object() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let src_file = create_test_file(&tmp_dir, "cp-dry-run-src.txt", b"hello");

    let target_key = "dry-run-cp-target.txt";
    let target_arg = format!("s3://{bucket}/{target_key}");

    // Sanity: target should not pre-exist.
    assert!(!helper.is_object_exist(&bucket, target_key, None).await);

    let output = run_s7cmd(&[
        "cp",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        src_file.to_str().unwrap(),
        &target_arg,
    ]);
    assert_dry_run_success(&output, "would copy");

    let exists_after = helper.is_object_exist(&bucket, target_key, None).await;
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        !exists_after,
        "cp --dry-run must NOT create the target object"
    );
}

// -----------------------------------------------------------------
// mv dry-run: source must remain, target must NOT be created
// -----------------------------------------------------------------

#[tokio::test]
async fn mv_dry_run_leaves_source_and_target_unchanged() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let source_key = "dry-run-mv-src.txt";
    let target_key = "dry-run-mv-dst.txt";
    helper
        .put_object(&bucket, source_key, b"keep me alive".to_vec())
        .await;
    assert!(helper.is_object_exist(&bucket, source_key, None).await);
    assert!(!helper.is_object_exist(&bucket, target_key, None).await);

    let source_arg = format!("s3://{bucket}/{source_key}");
    let target_arg = format!("s3://{bucket}/{target_key}");
    let output = run_s7cmd(&[
        "mv",
        "--dry-run",
        "--source-profile",
        "s7cmd-e2e-test",
        "--target-profile",
        "s7cmd-e2e-test",
        &source_arg,
        &target_arg,
    ]);
    assert_dry_run_success(&output, "would copy");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("would delete source object"),
        "mv --dry-run must log the source-delete intent; stderr: {stderr}"
    );

    let source_still_there = helper.is_object_exist(&bucket, source_key, None).await;
    let target_was_created = helper.is_object_exist(&bucket, target_key, None).await;
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        source_still_there,
        "mv --dry-run must NOT delete the source"
    );
    assert!(
        !target_was_created,
        "mv --dry-run must NOT create the target"
    );
}

// -----------------------------------------------------------------
// cp / mv dry-run with --enable-sync-object-annotations (s3util-rs 1.6.0)
//
// A real S3-to-S3 run copies the source object's annotations, so a faithful
// --dry-run must LOG each annotation it would copy — while still mutating
// nothing. These guard the vendored runner actually calling
// log_dry_run_annotation_sync from its dry-run branch; the short-circuit
// previously returned a placeholder before the S3 source was ever built, so
// the annotation lines were silently dropped and a real run copied more than
// the dry-run reported.
// -----------------------------------------------------------------

#[tokio::test]
async fn cp_dry_run_enable_sync_object_annotations_logs_would_copy_annotation() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let source_key = "dry-run-annotation-cp-src.txt";
    let target_key = "dry-run-annotation-cp-dst.txt";
    let annotation_name = "review-status";

    // Source object plus one annotation a real run would copy.
    helper
        .put_object(&bucket, source_key, b"annotated body".to_vec())
        .await;
    helper
        .put_object_annotation(&bucket, source_key, None, annotation_name, b"approved")
        .await;

    let source_arg = format!("s3://{bucket}/{source_key}");
    let target_arg = format!("s3://{bucket}/{target_key}");
    let output = run_s7cmd(&[
        "cp",
        "--dry-run",
        "--enable-sync-object-annotations",
        "--source-profile",
        "s7cmd-e2e-test",
        "--target-profile",
        "s7cmd-e2e-test",
        &source_arg,
        &target_arg,
    ]);
    assert_dry_run_success(&output, "would copy");

    // The 1.6.0 dry-run must additionally enumerate each annotation a real
    // run would copy — the behavior the vendored runner was missing.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("would copy object annotation"),
        "cp --dry-run --enable-sync-object-annotations must log the annotation \
         it would copy; stderr: {stderr}"
    );
    assert!(
        stderr.contains(annotation_name),
        "the annotation name must appear in the dry-run annotation log; stderr: {stderr}"
    );

    // Central dry-run guarantee: nothing is mutated. No target object means
    // no target annotation either.
    let target_created = helper.is_object_exist(&bucket, target_key, None).await;
    let target_annotation_created = helper
        .is_object_annotation_exist(&bucket, target_key, None, annotation_name)
        .await;
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        !target_created,
        "cp --dry-run must NOT create the target object"
    );
    assert!(
        !target_annotation_created,
        "cp --dry-run must NOT create the target annotation"
    );
}

#[tokio::test]
async fn mv_dry_run_enable_sync_object_annotations_logs_and_preserves_source() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let source_key = "dry-run-annotation-mv-src.txt";
    let target_key = "dry-run-annotation-mv-dst.txt";
    let annotation_name = "retention";

    helper
        .put_object(&bucket, source_key, b"annotated body".to_vec())
        .await;
    helper
        .put_object_annotation(&bucket, source_key, None, annotation_name, b"30d")
        .await;

    let source_arg = format!("s3://{bucket}/{source_key}");
    let target_arg = format!("s3://{bucket}/{target_key}");
    let output = run_s7cmd(&[
        "mv",
        "--dry-run",
        "--enable-sync-object-annotations",
        "--source-profile",
        "s7cmd-e2e-test",
        "--target-profile",
        "s7cmd-e2e-test",
        &source_arg,
        &target_arg,
    ]);
    assert_dry_run_success(&output, "would copy");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("would copy object annotation"),
        "mv --dry-run --enable-sync-object-annotations must log the annotation \
         it would copy; stderr: {stderr}"
    );
    assert!(
        stderr.contains(annotation_name),
        "the annotation name must appear in the dry-run annotation log; stderr: {stderr}"
    );
    assert!(
        stderr.contains("would delete source object"),
        "mv --dry-run must still log the source-delete intent; stderr: {stderr}"
    );

    // Nothing mutated: source (and its annotation) stays, target absent.
    let source_still_there = helper.is_object_exist(&bucket, source_key, None).await;
    let source_annotation_still_there = helper
        .is_object_annotation_exist(&bucket, source_key, None, annotation_name)
        .await;
    let target_created = helper.is_object_exist(&bucket, target_key, None).await;
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        source_still_there,
        "mv --dry-run must NOT delete the source"
    );
    assert!(
        source_annotation_still_there,
        "mv --dry-run must NOT remove the source annotation"
    );
    assert!(!target_created, "mv --dry-run must NOT create the target");
}

// -----------------------------------------------------------------
// rm dry-run: object must still exist
// -----------------------------------------------------------------

#[tokio::test]
async fn rm_dry_run_does_not_delete_object() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let key = "dry-run-rm-target.txt";
    helper
        .put_object(&bucket, key, b"keep me alive".to_vec())
        .await;
    assert!(helper.is_object_exist(&bucket, key, None).await);

    let object_arg = format!("s3://{bucket}/{key}");
    let output = run_s7cmd(&[
        "rm",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &object_arg,
    ]);
    assert_dry_run_success(&output, "would delete object");

    let still_exists = helper.is_object_exist(&bucket, key, None).await;
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(still_exists, "rm --dry-run must NOT delete the object");
}

// -----------------------------------------------------------------
// create-bucket dry-run: bucket must NOT exist after
// -----------------------------------------------------------------

#[tokio::test]
async fn create_bucket_dry_run_does_not_create_bucket() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    let bucket_arg = format!("s3://{bucket}");

    // Sanity: bucket should not pre-exist.
    assert!(!helper.is_bucket_exist(&bucket).await);

    let output = run_s7cmd(&[
        "create-bucket",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        "--target-region",
        REGION,
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would create bucket");

    let exists_after = helper.is_bucket_exist(&bucket).await;
    if exists_after {
        // Defensive: clean up if the dry-run failed our contract.
        helper.delete_bucket_with_cascade(&bucket).await;
    }

    assert!(
        !exists_after,
        "create-bucket --dry-run must NOT create the bucket"
    );
}

// -----------------------------------------------------------------
// delete-bucket dry-run: bucket must still exist
// -----------------------------------------------------------------

#[tokio::test]
async fn delete_bucket_dry_run_does_not_delete_bucket() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "delete-bucket",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would delete bucket");

    let still_exists = helper.is_bucket_exist(&bucket).await;
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        still_exists,
        "delete-bucket --dry-run must NOT delete the bucket"
    );
}

// -----------------------------------------------------------------
// put-bucket-cors dry-run: CORS must remain unset
// -----------------------------------------------------------------

#[tokio::test]
async fn put_bucket_cors_dry_run_does_not_set_cors() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let cfg_file = create_test_file(&tmp_dir, "cors.json", sample_cors_json().as_bytes());

    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-cors",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg_file.to_str().unwrap(),
    ]);
    assert_dry_run_success(&output, "would put bucket CORS");

    // Verify via s7cmd get-bucket-cors: a fresh bucket with no CORS set
    // returns NoSuchCORSConfiguration (exit 4 in s7cmd's mapping). If the
    // dry-run had actually run PutBucketCors, get would succeed with the
    // configuration — that's the failure we're guarding against.
    let get_out = run_s7cmd(&[
        "get-bucket-cors",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let get_succeeded = get_out.status.success();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        !get_succeeded,
        "put-bucket-cors --dry-run must NOT have configured CORS; \
         get-bucket-cors stdout: {}",
        String::from_utf8_lossy(&get_out.stdout)
    );
}

// -----------------------------------------------------------------
// delete-bucket-cors dry-run: CORS must remain set
// -----------------------------------------------------------------

#[tokio::test]
async fn delete_bucket_cors_dry_run_does_not_delete_cors() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let cfg_file = create_test_file(&tmp_dir, "cors.json", sample_cors_json().as_bytes());

    let bucket_arg = format!("s3://{bucket}");

    // Set CORS via s7cmd (real put, no dry-run) so we have something to
    // *not* delete in the next step.
    let setup = run_s7cmd(&[
        "put-bucket-cors",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg_file.to_str().unwrap(),
    ]);
    assert!(
        setup.status.success(),
        "setup put-bucket-cors must succeed; stderr: {}",
        String::from_utf8_lossy(&setup.stderr)
    );

    // Dry-run delete: must NOT remove the CORS configuration.
    let output = run_s7cmd(&[
        "delete-bucket-cors",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would delete bucket CORS");

    // Verify: get-bucket-cors should still succeed and the body must
    // contain the AllowedMethod we supplied at setup ("GET"). A weaker
    // "get_succeeded" check would miss a buggy delete that replaced
    // the rule with a different one — checking the actual content
    // catches that.
    let get_out = run_s7cmd(&[
        "get-bucket-cors",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        get_out.status.success(),
        "get-bucket-cors must succeed after dry-run delete"
    );
    assert!(
        stdout.contains(r#""AllowedMethods""#) && stdout.contains("\"GET\""),
        "delete-bucket-cors --dry-run must preserve the original rule \
         (AllowedMethods: [\"GET\"]); get stdout: {stdout}"
    );
}

// -----------------------------------------------------------------
// put-bucket-tagging dry-run: tags must NOT be applied
// -----------------------------------------------------------------

#[tokio::test]
async fn put_bucket_tagging_dry_run_does_not_set_tags() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-tagging",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        "--tagging",
        "env=test&team=sre",
    ]);
    assert_dry_run_success(&output, "would put bucket tagging");

    // get-bucket-tagging on a bucket with no tags returns NoSuchTagSet
    // (s7cmd maps this to exit 4). If the dry-run had actually applied
    // tags, get would succeed — that's the regression to catch.
    let get_out = run_s7cmd(&[
        "get-bucket-tagging",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let get_succeeded = get_out.status.success();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        !get_succeeded,
        "put-bucket-tagging --dry-run must NOT have applied tags; \
         get-bucket-tagging stdout: {}",
        String::from_utf8_lossy(&get_out.stdout)
    );
}

// =================================================================
// The remaining mutating commands. Each test follows the same shape:
//   - For put-*: bucket starts without the resource; dry-run must
//     leave it absent (s7cmd get-* exit code != 0).
//   - For delete-*: pre-set the resource via real s7cmd put; dry-run
//     must leave it present (s7cmd get-* exit code == 0).
// The setup commands are real (no --dry-run) so we have something
// concrete to *not* mutate.
// =================================================================

fn sample_encryption_json() -> &'static str {
    r#"{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"AES256"}}]}"#
}

fn sample_lifecycle_json() -> &'static str {
    r#"{"Rules":[{"ID":"r1","Status":"Enabled","Filter":{},"Expiration":{"Days":30}}]}"#
}

fn sample_website_json() -> &'static str {
    r#"{"IndexDocument":{"Suffix":"index.html"}}"#
}

fn sample_pab_json() -> &'static str {
    r#"{"BlockPublicAcls":true,"IgnorePublicAcls":true,"BlockPublicPolicy":true,"RestrictPublicBuckets":true}"#
}

// -----------------------------------------------------------------
// put-bucket-encryption / delete-bucket-encryption
// -----------------------------------------------------------------

#[tokio::test]
async fn put_bucket_encryption_dry_run_does_not_set_encryption() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let cfg = create_test_file(&tmp_dir, "enc.json", sample_encryption_json().as_bytes());
    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-encryption",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert_dry_run_success(&output, "would put bucket encryption");

    // Buckets *do* have a default encryption setting that succeeds on
    // get even after creation (SSE-S3 default), so we instead confirm
    // the configured algorithm was not overridden by parsing the
    // returned JSON. The dry-run uses AES256 here, which would also
    // be the default — switch to an aws:kms shape so the assertion
    // is meaningful.
    let kms_cfg = create_test_file(
        &tmp_dir,
        "enc-kms.json",
        br#"{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"aws:kms"}}]}"#,
    );
    // Re-dry-run with the KMS shape to exercise the same code path.
    let kms_output = run_s7cmd(&[
        "put-bucket-encryption",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        kms_cfg.to_str().unwrap(),
    ]);
    assert_dry_run_success(&kms_output, "would put bucket encryption");

    let get_out = run_s7cmd(&[
        "get-bucket-encryption",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    // Neither dry-run should have reached PutBucketEncryption: the
    // returned config must not show aws:kms.
    assert!(
        !stdout.contains("aws:kms"),
        "put-bucket-encryption --dry-run must NOT have applied KMS; \
         get-bucket-encryption stdout: {stdout}"
    );
}

#[tokio::test]
async fn delete_bucket_encryption_dry_run_does_not_delete() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let cfg = create_test_file(
        &tmp_dir,
        "enc.json",
        br#"{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"aws:kms"}}]}"#,
    );
    let bucket_arg = format!("s3://{bucket}");

    // Real put (no dry-run) to establish state we can detect.
    let setup = run_s7cmd(&[
        "put-bucket-encryption",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert!(
        setup.status.success(),
        "setup put-bucket-encryption must succeed; stderr: {}",
        String::from_utf8_lossy(&setup.stderr)
    );

    let output = run_s7cmd(&[
        "delete-bucket-encryption",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would delete bucket encryption");

    let get_out = run_s7cmd(&[
        "get-bucket-encryption",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    // After dry-run delete, the KMS algorithm we set should still be
    // present. (If the delete had really run, S3 would revert to its
    // default — SSE-S3 / AES256.)
    assert!(
        stdout.contains("aws:kms"),
        "delete-bucket-encryption --dry-run must NOT have removed encryption; \
         get-bucket-encryption stdout: {stdout}"
    );
}

// -----------------------------------------------------------------
// put-bucket-lifecycle-configuration / delete-bucket-lifecycle-configuration
// -----------------------------------------------------------------

#[tokio::test]
async fn put_bucket_lifecycle_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let cfg = create_test_file(&tmp_dir, "lc.json", sample_lifecycle_json().as_bytes());
    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-lifecycle-configuration",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert_dry_run_success(&output, "would put bucket lifecycle");

    let get_out = run_s7cmd(&[
        "get-bucket-lifecycle-configuration",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let get_succeeded = get_out.status.success();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        !get_succeeded,
        "put-bucket-lifecycle --dry-run must NOT have applied lifecycle; \
         get stdout: {}",
        String::from_utf8_lossy(&get_out.stdout)
    );
}

#[tokio::test]
async fn delete_bucket_lifecycle_dry_run_does_not_delete() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let cfg = create_test_file(&tmp_dir, "lc.json", sample_lifecycle_json().as_bytes());
    let bucket_arg = format!("s3://{bucket}");

    let setup = run_s7cmd(&[
        "put-bucket-lifecycle-configuration",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert!(setup.status.success());

    let output = run_s7cmd(&[
        "delete-bucket-lifecycle-configuration",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would delete bucket lifecycle");

    let get_out = run_s7cmd(&[
        "get-bucket-lifecycle-configuration",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        get_out.status.success(),
        "get-bucket-lifecycle-configuration must succeed after dry-run delete"
    );
    // Marker fields from sample_lifecycle_json: ID="r1", Days=30.
    assert!(
        stdout.contains("\"r1\""),
        "delete-bucket-lifecycle --dry-run must preserve rule ID 'r1'; \
         get stdout: {stdout}"
    );
    assert!(
        stdout.contains("30"),
        "delete-bucket-lifecycle --dry-run must preserve Days=30; \
         get stdout: {stdout}"
    );
}

// -----------------------------------------------------------------
// put-bucket-policy / delete-bucket-policy
// -----------------------------------------------------------------

fn deny_insecure_transport_policy(bucket: &str) -> String {
    // Use a Deny-style policy with an aws:SecureTransport condition.
    // A new bucket has BlockPublicPolicy=true in its default
    // PublicAccessBlock configuration, so any `"Principal":"*"` Allow
    // policy is rejected at PutBucketPolicy time. A Deny statement
    // is not considered "public" and is accepted on a fresh bucket.
    format!(
        r#"{{"Version":"2012-10-17","Statement":[{{"Sid":"DenyInsecureTransport","Effect":"Deny","Principal":"*","Action":"s3:*","Resource":["arn:aws:s3:::{bucket}","arn:aws:s3:::{bucket}/*"],"Condition":{{"Bool":{{"aws:SecureTransport":"false"}}}}}}]}}"#
    )
}

#[tokio::test]
async fn put_bucket_policy_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let policy_body = deny_insecure_transport_policy(&bucket);
    let cfg = create_test_file(&tmp_dir, "policy.json", policy_body.as_bytes());
    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-policy",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert_dry_run_success(&output, "would put bucket policy");

    let get_out = run_s7cmd(&[
        "get-bucket-policy",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let get_succeeded = get_out.status.success();
    helper.delete_bucket_with_cascade(&bucket).await;
    assert!(
        !get_succeeded,
        "put-bucket-policy --dry-run must NOT have applied policy"
    );
}

#[tokio::test]
async fn delete_bucket_policy_dry_run_does_not_delete() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let policy_body = deny_insecure_transport_policy(&bucket);
    let cfg = create_test_file(&tmp_dir, "policy.json", policy_body.as_bytes());
    let bucket_arg = format!("s3://{bucket}");

    let setup = run_s7cmd(&[
        "put-bucket-policy",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert!(setup.status.success());

    let output = run_s7cmd(&[
        "delete-bucket-policy",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would delete bucket policy");

    let get_out = run_s7cmd(&[
        "get-bucket-policy",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        get_out.status.success(),
        "get-bucket-policy must succeed after dry-run delete"
    );
    // Setup applied a deny-insecure-transport policy with a unique Sid
    // — confirm that exact Sid is present, not just *any* policy.
    assert!(
        stdout.contains("DenyInsecureTransport"),
        "delete-bucket-policy --dry-run must preserve the original policy \
         (Sid 'DenyInsecureTransport'); get stdout: {stdout}"
    );
}

// -----------------------------------------------------------------
// delete-bucket-tagging
// -----------------------------------------------------------------

#[tokio::test]
async fn delete_bucket_tagging_dry_run_does_not_delete() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let bucket_arg = format!("s3://{bucket}");

    let setup = run_s7cmd(&[
        "put-bucket-tagging",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        "--tagging",
        "env=test",
    ]);
    assert!(setup.status.success());

    let output = run_s7cmd(&[
        "delete-bucket-tagging",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would delete bucket tagging");

    let get_out = run_s7cmd(&[
        "get-bucket-tagging",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        get_out.status.success(),
        "get-bucket-tagging must succeed after dry-run delete"
    );
    // Setup applied env=test — confirm the exact key/value pair is
    // still present.
    assert!(
        stdout.contains("\"env\"") && stdout.contains("\"test\""),
        "delete-bucket-tagging --dry-run must preserve env=test tag; \
         get stdout: {stdout}"
    );
}

// -----------------------------------------------------------------
// put-bucket-website / delete-bucket-website
// -----------------------------------------------------------------

#[tokio::test]
async fn put_bucket_website_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let cfg = create_test_file(&tmp_dir, "web.json", sample_website_json().as_bytes());
    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-website",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert_dry_run_success(&output, "would put bucket website");

    let get_out = run_s7cmd(&[
        "get-bucket-website",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let get_succeeded = get_out.status.success();
    helper.delete_bucket_with_cascade(&bucket).await;
    assert!(
        !get_succeeded,
        "put-bucket-website --dry-run must NOT have applied website config"
    );
}

#[tokio::test]
async fn delete_bucket_website_dry_run_does_not_delete() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let cfg = create_test_file(&tmp_dir, "web.json", sample_website_json().as_bytes());
    let bucket_arg = format!("s3://{bucket}");

    let setup = run_s7cmd(&[
        "put-bucket-website",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert!(setup.status.success());

    let output = run_s7cmd(&[
        "delete-bucket-website",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would delete bucket website");

    let get_out = run_s7cmd(&[
        "get-bucket-website",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        get_out.status.success(),
        "get-bucket-website must succeed after dry-run delete"
    );
    // Setup applied IndexDocument with Suffix=index.html — confirm
    // the exact suffix value is still present.
    assert!(
        stdout.contains("index.html"),
        "delete-bucket-website --dry-run must preserve the IndexDocument \
         suffix; get stdout: {stdout}"
    );
}

// -----------------------------------------------------------------
// put-public-access-block / delete-public-access-block
// -----------------------------------------------------------------

#[tokio::test]
async fn put_public_access_block_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    // Use a distinguishable input: all four fields = false. AWS's
    // default PAB on a new bucket is all-true (since 2023), so if the
    // dry-run had actually issued PutPublicAccessBlock, get would
    // return all-false. If it did NOT issue the call, get returns
    // the all-true AWS default — which is what we assert below.
    let tmp_dir = create_temp_dir();
    let pab_all_false = br#"{"BlockPublicAcls":false,"IgnorePublicAcls":false,"BlockPublicPolicy":false,"RestrictPublicBuckets":false}"#;
    let cfg = create_test_file(&tmp_dir, "pab.json", pab_all_false);
    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-public-access-block",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert_dry_run_success(&output, "would put public access block");

    let get_out = run_s7cmd(&[
        "get-public-access-block",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    // Distinguish "dry-run skipped" vs "dry-run applied" by parsing
    // the JSON — the dry-run input had every field false, so each
    // field in the returned config must still be true (the AWS
    // default) for the dry-run to have skipped the call.
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("get-public-access-block must return JSON");
    let pab = &json["PublicAccessBlockConfiguration"];
    for field in [
        "BlockPublicAcls",
        "IgnorePublicAcls",
        "BlockPublicPolicy",
        "RestrictPublicBuckets",
    ] {
        assert_eq!(
            pab[field].as_bool(),
            Some(true),
            "put-public-access-block --dry-run must NOT have applied the all-false config; \
             field {field} should remain at AWS-default 'true'; got JSON: {stdout}"
        );
    }
}

#[tokio::test]
async fn delete_public_access_block_dry_run_does_not_delete() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let cfg = create_test_file(&tmp_dir, "pab.json", sample_pab_json().as_bytes());
    let bucket_arg = format!("s3://{bucket}");

    let setup = run_s7cmd(&[
        "put-public-access-block",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert!(setup.status.success());

    let output = run_s7cmd(&[
        "delete-public-access-block",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would delete public access block");

    let get_out = run_s7cmd(&[
        "get-public-access-block",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        get_out.status.success(),
        "get-public-access-block must succeed after dry-run delete"
    );
    // Setup applied all-true. Confirm every field is still true —
    // catches a buggy delete that returned the bucket to AWS defaults
    // even if those defaults happened to also be all-true.
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("get must return JSON");
    let pab = &json["PublicAccessBlockConfiguration"];
    for field in [
        "BlockPublicAcls",
        "IgnorePublicAcls",
        "BlockPublicPolicy",
        "RestrictPublicBuckets",
    ] {
        assert_eq!(
            pab[field].as_bool(),
            Some(true),
            "delete-public-access-block --dry-run must preserve the all-true \
             setup config; field {field} got: {stdout}"
        );
    }
}

// -----------------------------------------------------------------
// put-bucket-versioning
// -----------------------------------------------------------------

#[tokio::test]
async fn put_bucket_versioning_dry_run_does_not_change_state() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-versioning",
        "--dry-run",
        "--enabled",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would put bucket versioning");

    // Fresh bucket: versioning should still be unset/Suspended.
    // get-bucket-versioning returns success with empty/suspended status
    // either way; we assert the *enabled* status is NOT in the result.
    let get_out = run_s7cmd(&[
        "get-bucket-versioning",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        !stdout.contains("\"Enabled\""),
        "put-bucket-versioning --dry-run must NOT have enabled versioning; \
         get-bucket-versioning stdout: {stdout}"
    );
}

// -----------------------------------------------------------------
// put-object-tagging / delete-object-tagging
// -----------------------------------------------------------------

#[tokio::test]
async fn put_object_tagging_dry_run_does_not_change_tags() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let key = "dry-run-put-tagging.txt";
    helper
        .put_object_with_tagging(&bucket, key, b"x".to_vec(), "original=tag1")
        .await;

    let object_arg = format!("s3://{bucket}/{key}");
    let output = run_s7cmd(&[
        "put-object-tagging",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &object_arg,
        "--tagging",
        "replaced=tag2",
    ]);
    assert_dry_run_success(&output, "would put object tagging");

    // Read tags via SDK helper. We expect to still see the "original"
    // tag because the dry-run didn't issue PutObjectTagging.
    let tagging = helper.get_object_tagging(&bucket, key, None).await;
    let tags: Vec<(String, String)> = tagging
        .tag_set()
        .iter()
        .map(|t| (t.key().to_string(), t.value().to_string()))
        .collect();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        tags.iter().any(|(k, _)| k == "original"),
        "put-object-tagging --dry-run must preserve the existing tag set; got: {tags:?}"
    );
    assert!(
        !tags.iter().any(|(k, _)| k == "replaced"),
        "put-object-tagging --dry-run must NOT have applied the new tags; got: {tags:?}"
    );
}

#[tokio::test]
async fn delete_object_tagging_dry_run_does_not_remove_tags() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let key = "dry-run-delete-tagging.txt";
    helper
        .put_object_with_tagging(&bucket, key, b"x".to_vec(), "keep=me")
        .await;

    let object_arg = format!("s3://{bucket}/{key}");
    let output = run_s7cmd(&[
        "delete-object-tagging",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &object_arg,
    ]);
    assert_dry_run_success(&output, "would delete object tagging");

    let tagging = helper.get_object_tagging(&bucket, key, None).await;
    let still_present = tagging
        .tag_set()
        .iter()
        .any(|t| t.key() == "keep" && t.value() == "me");
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        still_present,
        "delete-object-tagging --dry-run must NOT have cleared tags"
    );
}

// -----------------------------------------------------------------
// put-bucket-logging — no DeleteBucketLogging API exists; setting an
// empty config disables. Verify that --dry-run leaves logging-disabled
// state unchanged (no LoggingEnabled in the result).
// -----------------------------------------------------------------

#[tokio::test]
async fn put_bucket_logging_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    // Create the target log-bucket the dry-run config references; even
    // though the dry-run doesn't actually issue PutBucketLogging, the
    // JSON parser still validates the body shape.
    let log_bucket = generate_bucket_name();
    helper.create_bucket(&log_bucket, REGION).await;

    let tmp_dir = create_temp_dir();
    let cfg_body =
        format!(r#"{{"LoggingEnabled":{{"TargetBucket":"{log_bucket}","TargetPrefix":"logs/"}}}}"#);
    let cfg = create_test_file(&tmp_dir, "log.json", cfg_body.as_bytes());
    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-logging",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert_dry_run_success(&output, "would put bucket logging");

    let get_out = run_s7cmd(&[
        "get-bucket-logging",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;
    helper.delete_bucket_with_cascade(&log_bucket).await;

    assert!(
        !stdout.contains("LoggingEnabled"),
        "put-bucket-logging --dry-run must NOT have enabled logging; \
         get-bucket-logging stdout: {stdout}"
    );
}

// -----------------------------------------------------------------
// put-bucket-notification-configuration — no separate delete API,
// and the AWS default already matches an empty config, so we can't
// detect "skipped" purely by reading state back. Instead, supply a
// notification config that *AWS would reject* (a TopicConfiguration
// pointing at a non-existent SNS topic): if --dry-run actually
// issued PutBucketNotificationConfiguration, AWS would respond
// with `InvalidArgument` and the binary would exit non-zero. A
// successful (exit 0) dry-run plus an empty notification state on
// get-back is concrete proof the API call was skipped.
// -----------------------------------------------------------------

#[tokio::test]
async fn put_bucket_notification_configuration_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    // SNS ARN that doesn't exist in this account. Syntactically valid
    // (parser accepts it) but AWS rejects on the real PutBucketNotification.
    let cfg_body = br#"{"TopicConfigurations":[{"Id":"dry-run-marker","TopicArn":"arn:aws:sns:us-east-1:000000000000:nonexistent","Events":["s3:ObjectCreated:*"]}]}"#;
    let tmp_dir = create_temp_dir();
    let cfg = create_test_file(&tmp_dir, "ntf.json", cfg_body);
    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-notification-configuration",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert_dry_run_success(&output, "would put bucket notification");

    let get_out = run_s7cmd(&[
        "get-bucket-notification-configuration",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        get_out.status.success(),
        "get-bucket-notification-configuration must succeed"
    );
    // The dry-run input had a unique Id "dry-run-marker". If the
    // call had actually been issued, either (a) AWS would have
    // rejected it (test would fail at assert_dry_run_success above)
    // or (b) the marker would now appear in get-back JSON. Neither
    // happens — confirming the API was skipped.
    assert!(
        !stdout.contains("dry-run-marker"),
        "put-bucket-notification-configuration --dry-run must NOT have \
         applied the marker config; get stdout: {stdout}"
    );
}

// ---------------------------------------------------------------
// v1.3.0 dry-run end-to-end coverage
// ---------------------------------------------------------------

#[tokio::test]
async fn put_bucket_replication_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let dest = generate_bucket_name();
    let cfg_body = format!(
        r#"{{
            "Role": "arn:aws:iam::000000000000:role/s3-replication-test",
            "Rules": [
                {{
                    "ID": "rule-1",
                    "Priority": 1,
                    "Filter": {{}},
                    "Status": "Enabled",
                    "DeleteMarkerReplication": {{ "Status": "Disabled" }},
                    "Destination": {{ "Bucket": "arn:aws:s3:::{dest}" }}
                }}
            ]
        }}"#
    );
    let tmp_dir = create_temp_dir();
    let cfg = create_test_file(&tmp_dir, "rep.json", cfg_body.as_bytes());

    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-replication",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
        cfg.to_str().unwrap(),
    ]);
    assert_dry_run_success(&output, "would put bucket replication");

    // Confirm replication was NOT applied — get-bucket-replication on a
    // bucket without replication returns 4 (NoSuchReplicationConfiguration).
    let get_out = run_s7cmd(&[
        "get-bucket-replication",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);

    helper.delete_bucket_with_cascade(&bucket).await;

    assert_eq!(
        get_out.status.code(),
        Some(4),
        "put-bucket-replication --dry-run must NOT have applied replication"
    );
}

/// Verify `delete-bucket-replication --dry-run` does NOT remove an existing
/// replication configuration. Requires `S7CMD_E2E_REPLICATION_ROLE_ARN` to
/// point at an IAM role whose trust policy allows `s3.amazonaws.com` to
/// AssumeRole, and the e2e profile must have `iam:PassRole` on it.
/// Without the env var, the test is skipped (return) rather than failed.
#[tokio::test]
async fn delete_bucket_replication_dry_run_does_not_change_state() {
    use aws_sdk_s3::types::{
        DeleteMarkerReplication, DeleteMarkerReplicationStatus, Destination,
        ReplicationConfiguration, ReplicationRule, ReplicationRuleFilter, ReplicationRuleStatus,
    };

    let Ok(role_arn) = std::env::var("S7CMD_E2E_REPLICATION_ROLE_ARN") else {
        eprintln!(
            "skipping delete_bucket_replication_dry_run_does_not_change_state: \
             S7CMD_E2E_REPLICATION_ROLE_ARN is not set"
        );
        return;
    };

    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    let dest_bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper.create_bucket(&dest_bucket, REGION).await;
    helper.enable_bucket_versioning(&bucket).await;
    helper.enable_bucket_versioning(&dest_bucket).await;

    // Set up replication via the SDK directly (not via s7cmd) so the
    // dry-run delete has real, observable state to preserve.
    let cfg = ReplicationConfiguration::builder()
        .role(&role_arn)
        .rules(
            ReplicationRule::builder()
                .id("dry-run-marker")
                .priority(1)
                .filter(ReplicationRuleFilter::builder().build())
                .status(ReplicationRuleStatus::Enabled)
                .delete_marker_replication(
                    DeleteMarkerReplication::builder()
                        .status(DeleteMarkerReplicationStatus::Disabled)
                        .build(),
                )
                .destination(
                    Destination::builder()
                        .bucket(format!("arn:aws:s3:::{dest_bucket}"))
                        .build()
                        .unwrap(),
                )
                .build()
                .unwrap(),
        )
        .build()
        .unwrap();

    helper
        .client
        .put_bucket_replication()
        .bucket(&bucket)
        .replication_configuration(cfg)
        .send()
        .await
        .expect("PutBucketReplication setup must succeed");

    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "delete-bucket-replication",
        "--dry-run",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would delete bucket replication");

    // Replication must still be present — if the dry-run had really
    // issued DeleteBucketReplication, get would return NotFound (4).
    let get_after = run_s7cmd(&[
        "get-bucket-replication",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let get_succeeded = get_after.status.success();
    let get_stdout = String::from_utf8_lossy(&get_after.stdout).to_string();

    helper.delete_bucket_with_cascade(&bucket).await;
    helper.delete_bucket_with_cascade(&dest_bucket).await;

    assert!(
        get_succeeded,
        "delete-bucket-replication --dry-run must NOT have deleted replication; \
         get exit code: {:?}",
        get_after.status.code()
    );
    assert!(
        get_stdout.contains("dry-run-marker"),
        "replication rule (id=dry-run-marker) must still be present after \
         dry-run delete; get stdout: {get_stdout}"
    );
}

#[tokio::test]
async fn put_bucket_accelerate_configuration_enabled_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-accelerate-configuration",
        "--dry-run",
        "--enabled",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would put bucket accelerate configuration");

    let get_out = run_s7cmd(&[
        "get-bucket-accelerate-configuration",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(
        get_out.status.success(),
        "get-bucket-accelerate-configuration must succeed; stderr: {}",
        String::from_utf8_lossy(&get_out.stderr)
    );
    // Fresh bucket has no Status set; if the put had actually been
    // issued, get would now show Status=Enabled.
    assert!(
        !stdout.contains("\"Enabled\""),
        "put-bucket-accelerate --dry-run --enabled must NOT have enabled \
         acceleration; get stdout: {stdout}"
    );
}

#[tokio::test]
async fn put_bucket_accelerate_configuration_suspended_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-accelerate-configuration",
        "--dry-run",
        "--suspended",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would put bucket accelerate configuration");

    let get_out = run_s7cmd(&[
        "get-bucket-accelerate-configuration",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(get_out.status.success());
    // Fresh bucket has no Status set; if the put had been issued, get
    // would show Status=Suspended.
    assert!(
        !stdout.contains("\"Suspended\""),
        "put-bucket-accelerate --dry-run --suspended must NOT have applied \
         Suspended; get stdout: {stdout}"
    );
}

#[tokio::test]
async fn put_bucket_request_payment_requester_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let bucket_arg = format!("s3://{bucket}");
    let output = run_s7cmd(&[
        "put-bucket-request-payment",
        "--dry-run",
        "--requester",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would put bucket request payment");

    let get_out = run_s7cmd(&[
        "get-bucket-request-payment",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(get_out.status.success());
    assert!(
        stdout.contains("\"BucketOwner\""),
        "put-bucket-request-payment --dry-run --requester must NOT have \
         flipped Payer; expected default BucketOwner; got: {stdout}"
    );
    assert!(
        !stdout.contains("\"Requester\""),
        "put-bucket-request-payment --dry-run --requester must NOT have \
         applied Requester; got: {stdout}"
    );
}

#[tokio::test]
async fn put_bucket_request_payment_bucket_owner_dry_run_does_not_apply() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let bucket_arg = format!("s3://{bucket}");

    // Set Payer=Requester first so the dry-run --bucket-owner has
    // something to potentially flip.
    let setup = run_s7cmd(&[
        "put-bucket-request-payment",
        "--requester",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert!(setup.status.success(), "setup must succeed");

    let output = run_s7cmd(&[
        "put-bucket-request-payment",
        "--dry-run",
        "--bucket-owner",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    assert_dry_run_success(&output, "would put bucket request payment");

    let get_out = run_s7cmd(&[
        "get-bucket-request-payment",
        "--target-profile",
        "s7cmd-e2e-test",
        &bucket_arg,
    ]);
    let stdout = String::from_utf8_lossy(&get_out.stdout).to_string();
    helper.delete_bucket_with_cascade(&bucket).await;

    assert!(get_out.status.success());
    // Setup made it Requester; dry-run --bucket-owner should NOT have
    // flipped it back. If the dry-run had issued the put, get would
    // now show BucketOwner.
    assert!(
        stdout.contains("\"Requester\""),
        "put-bucket-request-payment --dry-run --bucket-owner must NOT have \
         flipped Payer back; expected Requester; got: {stdout}"
    );
}

#[tokio::test]
async fn restore_object_dry_run_does_not_call_api() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let key = "dry-run-restore.txt";
    helper.put_object(&bucket, key, b"x".to_vec()).await;

    let object_arg = format!("s3://{bucket}/{key}");
    let output = run_s7cmd(&[
        "restore-object",
        "--dry-run",
        "--days",
        "1",
        "--tier",
        "Standard",
        "--target-profile",
        "s7cmd-e2e-test",
        &object_arg,
    ]);
    // If the API call had been issued, AWS would respond with
    // InvalidObjectState (Standard-class objects can't be restored)
    // and the binary would exit non-zero. A successful dry-run proves
    // the call was skipped.
    assert_dry_run_success(&output, "would restore object");

    helper.delete_bucket_with_cascade(&bucket).await;
}

#[tokio::test]
async fn restore_object_dry_run_with_version_id_does_not_call_api() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper.enable_bucket_versioning(&bucket).await;

    let key = "dry-run-restore-versioned.txt";
    let version_id = helper
        .put_object_returning_version_id(&bucket, key, b"x".to_vec())
        .await;

    let object_arg = format!("s3://{bucket}/{key}");
    let output = run_s7cmd(&[
        "restore-object",
        "--dry-run",
        "--days",
        "1",
        "--tier",
        "Bulk",
        "--source-version-id",
        &version_id,
        "--target-profile",
        "s7cmd-e2e-test",
        &object_arg,
    ]);
    assert_dry_run_success(&output, "would restore object");
    // Cross-check: the version_id field appears in the dry-run log,
    // so we know our runtime handed it through to the format.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&version_id),
        "version_id must appear in dry-run log line; got: {stderr}"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}
