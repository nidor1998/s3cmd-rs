//! Process-level CLI tests for the `get-object-annotation` subcommand.
//! These run without AWS credentials or network access (mock-endpoint
//! tests talk only to a loopback HTTP server).

mod common;
use common::{MockResponse, MockS3Server, mock_target_args, s7cmd_cmd_clean_env};

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

#[test]
fn help_succeeds_and_lists_option_groups() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["get-object-annotation", "--help"]));
    assert!(ok, "get-object-annotation --help must succeed");
    assert!(stdout.contains("AWS Configuration"));
    assert!(stdout.contains("Retry Options"));
    assert!(stdout.contains("Timeout Options"));
}

#[test]
fn missing_positional_exits_2() {
    let (ok, _stdout, stderr, code) = run(s7cmd().arg("get-object-annotation"));
    assert!(!ok);
    assert_eq!(code, Some(2), "clap missing-arg should exit 2");
    assert!(stderr.to_lowercase().contains("required") || stderr.to_lowercase().contains("usage"));
}

#[test]
fn missing_annotation_name_exits_2() {
    // target + outfile present, but --annotation-name is required.
    let (ok, _stdout, stderr, code) =
        run(s7cmd().args(["get-object-annotation", "s3://bucket/key", "-"]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(2),
        "--annotation-name is required; should exit 2"
    );
    assert!(stderr.to_lowercase().contains("required") || stderr.to_lowercase().contains("usage"));
}

#[test]
fn missing_outfile_exits_2() {
    // target + --annotation-name present, but the outfile positional is required.
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "get-object-annotation",
        "s3://bucket/key",
        "--annotation-name",
        "note",
    ]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(2),
        "outfile positional is required; should exit 2"
    );
    assert!(stderr.to_lowercase().contains("required") || stderr.to_lowercase().contains("usage"));
}

#[test]
fn bucket_only_path_no_key_exits_1() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "get-object-annotation",
        "s3://bucket",
        "-",
        "--annotation-name",
        "note",
    ]));
    assert!(!ok);
    assert_eq!(code, Some(1), "bucket-only path should exit 1 (validation)");
    assert!(
        !stderr.is_empty(),
        "should have an error message on stderr; got empty"
    );
}

// NOTE: s3util-rs's auto_complete_shell_short_circuits_without_target test
// is omitted — s7cmd hides the per-subcommand --auto-complete-shell flag
// and exposes only the top-level form (covered by tests/cli_help.rs).

#[test]
fn target_access_key_without_secret_exits_non_zero() {
    let (ok, _stdout, stderr, code) = run(s7cmd().args([
        "get-object-annotation",
        "s3://bucket/key",
        "-",
        "--annotation-name",
        "note",
        "--target-access-key",
        "AKIA",
    ]));
    assert!(!ok);
    assert_eq!(
        code,
        Some(2),
        "clap missing-arg should exit 2; stderr: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("required")
            || stderr.to_lowercase().contains("--target-secret-access-key")
    );
}

#[test]
fn help_mentions_annotation_name_and_target_version_id() {
    let (ok, stdout, _stderr, _code) = run(s7cmd().args(["get-object-annotation", "--help"]));
    assert!(ok);
    assert!(
        stdout.contains("annotation-name"),
        "help should list --annotation-name; got: {stdout}"
    );
    assert!(
        stdout.contains("target-version-id"),
        "help should list --target-version-id; got: {stdout}"
    );
}

// ---------- mock-endpoint tests ----------

#[test]
fn mock_endpoint_no_such_version_exits_4_with_version_in_message() {
    let dir = tempfile::tempdir().unwrap();
    let outfile = dir.path().join("annotation.bin");
    let server = MockS3Server::start(vec![MockResponse::s3_error(404, "NoSuchVersion")]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args([
        "get-object-annotation",
        "--annotation-name",
        "note",
        "--target-version-id",
        "mock-version",
    ])
    .args(mock_target_args(&server.endpoint_url()))
    .args(["s3://mock-bucket/mock-key", outfile.to_str().unwrap()]);
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(4),
        "NoSuchVersion should exit 4; stderr: {stderr}"
    );
    assert!(
        stderr.contains("s3://mock-bucket/mock-key (versionId=mock-version) not found"),
        "expected the versioned not-found message; got: {stderr}"
    );
    assert!(!outfile.exists(), "no outfile may be created on error");
}

