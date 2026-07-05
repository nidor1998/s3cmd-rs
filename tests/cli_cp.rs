//! Process-level CLI tests for the `cp` subcommand against a loopback mock
//! endpoint. These run without AWS credentials or network access.

mod common;
use common::{MockResponse, MockS3Server, mock_target_args, s7cmd_cmd_clean_env};

#[test]
fn skip_existing_head_failure_exits_1() {
    // --skip-existing on an S3 target issues HeadObject first. A non-404
    // failure there (403) is not "target absent" — it must abort the copy
    // with exit 1 instead of proceeding as if the target didn't exist.
    let src = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(src.path(), b"mock source body").unwrap();
    let server = MockS3Server::start(vec![MockResponse::s3_error(403, "AccessDenied")]);
    let mut cmd = s7cmd_cmd_clean_env();
    cmd.args(["cp", "--skip-existing", src.path().to_str().unwrap()])
        .args(mock_target_args(&server.endpoint_url()))
        .arg("s3://mock-bucket/mock-key");
    let (code, _stdout, stderr) = common::run(&mut cmd);
    assert_eq!(
        code,
        Some(1),
        "HeadObject failure under --skip-existing should exit 1; stderr: {stderr}"
    );
    let requests = server.requests();
    assert!(
        requests.iter().any(|r| r.starts_with("HEAD ")),
        "expected a HeadObject probe; got: {requests:?}"
    );
}
