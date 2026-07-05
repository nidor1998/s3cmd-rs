//! Process-level CLI tests for the `clean` subcommand against a loopback
//! mock endpoint. These run without AWS credentials or network access.

mod common;
use common::{MockResponse, MockS3Server, mock_target_args, s7cmd_cmd_clean_env};

const LIST_TWO_OBJECTS_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
    <ListBucketResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
      <Name>mock-bucket</Name><Prefix>prefix/</Prefix><KeyCount>2</KeyCount>\
      <MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated>\
      <Contents><Key>prefix/key-1</Key><LastModified>2026-01-01T00:00:00.000Z</LastModified>\
        <ETag>&quot;mock-etag-1&quot;</ETag><Size>1</Size><StorageClass>STANDARD</StorageClass></Contents>\
      <Contents><Key>prefix/key-2</Key><LastModified>2026-01-01T00:00:00.000Z</LastModified>\
        <ETag>&quot;mock-etag-2&quot;</ETag><Size>2</Size><StorageClass>STANDARD</StorageClass></Contents>\
    </ListBucketResult>";

const DELETE_ONE_OBJECT_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
    <DeleteResult xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
      <Deleted><Key>prefix/key-1</Key></Deleted>\
    </DeleteResult>";

#[test]
fn max_delete_reached_exits_with_warning() {
    // Two listed objects but --max-delete 1: the second object trips the
    // threshold, the deleter flushes the first (DeleteObjects), sets the
    // warning flag and cancels the pipeline. The run must not exit 0 —
    // s7cmd clean reports the truncated deletion as warning exit code 3.
    let server = MockS3Server::start(vec![
        MockResponse::new(200, LIST_TWO_OBJECTS_XML),
        MockResponse::new(200, DELETE_ONE_OBJECT_XML),
    ]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["clean", "--force", "--max-delete", "1"])
        .args(mock_target_args(&server.endpoint_url()))
        .arg("s3://mock-bucket/prefix/");
    let (code, stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(3),
        "max-delete truncation should exit 3 (warning); stdout: {stdout}; stderr: {stderr}"
    );
    assert!(
        stderr.contains("--max-delete has been reached"),
        "expected the max-delete warning; got: {stderr}"
    );
    let requests = server.requests();
    assert!(
        requests
            .iter()
            .any(|r| r.starts_with("POST ") && r.contains("delete")),
        "expected the buffered object to be flushed via DeleteObjects; got: {requests:?}"
    );
}