#[test]
fn mock_endpoint_no_such_annotation_with_version_exits_4() {
    let dir = tempfile::tempdir().unwrap();
    let outfile = dir.path().join("annotation.bin");
    let server = MockS3Server::start(vec![MockResponse::s3_error(404, "NoSuchAnnotation")]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args([
        "get-object-annotation",
        "--annotation-name",
        "note",
        "--target-version-id",
        "mock-version",
    ])
    .args(mock_target_args(&server.endpoint_url()))
    .args(["s3://mock-bucket/mock-key", outfile.to_str().unwrap()]);
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(4),
        "NoSuchAnnotation should exit 4; stderr: {stderr}"
    );
    assert!(
        stderr.contains(
            "annotation note not found for s3://mock-bucket/mock-key (versionId=mock-version)"
        ),
        "expected the versioned annotation-not-found message; got: {stderr}"
    );
    assert!(!outfile.exists(), "no outfile may be created on error");
}

#[test]
fn mock_endpoint_unverifiable_payload_written_to_bare_outfile() {
    // No SSE header and no checksum headers: nothing to verify, so the
    // payload is written with the "could not be verified" warning. The bare
    // (parent-less) outfile exercises the `Path::new(".")` fallback — the
    // child's working directory is a temp dir so the file lands there.
    let dir = tempfile::tempdir().unwrap();
    let payload = "mock annotation payload";
    let server = MockS3Server::start(vec![MockResponse::new(200, payload)]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.current_dir(dir.path())
        .args(["get-object-annotation", "--annotation-name", "note"])
        .args(mock_target_args(&server.endpoint_url()))
        .args(["s3://mock-bucket/mock-key", "annotation.bin"]);
    let (code, stdout, stderr) = common::run(&mut cmd);
    assert_eq!(code, Some(0), "expected success; stderr: {stderr}");
    assert!(
        stderr.contains("payload integrity could not be verified"),
        "expected the unverifiable warning; got: {stderr}"
    );
    assert_eq!(
        std::fs::read(dir.path().join("annotation.bin")).unwrap(),
        payload.as_bytes(),
        "outfile must hold the annotation payload"
    );
    assert!(
        stdout.contains("\"ContentLength\""),
        "expected the JSON metadata on stdout; got: {stdout}"
    );
}

#[test]
fn mock_endpoint_composite_checksum_skips_checksum_verification() {
    // A COMPOSITE checksum type (multipart-style) cannot be recomputed from
    // the payload bytes, so the runner must ignore the checksum header
    // (rather than fail the mismatch) and fall back to unverifiable.
    let dir = tempfile::tempdir().unwrap();
    let outfile = dir.path().join("annotation.bin");
    let payload = "mock annotation payload";
    let server = MockS3Server::start(vec![MockResponse::new(200, payload).with_headers(&[
        ("x-amz-checksum-type", "COMPOSITE"),
        ("x-amz-checksum-crc32", "AAAAAA==-2"),
    ])]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["get-object-annotation", "--annotation-name", "note"])
        .args(mock_target_args(&server.endpoint_url()))
        .args(["s3://mock-bucket/mock-key", outfile.to_str().unwrap()]);
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(0),
        "composite checksum must not be verified; stderr: {stderr}"
    );
    assert!(
        stderr.contains("payload integrity could not be verified"),
        "expected the unverifiable warning; got: {stderr}"
    );
    assert_eq!(
        std::fs::read(&outfile).unwrap(),
        payload.as_bytes(),
        "outfile must hold the annotation payload"
    );
}

#[test]
fn mock_endpoint_payload_over_1mib_exits_1_without_outfile() {
    // A buggy or hostile endpoint returning more than the 1 MiB annotation
    // cap must abort the bounded read, exit 1, and leave no outfile behind.
    let dir = tempfile::tempdir().unwrap();
    let outfile = dir.path().join("annotation.bin");
    let oversized = vec![b'a'; 1024 * 1024 + 1];
    let server = MockS3Server::start(vec![MockResponse::new(200, oversized)]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["get-object-annotation", "--annotation-name", "note"])
        .args(mock_target_args(&server.endpoint_url()))
        .args(["s3://mock-bucket/mock-key", outfile.to_str().unwrap()]);
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(1),
        "oversized payload should exit 1; stderr: {stderr}"
    );
    assert!(
        stderr.contains("exceeds the 1 MiB limit"),
        "expected the cap message; got: {stderr}"
    );
    assert!(!outfile.exists(), "no outfile may be created on error");
}

