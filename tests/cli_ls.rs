//! Process-level CLI tests for the `ls` subcommand: broken-pipe handling and
//! env-var isolation of the positional target. These run without AWS
//! credentials or network access (they talk only to a loopback mock
//! endpoint).
//!
//! `s7cmd ls | head -1`-style consumers close the pipe early; `ls` must treat
//! the resulting `BrokenPipe` write error as a silent success (exit 0), in
//! both bucket-listing mode (no target) and listing-pipeline mode (target
//! prefix). Each broken-pipe test delays the mock response, closes the
//! child's stdout immediately after spawn, and only then lets the listing
//! arrive — so the child's first stdout flush hits a closed pipe
//! deterministically.

mod common;
use common::{MockResponse, MockS3Server, mock_target_args, run, s7cmd_cmd_clean_env};

use std::time::Duration;

const LIST_BUCKETS_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
    <ListAllMyBucketsResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
      <Owner><ID>mock-owner</ID><DisplayName>mock</DisplayName></Owner>\
      <Buckets>\
        <Bucket><Name>mock-bucket-1</Name><CreationDate>2026-01-01T00:00:00.000Z</CreationDate></Bucket>\
        <Bucket><Name>mock-bucket-2</Name><CreationDate>2026-01-01T00:00:00.000Z</CreationDate></Bucket>\
      </Buckets>\
    </ListAllMyBucketsResult>";

const LIST_OBJECTS_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
    <ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
      <Name>mock-bucket</Name><Prefix></Prefix><KeyCount>2</KeyCount>\
      <MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated>\
      <Contents><Key>key-1</Key><LastModified>2026-01-01T00:00:00.000Z</LastModified>\
        <ETag>&quot;mock-etag-1&quot;</ETag><Size>1</Size><StorageClass>STANDARD</StorageClass></Contents>\
      <Contents><Key>key-2</Key><LastModified>2026-01-01T00:00:00.000Z</LastModified>\
        <ETag>&quot;mock-etag-2&quot;</ETag><Size>2</Size><StorageClass>STANDARD</StorageClass></Contents>\
    </ListBucketResult>";

/// Spawn `s7cmd ls` with the given extra args against `server`, close its
/// stdout pipe immediately, and return `(exit_code, stderr)`.
fn run_ls_with_closed_stdout(server: &MockS3Server, extra: &[&str]) -> (Option<i32>, String) {
    use std::io::Read;

    let mut cmd = s7cmd_cmd_clean_env();
    cmd.arg("ls")
        .args(mock_target_args(&server.endpoint_url()))
        .args(extra);
    let mut child = cmd.spawn().expect("failed to spawn s7cmd binary");
    // Close the read end of the child's stdout before the (delayed) listing
    // arrives: the child's first write/flush then fails with BrokenPipe.
    drop(child.stdout.take());
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr).ok();
    }
    let status = child.wait().expect("wait for s7cmd");
    (status.code(), stderr)
}

#[test]
fn bucket_listing_broken_pipe_exits_0() {
    let server = MockS3Server::start(vec![
        MockResponse::new(200, LIST_BUCKETS_XML).with_delay(Duration::from_millis(500)),
    ]);
    let (code, stderr) = run_ls_with_closed_stdout(&server, &[]);
    assert_eq!(
        code,
        Some(0),
        "BrokenPipe in bucket-listing mode must exit 0; stderr: {stderr}"
    );
}

/// A `TARGET` env var must not supply `ls`'s optional positional. Through
/// s3ls-rs 1.1.0 the positional carried clap's `env` attribute, so an
/// exported TARGET was parsed as the listing target (an invalid value even
/// failed the run at clap parse time); s3ls-rs 1.2.0 drops `env`. With the
/// variable exported, `ls` must ignore it and fall back to bucket-listing
/// mode against the mock endpoint.
#[test]
fn target_env_does_not_supply_ls_target() {
    let server = MockS3Server::start(vec![MockResponse::new(200, LIST_BUCKETS_XML)]);
    let (code, stdout, stderr) = run(s7cmd_cmd_clean_env()
        .arg("ls")
        .args(mock_target_args(&server.endpoint_url()))
        .env("TARGET", "notavalidpath"));
    assert_eq!(
        code,
        Some(0),
        "TARGET env var must be ignored (bucket-listing mode); stderr: {stderr}"
    );
    assert!(
        stdout.contains("mock-bucket-1"),
        "expected bucket listing on stdout; got: {stdout}"
    );
}

#[test]
fn listing_pipeline_broken_pipe_exits_0() {
    let server = MockS3Server::start(vec![
        MockResponse::new(200, LIST_OBJECTS_XML).with_delay(Duration::from_millis(500)),
    ]);
    let (code, stderr) = run_ls_with_closed_stdout(&server, &["s3://mock-bucket/"]);
    assert_eq!(
        code,
        Some(0),
        "BrokenPipe in listing-pipeline mode must exit 0; stderr: {stderr}"
    );
}
