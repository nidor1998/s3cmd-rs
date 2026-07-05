//! Shared test helpers.
//!
//! Two layers:
//!
//! 1. **Always-available** — `s7cmd_cmd`, `run`, `create_temp_dir`,
//!    `create_test_file`, `generate_bucket_name`, `PROFILE_NAME`. Used by
//!    every test file (including the non-AWS `cli_arg_validation.rs`).
//!
//! 2. **`cfg(e2e_test)`-gated** — `TestHelper` plus `REGION`. Used only by
//!    the `e2e_*.rs` files. Default `cargo test` does not compile or load
//!    these, so it never tries to construct an AWS SDK client.

#![allow(dead_code)] // helpers are pulled in à la carte by each test file

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// === Always-available constants and helpers ===

/// AWS profile prepared by the maintainer. All e2e tests authenticate
/// through this profile.
pub const PROFILE_NAME: &str = "s7cmd-e2e-test";

/// Build a `Command` pointing at the freshly-built `s7cmd` binary with
/// stdin closed and stdout/stderr piped — the standard shape for assertion
/// capture. Tests can chain `.args(...)` / `.env_remove(...)` as needed.
pub fn s7cmd_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_s7cmd"));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Run a prepared command and return `(exit_code, stdout, stderr)`.
/// Both stdout and stderr are surfaced so failing assertions can include
/// the full child output.
pub fn run(cmd: &mut Command) -> (Option<i32>, String, String) {
    let output = cmd.output().expect("failed to spawn s7cmd binary");
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    )
}

/// UUID-suffixed bucket name. Globally unique so parallel test runs do not
/// collide and so leftover buckets from prior runs are easy to identify.
pub fn generate_bucket_name() -> String {
    format!("s7cmd-e2e-{}", uuid::Uuid::new_v4())
}

/// Per-test temp directory under `./playground/`. The parent is gitignored.
pub fn create_temp_dir() -> PathBuf {
    let dir = PathBuf::from(format!("./playground/tmp_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("create_temp_dir");
    dir
}

/// Write `body` to `dir/name` and return the full path.
pub fn create_test_file(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).expect("create_test_file");
    path
}

/// Create a zero-filled file of `size` bytes — used by the Ctrl+C tests
/// to make a transfer last long enough for SIGINT to land mid-stream.
pub fn create_sized_file(dir: &Path, name: &str, size: usize) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, vec![0u8; size]).expect("create_sized_file");
    path
}

// === Mock S3 endpoint (no AWS, no network beyond loopback) ===

/// One canned HTTP response served by [`MockS3Server`].
pub struct MockResponse {
    pub status: u16,
    /// Extra response headers, e.g. `("ETag", "\"abc\"")`. `Content-Length`,
    /// `Content-Type` (application/xml) and `Connection: close` are added
    /// automatically.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Delay before writing the response — used by broken-pipe tests to
    /// guarantee the parent closes the child's stdout before the child
    /// receives (and prints) the listing.
    pub delay: Option<std::time::Duration>,
}

impl MockResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
            delay: None,
        }
    }

    pub fn with_headers(mut self, headers: &[(&str, &str)]) -> Self {
        self.headers = headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self
    }

    pub fn with_delay(mut self, delay: std::time::Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// Standard S3 error response: XML body with the given error `code`,
    /// served with the given HTTP status. The AWS SDK extracts `<Code>` and
    /// s3util-rs / s3rm-rs / s3ls-rs classify arms off it, so this is how
    /// tests drive specific `NoSuchBucket` / `NoSuchKey` / `NoSuchAnnotation`
    /// / subresource-not-found match arms without AWS.
    pub fn s3_error(status: u16, code: &str) -> Self {
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <Error><Code>{code}</Code><Message>mock {code}</Message>\
             <RequestId>mock-request-id</RequestId></Error>"
        );
        Self::new(status, body.into_bytes())
    }
}