#[test]
fn mock_endpoint_matching_checksum_written_and_verified() {
    // A correct CRC64NVME response header: the in-transit and post-write
    // checks both pass and the run reports "written and verified".
    let dir = tempfile::tempdir().unwrap();
    let outfile = dir.path().join("annotation.bin");
    let payload = "mock annotation payload";
    let crc64 = s3util_rs::storage::annotation::compute_checksum_base64(
        payload.as_bytes(),
        aws_sdk_s3::types::ChecksumAlgorithm::Crc64Nvme,
    );
    let server = MockS3Server::start(vec![
        MockResponse::new(200, payload)
            .with_headers(&[("x-amz-checksum-crc64nvme", crc64.as_str())]),
    ]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["get-object-annotation", "-v", "--annotation-name", "note"])
        .args(mock_target_args(&server.endpoint_url()))
        .args(["s3://mock-bucket/mock-key", outfile.to_str().unwrap()]);
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(code, Some(0), "expected success; stderr: {stderr}");
    assert!(
        stderr.contains("written and verified"),
        "expected the verified info line (-v); got: {stderr}"
    );
    assert!(
        !stderr.contains("could not be verified"),
        "must not warn when the checksum matched; got: {stderr}"
    );
    assert_eq!(
        std::fs::read(&outfile).unwrap(),
        payload.as_bytes(),
        "outfile must hold the annotation payload"
    );
}

#[test]
fn mock_endpoint_checksum_mismatch_exits_1_without_outfile() {
    // The response checksum does not match the payload bytes. The SDK's
    // response-checksum validation (checksum mode ENABLED) catches this
    // while streaming the body — the run must exit 1 and never create the
    // outfile (a corrupted download must never be written).
    let dir = tempfile::tempdir().unwrap();
    let outfile = dir.path().join("annotation.bin");
    let payload = "mock annotation payload";
    let wrong_crc64 = s3util_rs::storage::annotation::compute_checksum_base64(
        b"different bytes entirely",
        aws_sdk_s3::types::ChecksumAlgorithm::Crc64Nvme,
    );
    let server = MockS3Server::start(vec![
        MockResponse::new(200, payload)
            .with_headers(&[("x-amz-checksum-crc64nvme", wrong_crc64.as_str())]),
    ]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["get-object-annotation", "--annotation-name", "note"])
        .args(mock_target_args(&server.endpoint_url()))
        .args(["s3://mock-bucket/mock-key", outfile.to_str().unwrap()]);
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(1),
        "checksum mismatch should exit 1; stderr: {stderr}"
    );
    assert!(
        stderr.contains("checksum mismatch"),
        "expected the streaming checksum-mismatch error; got: {stderr}"
    );
    assert!(!outfile.exists(), "no outfile may be created on mismatch");
}

#[test]
fn mock_endpoint_aes256_etag_mismatch_exits_1_without_outfile() {
    // An AES256 object whose ETag is not the MD5 of the received bytes. The
    // SDK does not validate ETags, so this exercises the runner's own
    // in-transit integrity check: it must abort with exit 1 and leave no
    // outfile behind.
    let dir = tempfile::tempdir().unwrap();
    let outfile = dir.path().join("annotation.bin");
    let payload = "mock annotation payload";
    let wrong_etag = format!("\"{:x}\"", md5::compute(b"different bytes entirely"));
    let server = MockS3Server::start(vec![MockResponse::new(200, payload).with_headers(&[
        ("x-amz-server-side-encryption", "AES256"),
        ("ETag", wrong_etag.as_str()),
    ])]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["get-object-annotation", "--annotation-name", "note"])
        .args(mock_target_args(&server.endpoint_url()))
        .args(["s3://mock-bucket/mock-key", outfile.to_str().unwrap()]);
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(1),
        "ETag mismatch should exit 1; stderr: {stderr}"
    );
    assert!(
        stderr.contains("ETag (MD5) verification failed"),
        "expected the ETag verification error; got: {stderr}"
    );
    assert!(!outfile.exists(), "no outfile may be created on mismatch");
}

#[test]
fn mock_endpoint_write_to_unwritable_outfile_exits_1_without_outfile() {
    // The S3 fetch succeeds and the payload passes the in-transit checks
    // (unverifiable, but writable), yet the <OUTFILE>'s parent directory does
    // not exist so the temp-file write fails. run_get_object_annotation must
    // propagate that as exit 1 and create nothing — covering the
    // write_verified_output error branch reached only after a successful fetch.
    let dir = tempfile::tempdir().unwrap();
    let missing_parent = dir.path().join("no-such-subdir");
    let outfile = missing_parent.join("annotation.bin");
    let payload = "mock annotation payload";
    let server = MockS3Server::start(vec![MockResponse::new(200, payload)]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["get-object-annotation", "--annotation-name", "note"])
        .args(mock_target_args(&server.endpoint_url()))
        .args(["s3://mock-bucket/mock-key", outfile.to_str().unwrap()]);
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(1),
        "an unwritable outfile must exit 1; stderr: {stderr}"
    );
    assert!(
        stderr.contains("creating temp file next to"),
        "expected the temp-file creation error; got: {stderr}"
    );
    assert!(
        !outfile.exists(),
        "no outfile may be created on write failure"
    );
    assert!(
        !missing_parent.exists(),
        "the missing parent dir must not be created"
    );
}