/// Minimal single-threaded HTTP/1.1 server that plays back `responses` in
/// order, one per request, repeating the last response for any extra
/// requests (e.g. SDK retries). Each response is served with
/// `Connection: close`. Requests with `Expect: 100-continue` get the interim
/// response so the SDK proceeds to send the body without waiting.
///
/// Point s7cmd at it with `--target-endpoint-url` (see [`mock_target_args`]).
/// The SDK uses path-style addressing automatically because the host is an
/// IP literal, so any bucket name works.
pub struct MockS3Server {
    addr: std::net::SocketAddr,
    requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MockS3Server {
    pub fn start(responses: Vec<MockResponse>) -> Self {
        assert!(!responses.is_empty(), "need at least one mock response");
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock S3 server on loopback");
        let addr = listener.local_addr().expect("mock server local_addr");
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let thread_requests = std::sync::Arc::clone(&requests);
        let thread_shutdown = std::sync::Arc::clone(&shutdown);
        let handle = std::thread::spawn(move || {
            let mut served = 0usize;
            for stream in listener.incoming() {
                if thread_shutdown.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let response = &responses[served.min(responses.len() - 1)];
                if serve_one(stream, response, &thread_requests).is_ok() {
                    served += 1;
                }
            }
        });

        Self {
            addr,
            requests,
            shutdown,
            handle: Some(handle),
        }
    }

    pub fn endpoint_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Request lines seen so far, as `"METHOD /path?query"`.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for MockS3Server {
    fn drop(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);
        // Unblock the accept() so the thread observes the flag and exits.
        let _ = std::net::TcpStream::connect(self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Serve a single request on `stream` with `response`. Returns Err if the
/// connection was not a parseable HTTP request (e.g. the wake-up probe from
/// `Drop`), so it isn't counted against the response sequence.
fn serve_one(
    mut stream: std::net::TcpStream,
    response: &MockResponse,
    requests: &std::sync::Mutex<Vec<String>>,
) -> Result<(), ()> {
    use std::io::{Read, Write};

    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .ok();

    // Read the request head (start line + headers).
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            _ => return Err(()), // disconnect/timeout before a full head
        }
        if head.len() > 64 * 1024 {
            return Err(());
        }
    }
    let head_text = String::from_utf8_lossy(&head).to_string();
    let request_line = head_text.split("\r\n").next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Err(());
    };
    requests.lock().unwrap().push(format!("{method} {target}"));

    let header = |name: &str| -> Option<String> {
        head_text.split("\r\n").skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case(name)
                .then(|| v.trim().to_string())
        })
    };

    // Let bodies with Expect: 100-continue proceed immediately.
    if header("expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue")) {
        let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }

    // Drain the request body so the client finishes writing before we
    // respond and close. (The AWS SDK sends sized bodies, not chunked.)
    if let Some(len) = header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        let mut remaining = len;
        let mut buf = [0u8; 4096];
        while remaining > 0 {
            match stream.read(&mut buf[..remaining.min(4096)]) {
                Ok(0) | Err(_) => break,
                Ok(n) => remaining -= n,
            }
        }
    }

    if let Some(delay) = response.delay {
        std::thread::sleep(delay);
    }

    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Mock",
    };
    // HEAD responses must not carry a body.
    let body: &[u8] = if method == "HEAD" {
        b""
    } else {
        &response.body
    };
    let mut out = format!(
        "HTTP/1.1 {} {}\r\ncontent-type: application/xml\r\ncontent-length: {}\r\nx-amz-request-id: mock-request-id\r\nconnection: close\r\n",
        response.status,
        reason,
        body.len()
    );
    for (k, v) in &response.headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
    Ok(())
}

/// The standard client flags for pointing a subcommand at a [`MockS3Server`]:
/// explicit endpoint, static credentials and region (so no profile, IMDS or
/// config-file lookup happens), and a single attempt (no retries), keeping
/// the request↔response sequence 1:1.
pub fn mock_target_args(endpoint_url: &str) -> Vec<String> {
    [
        "--target-endpoint-url",
        endpoint_url,
        "--target-access-key",
        "mock-access-key",
        "--target-secret-access-key",
        "mock-secret-key",
        "--target-region",
        "us-east-1",
        "--aws-max-attempts",
        "1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// [`mock_target_args`] for subcommands whose client flags are
/// `--source-*`-prefixed (e.g. `rename`, which is source-oriented).
pub fn mock_source_args(endpoint_url: &str) -> Vec<String> {
    mock_target_args(endpoint_url)
        .iter()
        .map(|s| s.replace("--target-", "--source-"))
        .collect()
}

/// `s7cmd_cmd()` with ambient environment that could redirect or re-level
/// the run scrubbed away: mock tests must talk only to the mock endpoint
/// and assert on default-verbosity output.
pub fn s7cmd_cmd_clean_env() -> Command {
    let mut cmd = s7cmd_cmd();
    cmd.env_remove("RUST_LOG")
        .env_remove("AWS_PROFILE")
        .env_remove("AWS_ENDPOINT_URL")
        .env_remove("AWS_ENDPOINT_URL_S3");
    cmd
}

// === e2e_test-gated SDK helpers ===

// Re-exported for use by tests/e2e_*.rs files. Test crates that don't
// consume them (e.g. cli_arg_validation.rs uses only s7cmd_cmd/run) make
// these re-exports look unused inside that crate's compilation, so suppress
// the false-positive locally.
#[cfg(e2e_test)]
#[allow(unused_imports)]
pub use e2e::{EXPRESS_ONE_ZONE_AZ, REGION, TestHelper};

#[cfg(e2e_test)]
mod e2e {
    use super::PROFILE_NAME;

    use aws_config::BehaviorVersion;
    use aws_config::meta::region::{ProvideRegion, RegionProviderChain};
    use aws_sdk_s3::Client;
    use aws_sdk_s3::config::Region;
    use aws_sdk_s3::operation::get_object_tagging::GetObjectTaggingOutput;
    use aws_sdk_s3::operation::head_object::HeadObjectOutput;
    use aws_sdk_s3::primitives::ByteStream;
    use aws_sdk_s3::types::{
        BucketInfo, BucketLocationConstraint, BucketType, BucketVersioningStatus,
        CreateBucketConfiguration, DataRedundancy, LocationInfo, LocationType, Tag, Tagging,
        VersioningConfiguration,
    };

    /// Default region. Overridable at runtime (not compile time) via the
    /// `S7CMD_E2E_REGION` environment variable, so a single binary can be
    /// pointed at different accounts/regions without recompiling.
    pub const REGION: &str = "ap-northeast-1";

    /// Availability Zone ID used for Express One Zone directory bucket tests.
    pub const EXPRESS_ONE_ZONE_AZ: &str = "apne1-az4";

    pub struct TestHelper {
        pub client: Client,
    }

    impl TestHelper {
        pub async fn new() -> Self {
            Self {
                client: Self::create_client().await,
            }
        }

        async fn create_client() -> Client {
            let cfg = aws_config::defaults(BehaviorVersion::latest())
                .credentials_provider(
                    aws_config::profile::ProfileFileCredentialsProvider::builder()
                        .profile_name(PROFILE_NAME)
                        .build(),
                )
                .region(Self::region_provider())
                .load()
                .await;
            Client::new(&cfg)
        }

        fn region_provider() -> Box<dyn ProvideRegion> {
            // Order: $S7CMD_E2E_REGION > profile region > REGION constant.
            let env_region = std::env::var("S7CMD_E2E_REGION").ok().map(Region::new);
            let profile_region = aws_config::profile::ProfileFileRegionProvider::builder()
                .profile_name(PROFILE_NAME)
                .build();
            let chain = RegionProviderChain::first_try(env_region)
                .or_else(profile_region)
                .or_else(REGION);
            Box::new(chain)
        }

        // ---- Bucket lifecycle ----

        pub async fn create_bucket(&self, bucket: &str, region: &str) {
            let constraint = BucketLocationConstraint::from(region);
            let cfg = CreateBucketConfiguration::builder()
                .location_constraint(constraint)
                .build();
            self.client
                .create_bucket()
                .create_bucket_configuration(cfg)
                .bucket(bucket)
                .send()
                .await
                .expect("create_bucket");
        }

        pub async fn is_bucket_exist(&self, bucket: &str) -> bool {
            let head = self.client.head_bucket().bucket(bucket).send().await;
            match head {
                Ok(_) => true,
                Err(e) => !e.into_service_error().is_not_found(),
            }
        }

        /// Empty the bucket (objects + versions + delete-markers) and then
        /// delete it. Idempotent on already-gone buckets so cleanup is
        /// safe to call multiple times.
        pub async fn delete_bucket_with_cascade(&self, bucket: &str) {
            if !self.is_bucket_exist(bucket).await {
                return;
            }
            self.delete_all_objects(bucket).await;
            self.delete_all_object_versions(bucket).await;
            let _ = self.client.delete_bucket().bucket(bucket).send().await;
        }

        // ---- Object lifecycle ----

        pub async fn put_object(&self, bucket: &str, key: &str, body: Vec<u8>) {
            self.client
                .put_object()
                .bucket(bucket)
                .key(key)
                .body(body.into())
                .send()
                .await
                .expect("put_object");
        }

        /// PutObject with a `tagging` query parameter. Used by tests that
        /// need to seed tags atomically with the object so they can later
        /// verify --dry-run leaves them untouched.
        pub async fn put_object_with_tagging(
            &self,
            bucket: &str,
            key: &str,
            body: Vec<u8>,
            tagging: &str,
        ) {
            self.client
                .put_object()
                .bucket(bucket)
                .key(key)
                .tagging(tagging)
                .body(body.into())
                .send()
                .await
                .expect("put_object_with_tagging");
        }

        /// Fetch an object's body and return the raw bytes. Used by
        /// process-level e2e tests to verify upload/download round-trips.
        pub async fn get_object_bytes(
            &self,
            bucket: &str,
            key: &str,
            version_id: Option<String>,
        ) -> Vec<u8> {
            let out = self
                .client
                .get_object()
                .bucket(bucket)
                .key(key)
                .set_version_id(version_id)
                .send()
                .await
                .expect("get_object");
            out.body
                .collect()
                .await
                .expect("collect body")
                .into_bytes()
                .to_vec()
        }

        /// Fetch the tag set for an object. Used by --dry-run tests to
        /// confirm that put-/delete-object-tagging dry-runs leave the
        /// existing tag set unchanged.
        pub async fn get_object_tagging(
            &self,
            bucket: &str,
            key: &str,
            version_id: Option<String>,
        ) -> GetObjectTaggingOutput {
            self.client
                .get_object_tagging()
                .bucket(bucket)
                .key(key)
                .set_version_id(version_id)
                .send()
                .await
                .expect("get_object_tagging")
        }

        /// PutObject and return the assigned `VersionId`. The bucket must
        /// have versioning enabled, otherwise S3 returns `null` or no value.
        pub async fn put_object_returning_version_id(
            &self,
            bucket: &str,
            key: &str,
            body: Vec<u8>,
        ) -> String {
            let out = self
                .client
                .put_object()
                .bucket(bucket)
                .key(key)
                .body(body.into())
                .send()
                .await
                .expect("put_object");
            out.version_id()
                .expect("PutObject must return VersionId on a versioned bucket")
                .to_string()
        }

        pub async fn is_object_exist(
            &self,
            bucket: &str,
            key: &str,
            version_id: Option<String>,
        ) -> bool {
            let req = self
                .client
                .head_object()
                .bucket(bucket)
                .key(key)
                .set_version_id(version_id);
            match req.send().await {
                Ok(_) => true,
                Err(e) => !e.into_service_error().is_not_found(),
            }
        }

        pub async fn delete_object(&self, bucket: &str, key: &str, version_id: Option<String>) {
            self.client
                .delete_object()
                .bucket(bucket)
                .key(key)
                .set_version_id(version_id)
                .send()
                .await
                .expect("delete_object");
        }

        pub async fn delete_all_objects(&self, bucket: &str) {
            let Ok(list) = self.client.list_objects_v2().bucket(bucket).send().await else {
                return;
            };
            for obj in list.contents() {
                if let Some(key) = obj.key() {
                    self.delete_object(bucket, key, None).await;
                }
            }
        }

        /// Delete every non-current version and delete-marker. Versioning
        /// not enabled / list unsupported (e.g. directory bucket) → no-op.
        pub async fn delete_all_object_versions(&self, bucket: &str) {
            let Ok(out) = self
                .client
                .list_object_versions()
                .bucket(bucket)
                .send()
                .await
            else {
                return;
            };
            for v in out.versions() {
                if let Some(key) = v.key() {
                    self.delete_object(bucket, key, v.version_id().map(str::to_string))
                        .await;
                }
            }
            for m in out.delete_markers() {
                if let Some(key) = m.key() {
                    self.delete_object(bucket, key, m.version_id().map(str::to_string))
                        .await;
                }
            }
        }

        // ---- Seeding helpers (set state that get-*/delete-* tests read) ----

        pub async fn put_object_tagging(&self, bucket: &str, key: &str, tags: &[(&str, &str)]) {
            let tag_set: Vec<Tag> = tags
                .iter()
                .map(|(k, v)| Tag::builder().key(*k).value(*v).build().unwrap())
                .collect();
            let tagging = Tagging::builder()
                .set_tag_set(Some(tag_set))
                .build()
                .unwrap();
            self.client
                .put_object_tagging()
                .bucket(bucket)
                .key(key)
                .tagging(tagging)
                .send()
                .await
                .expect("put_object_tagging");
        }

        /// Attach an annotation to an object via the raw SDK, so
        /// get-/list-/delete-object-annotation tests have state to read. The
        /// SDK adds a default (CRC32) request checksum automatically, so no
        /// explicit checksum/Content-MD5 is needed here.
        pub async fn put_object_annotation(
            &self,
            bucket: &str,
            key: &str,
            version_id: Option<String>,
            annotation_name: &str,
            payload: &[u8],
        ) {
            self.client
                .put_object_annotation()
                .bucket(bucket)
                .key(key)
                .set_version_id(version_id)
                .annotation_name(annotation_name)
                .annotation_payload(ByteStream::from(payload.to_vec()))
                .send()
                .await
                .expect("put_object_annotation");
        }

        /// Fetch an annotation's payload bytes. Used to verify a
        /// put-object-annotation round-trip landed the expected bytes.
        pub async fn get_object_annotation_payload(
            &self,
            bucket: &str,
            key: &str,
            version_id: Option<String>,
            annotation_name: &str,
        ) -> Vec<u8> {
            let out = self
                .client
                .get_object_annotation()
                .bucket(bucket)
                .key(key)
                .set_version_id(version_id)
                .annotation_name(annotation_name)
                .send()
                .await
                .expect("get_object_annotation");
            out.annotation_payload
                .collect()
                .await
                .expect("collect annotation payload")
                .into_bytes()
                .to_vec()
        }

        /// Whether an annotation with the given name exists on the object.
        /// Any error (including `NoSuchAnnotation`) maps to `false` — used to
        /// confirm delete-object-annotation removed the annotation.
        pub async fn is_object_annotation_exist(
            &self,
            bucket: &str,
            key: &str,
            version_id: Option<String>,
            annotation_name: &str,
        ) -> bool {
            self.client
                .get_object_annotation()
                .bucket(bucket)
                .key(key)
                .set_version_id(version_id)
                .annotation_name(annotation_name)
                .send()
                .await
                .is_ok()
        }

        pub async fn put_bucket_tagging(&self, bucket: &str, tags: &[(&str, &str)]) {
            let tag_set: Vec<Tag> = tags
                .iter()
                .map(|(k, v)| Tag::builder().key(*k).value(*v).build().unwrap())
                .collect();
            let tagging = Tagging::builder()
                .set_tag_set(Some(tag_set))
                .build()
                .unwrap();
            self.client
                .put_bucket_tagging()
                .bucket(bucket)
                .tagging(tagging)
                .send()
                .await
                .expect("put_bucket_tagging");
        }

        pub async fn put_bucket_policy(&self, bucket: &str, policy_json: &str) {
            self.client
                .put_bucket_policy()
                .bucket(bucket)
                .policy(policy_json)
                .send()
                .await
                .expect("put_bucket_policy");
        }

        pub async fn enable_bucket_versioning(&self, bucket: &str) {
            let cfg = VersioningConfiguration::builder()
                .status(BucketVersioningStatus::Enabled)
                .build();
            self.client
                .put_bucket_versioning()
                .bucket(bucket)
                .versioning_configuration(cfg)
                .send()
                .await
                .expect("enable_bucket_versioning");
        }

        // ---- Multipart cleanup (Ctrl+C tests for cp/mv) ----

        pub async fn abort_all_multipart_uploads(&self, bucket: &str) {
            let Ok(list) = self
                .client
                .list_multipart_uploads()
                .bucket(bucket)
                .send()
                .await
            else {
                return;
            };
            for upload in list.uploads() {
                if let (Some(key), Some(upload_id)) = (upload.key(), upload.upload_id()) {
                    let _ = self
                        .client
                        .abort_multipart_upload()
                        .bucket(bucket)
                        .key(key)
                        .upload_id(upload_id)
                        .send()
                        .await;
                }
            }
        }

        // ---- Express One Zone directory bucket helpers (rename tests) ----

        pub async fn create_directory_bucket(&self, bucket_name: &str, availability_zone: &str) {
            let location_info = LocationInfo::builder()
                .r#type(LocationType::AvailabilityZone)
                .name(availability_zone)
                .build();
            let bucket_info = BucketInfo::builder()
                .data_redundancy(DataRedundancy::SingleAvailabilityZone)
                .r#type(BucketType::Directory)
                .build();
            let configuration = CreateBucketConfiguration::builder()
                .location(location_info)
                .bucket(bucket_info)
                .build();
            self.client
                .create_bucket()
                .create_bucket_configuration(configuration)
                .bucket(bucket_name)
                .send()
                .await
                .expect("create_directory_bucket");
        }

        /// HeadObject and return the full output. Used by rename tests to
        /// obtain the ETag for conditional-check flags.
        pub async fn head_object(
            &self,
            bucket: &str,
            key: &str,
            version_id: Option<String>,
        ) -> HeadObjectOutput {
            self.client
                .head_object()
                .bucket(bucket)
                .key(key)
                .set_version_id(version_id)
                .send()
                .await
                .expect("head_object")
        }

        /// Empty a directory bucket (no versioning support) and delete it.
        /// Idempotent: no-op when the bucket does not exist.
        pub async fn delete_directory_bucket_with_cascade(&self, bucket: &str) {
            if !self.is_bucket_exist(bucket).await {
                return;
            }
            self.delete_all_objects(bucket).await;
            let _ = self.client.delete_bucket().bucket(bucket).send().await;
        }
    }
}
