//! Process-level regression tests for SIGINT (Ctrl+C) exit-code handling
//! in the pipeline subcommands (`ls`, `clean`, `sync`, `cp`, `mv`) and in
//! `batch-run`.
//!
//! Each of these subcommands catches Ctrl+C, cancels its pipeline, and must
//! then exit gracefully with code 130 (128 + SIGINT), the conventional shell
//! encoding for a run interrupted by the user. These tests run the real
//! binary against a minimal in-process S3 endpoint (no AWS access needed):
//! the endpoint keeps returning truncated list pages so the run stays busy
//! until the test sends SIGINT, and answers object `GET` (for `sync`
//! downloads), batch `DeleteObjects`, and single-object `DeleteObject`
//! requests so the later pipeline stages stay active as well. For `cp`/`mv`
//! the endpoint instead serves an object whose body stalls after a small
//! prefix, so the interrupt arrives while the transfer is blocked
//! mid-download.
//!
//! Covers the `is_ctrl_c_received()` → `SIGINT_EXIT_CODE` paths in
//! `src/ls_bin/mod.rs`, `src/clean_bin/mod.rs`, and
//! `src/sync_bin/cli/mod.rs` (ported from s3rm-rs 1.6.0 / s3ls-rs 1.3.0;
//! upstream s3sync adopted the same fix in 1.62.0), the
//! `is_ctrl_c_received()`-over-shutdown-error precedence in
//! `src/util_bin/cli/mod.rs::run_copy_phase` (ported from s3util-rs
//! 1.10.0), plus batch-run's severity-ranked aggregation of a line that
//! returns 130.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BUCKET: &str = "fake-sigint-bucket";

/// One-byte object body served for every object `GET`; the listing
/// advertises `Size` 1 and the matching MD5 ETag so `sync` downloads it
/// without complaint.
const OBJECT_BODY: &str = "x";
const OBJECT_ETAG: &str = "\"9dd4e461268c8034f5c8564e155c67a6\"";

/// Size advertised for the stalled object (see
/// [`spawn_fake_s3_stalled_objects`]): large enough that the child is
/// guaranteed to still be reading the body when SIGINT arrives, small
/// enough to stay below any multipart-download threshold so the transfer
/// is a single `GET`.
const STALLED_OBJECT_SIZE: usize = 1024 * 1024;
/// Body bytes actually written before the response stalls.
const STALLED_OBJECT_PREFIX: usize = 1024;

/// Handle to a fake S3 endpoint running on a background thread.
///
/// The fields below marked `cfg_attr(..., allow(dead_code))` are read only
/// by the unix-gated SIGINT tests; without the attribute they warn as
/// never-read on Windows, where only the no-SIGINT control tests run.
struct FakeS3 {
    endpoint: String,
    pages_served: Arc<AtomicUsize>,
    #[cfg_attr(not(target_family = "unix"), allow(dead_code))]
    object_gets_served: Arc<AtomicUsize>,
    deletes_served: Arc<AtomicUsize>,
    deleted_keys: Arc<Mutex<Vec<String>>>,
    #[cfg_attr(not(target_family = "unix"), allow(dead_code))]
    annotation_pages_served: Arc<AtomicUsize>,
    /// While false, `ListObjectAnnotations` responses carry a continuation
    /// token (endless listing); set to true to make the next response the
    /// final page. The annotation listing loop is deliberately
    /// non-cancellable in the library, so tests end it from the outside.
    #[cfg_attr(not(target_family = "unix"), allow(dead_code))]
    finish_annotations: Arc<AtomicBool>,
}

/// Serve canned S3 responses over plain HTTP/1.1, one request per
/// connection, serialized by the accept loop:
///
/// - `GET ?versioning` → versioning enabled
/// - `GET ?list-type=2` (ListObjectsV2) → one page with one object,
///   advancing its continuation token each time. With
///   `total_pages = Some(n)` the n-th page is final (`IsTruncated` false);
///   with `None` the listing is endless, so the child only stops when it
///   is signalled.
/// - `GET` / `HEAD` on an object key → the one-byte object (for `sync`)
/// - `POST ?delete` (batch `DeleteObjects`) → echo every requested key
///   back as `<Deleted>`
/// - `DELETE` (single `DeleteObject`) → 204 No Content
fn spawn_fake_s3(total_pages: Option<usize>) -> FakeS3 {
    spawn_fake_s3_impl(total_pages, false)
}

/// [`spawn_fake_s3`] variant for the `cp`/`mv` SIGINT tests: object `HEAD`
/// advertises a [`STALLED_OBJECT_SIZE`]-byte object, and object `GET`
/// writes only a [`STALLED_OBJECT_PREFIX`]-byte body prefix and then holds
/// the connection open — so the child is provably blocked mid-download
/// when the test sends SIGINT, and the forced shutdown that follows
/// surfaces whatever error the aborted body read produces.
///
/// Called only by the unix-gated SIGINT tests, hence the Windows
/// dead-code allowance.
#[cfg_attr(not(target_family = "unix"), allow(dead_code))]
fn spawn_fake_s3_stalled_objects() -> FakeS3 {
    spawn_fake_s3_impl(None, true)
}

fn spawn_fake_s3_impl(total_pages: Option<usize>, stall_object_bodies: bool) -> FakeS3 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind fake S3 listener");
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let pages_served = Arc::new(AtomicUsize::new(0));
    let object_gets_served = Arc::new(AtomicUsize::new(0));
    let deletes_served = Arc::new(AtomicUsize::new(0));
    let deleted_keys = Arc::new(Mutex::new(Vec::new()));
    let annotation_pages_served = Arc::new(AtomicUsize::new(0));
    let finish_annotations = Arc::new(AtomicBool::new(false));

    let pages = Arc::clone(&pages_served);
    let gets = Arc::clone(&object_gets_served);
    let deletes = Arc::clone(&deletes_served);
    let keys = Arc::clone(&deleted_keys);
    let annotation_pages = Arc::clone(&annotation_pages_served);
    let finish_ann = Arc::clone(&finish_annotations);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let Some((request_line, request_body)) = read_request(&mut stream) else {
                continue;
            };

            if stall_object_bodies && is_object_request(&request_line) {
                let head_only = request_line.starts_with("HEAD");
                if !head_only {
                    gets.fetch_add(1, Ordering::SeqCst);
                }
                let _ = stream.write_all(&stalled_object_response(head_only));
                let _ = stream.flush();
                if !head_only {
                    hold_until_peer_closes(&mut stream);
                }
                continue;
            }

            let response = route_request(
                &request_line,
                &request_body,
                total_pages,
                &pages,
                &gets,
                &deletes,
                &keys,
                &annotation_pages,
                &finish_ann,
            );
            let _ = stream.write_all(&response);
            let _ = stream.flush();
        }
    });

    FakeS3 {
        endpoint,
        pages_served,
        object_gets_served,
        deletes_served,
        deleted_keys,
        annotation_pages_served,
        finish_annotations,
    }
}

/// Whether the request is an object `GET`/`HEAD` (as opposed to a bucket
/// subresource query like `?versioning` or a `?list-type=2` listing).
fn is_object_request(request_line: &str) -> bool {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    (method == "GET" || method == "HEAD")
        && !has_query_param(target, "versioning")
        && !has_query_param(target, "list-type")
        && !has_query_param(target, "annotation")
}

/// Response for the stalled object: full headers advertising
/// [`STALLED_OBJECT_SIZE`] bytes, but (for `GET`) only a
/// [`STALLED_OBJECT_PREFIX`]-byte body prefix — the caller then keeps the
/// connection open so the client blocks waiting for the rest.
fn stalled_object_response(head_only: bool) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: {STALLED_OBJECT_SIZE}\r\netag: \"fakestalledobjectetag\"\r\nlast-modified: Thu, 01 Jan 2026 00:00:00 GMT\r\naccept-ranges: bytes\r\nconnection: close\r\n\r\n",
    )
    .into_bytes();
    if !head_only {
        response.extend(std::iter::repeat_n(b'x', STALLED_OBJECT_PREFIX));
    }
    response
}

/// Hold the connection open without sending further body bytes until the
/// peer closes it (the child exited) or the socket read times out. Blocks
/// the accept loop, which is fine: the stalled `GET` is the last request a
/// `cp`/`mv` run issues before it is interrupted.
fn hold_until_peer_closes(stream: &mut TcpStream) {
    let mut discard = [0u8; 512];
    loop {
        match stream.read(&mut discard) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

/// Read one HTTP request (head plus `content-length` body) and return its
/// request line and body.
fn read_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let header_end = loop {
        match stream.read(&mut buf) {
            Ok(0) => return None,
            Ok(n) => {
                data.extend_from_slice(&buf[..n]);
                if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    break pos + 4;
                }
            }
            Err(_) => return None,
        }
    };

    let head = String::from_utf8_lossy(&data[..header_end]).to_string();
    let request_line = head.lines().next().unwrap_or("").to_string();

    let mut content_length = 0usize;
    for line in head.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
    while data.len() - header_end < content_length {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
            Err(_) => break,
        }
    }

    let body = String::from_utf8_lossy(&data[header_end..]).to_string();
    Some((request_line, body))
}

#[allow(clippy::too_many_arguments)]
fn route_request(
    request_line: &str,
    request_body: &str,
    total_pages: Option<usize>,
    pages_served: &AtomicUsize,
    object_gets_served: &AtomicUsize,
    deletes_served: &AtomicUsize,
    deleted_keys: &Mutex<Vec<String>>,
    annotation_pages_served: &AtomicUsize,
    finish_annotations: &AtomicBool,
) -> Vec<u8> {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");

    match method {
        // GetBucketVersioning (prerequisite checks). Must not be confused
        // with the `versions` (ListObjectVersions) query parameter.
        "GET" if has_query_param(target, "versioning") => {
            xml_response("200 OK", &versioning_enabled_page())
        }
        // ListObjectAnnotations (cp/mv annotation sync; `?annotation` with
        // `x-id=ListObjectAnnotations`). Pages are endless until
        // `finish_annotations` is set, keeping the child inside the
        // library's (deliberately non-cancellable) listing loop until the
        // test decides to let it finish.
        "GET" if has_query_param(target, "annotation") => {
            let page = annotation_pages_served.fetch_add(1, Ordering::SeqCst);
            let truncated = !finish_annotations.load(Ordering::SeqCst);
            xml_response("200 OK", &annotations_page(page, truncated))
        }
        // ListObjectsV2. Every object `GET` the AWS SDK issues carries
        // `x-id=GetObject` but never `list-type`, so this param is the
        // discriminator.
        "GET" if has_query_param(target, "list-type") => {
            let page = pages_served.fetch_add(1, Ordering::SeqCst);
            let truncated = total_pages.is_none_or(|total| page + 1 < total);
            xml_response("200 OK", &objects_page(page, truncated))
        }
        // Object GET / HEAD (sync's download stage).
        "GET" | "HEAD" => {
            if method == "GET" {
                object_gets_served.fetch_add(1, Ordering::SeqCst);
            }
            object_response(method == "HEAD")
        }
        // Batch DeleteObjects: acknowledge every requested key as deleted.
        "POST" if has_query_param(target, "delete") => {
            let requested = parse_delete_request(request_body);
            {
                let mut recorded = deleted_keys.lock().unwrap();
                recorded.extend(requested.iter().cloned());
            }
            deletes_served.fetch_add(1, Ordering::SeqCst);
            xml_response("200 OK", &delete_result_page(&requested))
        }
        // Single-object DeleteObject.
        "DELETE" => {
            let path = target.split('?').next().unwrap_or("");
            let key = path.rsplit('/').next().unwrap_or("").to_string();
            deleted_keys.lock().unwrap().push(key);
            deletes_served.fetch_add(1, Ordering::SeqCst);
            b"HTTP/1.1 204 No Content\r\nconnection: close\r\n\r\n".to_vec()
        }
        _ => xml_response("400 Bad Request", ""),
    }
}

fn xml_response(status_line: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status_line}\r\ncontent-type: application/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len(),
    )
    .into_bytes()
}

/// The one-byte object every listing page advertises. Headers match the
/// listing entry (size 1, MD5 ETag) so `sync` writes it without complaint.
fn object_response(head_only: bool) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: {}\r\netag: {OBJECT_ETAG}\r\nlast-modified: Thu, 01 Jan 2026 00:00:00 GMT\r\naccept-ranges: bytes\r\nconnection: close\r\n\r\n",
        OBJECT_BODY.len(),
    )
    .into_bytes();
    if !head_only {
        response.extend_from_slice(OBJECT_BODY.as_bytes());
    }
    response
}

/// Whether the request target carries the query parameter `name`
/// (`?versioning` matches `versioning` but not `versions`).
fn has_query_param(target: &str, name: &str) -> bool {
    target
        .split_once('?')
        .map(|(_, query)| {
            query
                .split('&')
                .any(|param| param.split('=').next() == Some(name))
        })
        .unwrap_or(false)
}

/// Extract the keys from a `DeleteObjects` request body.
fn parse_delete_request(body: &str) -> Vec<String> {
    body.split("<Object>")
        .skip(1)
        .map(|chunk| {
            let object = chunk.split("</Object>").next().unwrap_or(chunk);
            extract_tag(object, "Key").unwrap_or_default()
        })
        .collect()
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].to_string())
}

/// One `ListObjectAnnotations` page carrying a single annotation entry,
/// truncated (continuation token present) or final. Shape mirrors the
/// deserializer's expectations as pinned by s3util-rs's own paging tests.
fn annotations_page(page: usize, truncated: bool) -> String {
    let next_token = if truncated {
        format!("<NextContinuationToken>ann-token-{page}</NextContinuationToken>")
    } else {
        String::new()
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ListObjectAnnotationsOutput><Annotations><AnnotationEntry><AnnotationName>note-{page}</AnnotationName><LastModified>2026-01-01T00:00:00.000Z</LastModified><Size>1</Size></AnnotationEntry></Annotations>{next_token}</ListObjectAnnotationsOutput>"#
    )
}

fn versioning_enabled_page() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Status>Enabled</Status></VersioningConfiguration>"#
        .to_string()
}

fn objects_page(page: usize, truncated: bool) -> String {
    let next_token = if truncated {
        format!("<NextContinuationToken>token-{page}</NextContinuationToken>")
    } else {
        String::new()
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>{BUCKET}</Name><Prefix></Prefix><KeyCount>1</KeyCount><MaxKeys>1000</MaxKeys><IsTruncated>{truncated}</IsTruncated>{next_token}<Contents><Key>page-{page}.txt</Key><LastModified>2026-01-01T00:00:00.000Z</LastModified><ETag>&quot;9dd4e461268c8034f5c8564e155c67a6&quot;</ETag><Size>1</Size><StorageClass>STANDARD</StorageClass></Contents></ListBucketResult>"#
    )
}

fn delete_result_page(deleted: &[String]) -> String {
    let mut entries = String::new();
    for key in deleted {
        entries.push_str(&format!("<Deleted><Key>{key}</Key></Deleted>"));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<DeleteResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">{entries}</DeleteResult>"#
    )
}

/// `--target-*` client flags pointing at the fake endpoint with static
/// credentials, so no profile, IMDS, or config-file lookup happens and no
/// retries blur the request↔response accounting.
fn target_client_args(endpoint: &str) -> Vec<String> {
    [
        "--target-endpoint-url",
        endpoint,
        "--target-access-key",
        "fake-access-key",
        "--target-secret-access-key",
        "fake-secret-key",
        "--target-region",
        "us-east-1",
        "--aws-max-attempts",
        "1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// [`target_client_args`] with `--source-*` prefixes (for `sync`, whose S3
/// side in these tests is the source).
fn source_client_args(endpoint: &str) -> Vec<String> {
    target_client_args(endpoint)
        .iter()
        .map(|s| s.replace("--target-", "--source-"))
        .collect()
}

/// Spawn the s7cmd binary with ambient environment that could redirect the
/// run scrubbed away (same scrub set as `common::s7cmd_cmd_clean_env`), so
/// the child talks only to the fake endpoint.
fn spawn_s7cmd(args: &[String]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_s7cmd"))
        .args(args)
        .env_remove("RUST_LOG")
        .env_remove("AWS_PROFILE")
        .env_remove("AWS_ENDPOINT_URL")
        .env_remove("AWS_ENDPOINT_URL_S3")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn s7cmd")
}

/// Fresh per-test scratch directory (used as sync's local target).
fn create_temp_dir(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "s7cmd_sigint_{label}_{}_{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Wait until the fake endpoint has served at least `at_least` requests
/// counted by `counter`, so the child is provably inside the pipeline (its
/// Ctrl+C handler is installed before the pipeline starts, thus long since
/// registered). Fails fast with the child's stderr if it exits early.
#[cfg(target_family = "unix")]
fn wait_for_count(child: &mut Child, counter: &AtomicUsize, at_least: usize, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while counter.load(Ordering::SeqCst) < at_least {
        if let Some(status) = child.try_wait().expect("failed to poll s7cmd") {
            let stderr = read_stderr(child);
            panic!(
                "s7cmd exited ({status:?}) before the fake S3 served {at_least} {what}\nstderr: {stderr}"
            );
        }
        assert!(
            Instant::now() < deadline,
            "fake S3 served only {} {what} before timeout",
            counter.load(Ordering::SeqCst)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Wait for the child to exit within `deadline` — SIGINT must terminate
/// the process promptly, not hang it. Kills the child on timeout.
fn wait_with_deadline(child: &mut Child, deadline: Duration) -> std::process::ExitStatus {
    let end = Instant::now() + deadline;
    loop {
        if let Some(status) = child.try_wait().expect("failed to poll s7cmd") {
            return status;
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            panic!("s7cmd did not exit within {deadline:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Read the child's stderr after it has exited.
fn read_stderr(child: &mut Child) -> String {
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    stderr
}

#[cfg(target_family = "unix")]
fn send_sigint(child: &Child) {
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGINT,
    )
    .expect("failed to send SIGINT to s7cmd");
}

/// Interrupt the child and require a graceful exit with code 130 —
/// `code()` is `None` for a raw signal kill, so this also proves the
/// signal was caught rather than terminating the process directly — and a
/// stderr free of panics and terminal failure logs.
#[cfg(target_family = "unix")]
fn interrupt_and_expect_130(mut child: Child, label: &str) {
    send_sigint(&child);

    let status = wait_with_deadline(&mut child, Duration::from_secs(15));
    let stderr = read_stderr(&mut child);

    assert_eq!(
        status.code(),
        Some(130),
        "[{label}] expected graceful exit 130 after SIGINT, got {status:?}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("panicked") && !stderr.contains("failed."),
        "[{label}] SIGINT should terminate without failure logs\nstderr: {stderr}"
    );
}

// ---- ls ----

/// Ctrl+C during a recursive listing (parallel-listing dispatch).
#[test]
#[cfg(target_family = "unix")]
fn sigint_during_ls_recursive_listing_exits_130() {
    let fake = spawn_fake_s3(None);
    let mut args = vec!["ls".to_string(), "--recursive".to_string()];
    args.extend(target_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/"));
    let mut child = spawn_s7cmd(&args);

    wait_for_count(&mut child, &fake.pages_served, 3, "list page(s)");
    interrupt_and_expect_130(child, "ls-recursive");
}

/// Ctrl+C during the default (delimiter, sequential) listing.
#[test]
#[cfg(target_family = "unix")]
fn sigint_during_ls_delimiter_listing_exits_130() {
    let fake = spawn_fake_s3(None);
    let mut args = vec!["ls".to_string()];
    args.extend(target_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/"));
    let mut child = spawn_s7cmd(&args);

    wait_for_count(&mut child, &fake.pages_served, 3, "list page(s)");
    interrupt_and_expect_130(child, "ls-delimiter");
}

// ---- clean ----

/// Ctrl+C during a `--dry-run` listing: only list requests are ever in
/// flight, and none may turn into deletions during shutdown.
#[test]
#[cfg(target_family = "unix")]
fn sigint_during_clean_dry_run_listing_exits_130() {
    let fake = spawn_fake_s3(None);
    let mut args = vec![
        "clean".to_string(),
        "--dry-run".to_string(),
        "--max-parallel-listings".to_string(),
        "1".to_string(),
    ];
    args.extend(target_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/"));
    let mut child = spawn_s7cmd(&args);

    wait_for_count(&mut child, &fake.pages_served, 3, "list page(s)");
    interrupt_and_expect_130(child, "clean-dry-run");

    assert_eq!(
        fake.deletes_served.load(Ordering::SeqCst),
        0,
        "a dry run must never send delete requests"
    );
}

/// Ctrl+C while batch `DeleteObjects` requests are actively being sent
/// (`--batch-size 2` flushes a batch every two listed objects).
#[test]
#[cfg(target_family = "unix")]
fn sigint_during_clean_batch_deletion_exits_130() {
    let fake = spawn_fake_s3(None);
    let mut args = vec![
        "clean".to_string(),
        "--force".to_string(),
        "--batch-size".to_string(),
        "2".to_string(),
        "--max-parallel-listings".to_string(),
        "1".to_string(),
    ];
    args.extend(target_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/"));
    let mut child = spawn_s7cmd(&args);

    wait_for_count(&mut child, &fake.pages_served, 3, "list page(s)");
    wait_for_count(&mut child, &fake.deletes_served, 1, "delete request(s)");
    interrupt_and_expect_130(child, "clean-batch");
}

/// Ctrl+C while single-object `DeleteObject` requests (`--batch-size 1`)
/// are actively being sent.
#[test]
#[cfg(target_family = "unix")]
fn sigint_during_clean_single_object_deletion_exits_130() {
    let fake = spawn_fake_s3(None);
    let mut args = vec![
        "clean".to_string(),
        "--force".to_string(),
        "--batch-size".to_string(),
        "1".to_string(),
        "--max-parallel-listings".to_string(),
        "1".to_string(),
    ];
    args.extend(target_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/"));
    let mut child = spawn_s7cmd(&args);

    wait_for_count(&mut child, &fake.pages_served, 3, "list page(s)");
    wait_for_count(&mut child, &fake.deletes_served, 2, "delete request(s)");
    interrupt_and_expect_130(child, "clean-single");
}

// ---- sync ----

/// Ctrl+C during a `--dry-run` sync listing: only list requests are ever
/// in flight, and no object may be downloaded during shutdown.
#[test]
#[cfg(target_family = "unix")]
fn sigint_during_sync_dry_run_listing_exits_130() {
    let fake = spawn_fake_s3(None);
    let local_dir = create_temp_dir("sync_dry_run");
    let mut args = vec!["sync".to_string(), "--dry-run".to_string()];
    args.extend(source_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/"));
    args.push(format!("{}/", local_dir.to_string_lossy()));
    let mut child = spawn_s7cmd(&args);

    wait_for_count(&mut child, &fake.pages_served, 3, "list page(s)");
    interrupt_and_expect_130(child, "sync-dry-run");

    assert_eq!(
        fake.object_gets_served.load(Ordering::SeqCst),
        0,
        "a dry run must never download objects"
    );
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Ctrl+C while `sync` is actively downloading objects (list pages and
/// object `GET`s both in flight).
#[test]
#[cfg(target_family = "unix")]
fn sigint_during_sync_download_exits_130() {
    let fake = spawn_fake_s3(None);
    let local_dir = create_temp_dir("sync_download");
    let mut args = vec!["sync".to_string()];
    args.extend(source_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/"));
    args.push(format!("{}/", local_dir.to_string_lossy()));
    let mut child = spawn_s7cmd(&args);

    wait_for_count(&mut child, &fake.pages_served, 3, "list page(s)");
    wait_for_count(&mut child, &fake.object_gets_served, 2, "object GET(s)");
    interrupt_and_expect_130(child, "sync-download");

    let _ = std::fs::remove_dir_all(&local_dir);
}

// ---- cp / mv ----

/// Ctrl+C while `cp` is blocked mid-download. The stalled body means the
/// forced shutdown surfaces whatever error the aborted read produces (not
/// necessarily a clean `Cancelled`), and the interruption must still win:
/// exit 130, no failure report — the `is_ctrl_c_received()` precedence in
/// `run_copy_phase`, ported from s3util-rs 1.10.0.
#[test]
#[cfg(target_family = "unix")]
fn sigint_during_cp_download_exits_130() {
    let fake = spawn_fake_s3_stalled_objects();
    let local_dir = create_temp_dir("cp_download");
    let mut args = vec!["cp".to_string()];
    args.extend(source_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/stalled-object.bin"));
    args.push(format!("{}/", local_dir.to_string_lossy()));
    let mut child = spawn_s7cmd(&args);

    wait_for_count(&mut child, &fake.object_gets_served, 1, "object GET(s)");
    interrupt_and_expect_130(child, "cp-download");

    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Ctrl+C while `mv` is blocked mid-download: same 130 guarantee as `cp`,
/// plus the safety half of the contract — an interrupted `mv` must never
/// have deleted its source object.
#[test]
#[cfg(target_family = "unix")]
fn sigint_during_mv_download_exits_130_and_never_deletes_source() {
    let fake = spawn_fake_s3_stalled_objects();
    let local_dir = create_temp_dir("mv_download");
    let mut args = vec!["mv".to_string()];
    args.extend(source_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/stalled-object.bin"));
    args.push(format!("{}/", local_dir.to_string_lossy()));
    let mut child = spawn_s7cmd(&args);

    wait_for_count(&mut child, &fake.object_gets_served, 1, "object GET(s)");
    interrupt_and_expect_130(child, "mv-download");

    assert_eq!(
        fake.deletes_served.load(Ordering::SeqCst),
        0,
        "an interrupted mv must never delete the source object; deleted: {:?}",
        fake.deleted_keys.lock().unwrap()
    );
    let _ = std::fs::remove_dir_all(&local_dir);
}

/// Ctrl+C during a `cp --dry-run` whose S3-to-S3 annotation listing is
/// taking real network time. The dry-run path runs no transfer, but it must
/// still install the Ctrl+C handler and report the interruption as a
/// cancellation — before this fix the dry-run early return in
/// `run_copy_phase` installed no handler and hard-coded `cancelled: false`,
/// so inside batch-run the line would have finished as a success (upstream
/// s3util-rs routes dry-run through the full flag-consulting phase);
/// standalone, the process died to the default disposition instead of
/// exiting gracefully.
#[test]
#[cfg(target_family = "unix")]
fn sigint_during_cp_dry_run_annotation_listing_exits_130() {
    let fake = spawn_fake_s3(None);
    let mut args = vec![
        "cp".to_string(),
        "--dry-run".to_string(),
        "--enable-sync-object-annotations".to_string(),
    ];
    args.extend(source_client_args(&fake.endpoint));
    // `--aws-max-attempts` is a shared (not per-side) flag and is already
    // supplied by the source set above; drop it from the target set (its
    // last two elements) to avoid a duplicate-argument clap error.
    let mut target_args = target_client_args(&fake.endpoint);
    target_args.truncate(target_args.len() - 2);
    args.extend(target_args);
    args.push(format!("s3://{BUCKET}/annotated-src.bin"));
    args.push(format!("s3://{BUCKET}/annotated-dst.bin"));
    let mut child = spawn_s7cmd(&args);

    wait_for_count(
        &mut child,
        &fake.annotation_pages_served,
        3,
        "annotation page(s)",
    );
    send_sigint(&child);
    // The library's annotation listing loop deliberately ignores
    // cancellation (annotation integrity), so end the listing from the
    // fake's side: the next page is final, and the frontend must then
    // report the interruption rather than the listing's success.
    fake.finish_annotations.store(true, Ordering::SeqCst);

    let status = wait_with_deadline(&mut child, Duration::from_secs(15));
    let stderr = read_stderr(&mut child);

    assert_eq!(
        status.code(),
        Some(130),
        "[cp-dry-run] expected graceful exit 130 after SIGINT, got {status:?}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("panicked") && !stderr.contains("failed."),
        "[cp-dry-run] SIGINT should terminate without failure logs\nstderr: {stderr}"
    );
    assert_eq!(
        fake.object_gets_served.load(Ordering::SeqCst),
        0,
        "a dry run must never download objects"
    );
}

// ---- batch-run ----

/// A batch line interrupted by Ctrl+C: the `clean` line's cancellation
/// handler makes it return 130 in-process (batch-run must survive — the
/// engines return the code instead of calling `process::exit`), the
/// executor buckets it as skipped, and the severity-ranked batch exit code
/// surfaces 130 (any-other-non-zero outranks 0).
#[test]
#[cfg(target_family = "unix")]
fn batch_run_clean_line_interrupted_by_sigint_exits_130() {
    let fake = spawn_fake_s3(None);
    let script_dir = create_temp_dir("batch_run");
    let script_path = script_dir.join("script.txt");
    let line = format!(
        "clean --force --batch-size 2 --max-parallel-listings 1 {} s3://{BUCKET}/",
        target_client_args(&fake.endpoint).join(" "),
    );
    std::fs::write(&script_path, format!("{line}\n")).expect("write batch script");

    let args = vec![
        "batch-run".to_string(),
        script_path.to_string_lossy().into_owned(),
    ];
    let mut child = spawn_s7cmd(&args);

    wait_for_count(&mut child, &fake.pages_served, 3, "list page(s)");
    send_sigint(&child);

    let status = wait_with_deadline(&mut child, Duration::from_secs(15));
    let stderr = read_stderr(&mut child);

    assert_eq!(
        status.code(),
        Some(130),
        "expected batch-run to exit 130 after its line was interrupted, got {status:?}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "SIGINT should terminate without panics\nstderr: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&script_dir);
}

/// The parallel executor's spawn loop checks the interrupt flag at the top
/// of each iteration, then awaits a semaphore permit — a window where a
/// SIGINT can land while all workers are busy. The permit freed by an
/// interrupted line must NOT dispatch the next queued line: its freshly
/// installed Ctrl+C handler would never see the already-delivered signal,
/// so it would run to full completion (real requests included) yet read
/// the process-global interruption state stored by a sibling line and
/// misreport as skipped. Guards the post-await interrupt re-check.
///
/// Layout: 2 workers, 3 lines. Lines 1-2 list endlessly on their own
/// endpoints (both provably in flight before SIGINT), line 3 targets a
/// third endpoint that must never receive a request.
#[test]
#[cfg(target_family = "unix")]
fn batch_run_parallel_queued_line_after_sigint_never_runs() {
    let busy_a = spawn_fake_s3(None);
    let busy_b = spawn_fake_s3(None);
    let queued = spawn_fake_s3(Some(1));
    let script_dir = create_temp_dir("batch_run_parallel");
    let script_path = script_dir.join("script.txt");
    let ls_line = |endpoint: &str| {
        format!(
            "ls --recursive {} s3://{BUCKET}/",
            target_client_args(endpoint).join(" "),
        )
    };
    let script = format!(
        "{}\n{}\n{}\n",
        ls_line(&busy_a.endpoint),
        ls_line(&busy_b.endpoint),
        ls_line(&queued.endpoint),
    );
    std::fs::write(&script_path, script).expect("write batch script");

    let args = vec![
        "batch-run".to_string(),
        "--parallel".to_string(),
        "2".to_string(),
        script_path.to_string_lossy().into_owned(),
    ];
    let mut child = spawn_s7cmd(&args);

    // Both workers provably mid-listing => their Ctrl+C handlers are
    // installed, both permits are held, and the spawn loop is parked on
    // `acquire_owned().await` for line 3.
    wait_for_count(&mut child, &busy_a.pages_served, 2, "list page(s) (line 1)");
    wait_for_count(&mut child, &busy_b.pages_served, 2, "list page(s) (line 2)");
    send_sigint(&child);

    let status = wait_with_deadline(&mut child, Duration::from_secs(15));
    let stderr = read_stderr(&mut child);

    assert_eq!(
        status.code(),
        Some(130),
        "expected batch-run to exit 130 after its lines were interrupted, got {status:?}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "SIGINT should terminate without panics\nstderr: {stderr}"
    );
    assert_eq!(
        queued.pages_served.load(Ordering::SeqCst),
        0,
        "the queued third line must not run after SIGINT (permit freed by an \
         interrupted line must not dispatch it)\nstderr: {stderr}"
    );

    let _ = std::fs::remove_dir_all(&script_dir);
}

// ---- no-SIGINT controls ----
//
// The same harness without SIGINT must complete the full run and exit 0 —
// the SIGINT handling must not affect uninterrupted runs. These also prove
// the fake endpoint drives each pipeline to a genuine completion, so the
// SIGINT tests above interrupt a run that would otherwise have succeeded.

#[test]
fn ls_without_sigint_exits_zero() {
    let fake = spawn_fake_s3(Some(3));
    let mut args = vec!["ls".to_string()];
    args.extend(target_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/"));
    let mut child = spawn_s7cmd(&args);

    let status = wait_with_deadline(&mut child, Duration::from_secs(30));
    let stderr = read_stderr(&mut child);

    assert_eq!(
        status.code(),
        Some(0),
        "expected exit 0, got {status:?}\nstderr: {stderr}"
    );
    assert_eq!(fake.pages_served.load(Ordering::SeqCst), 3);
}

#[test]
fn clean_without_sigint_exits_zero() {
    let fake = spawn_fake_s3(Some(3));
    let mut args = vec![
        "clean".to_string(),
        "--force".to_string(),
        "--max-parallel-listings".to_string(),
        "1".to_string(),
    ];
    args.extend(target_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/"));
    let mut child = spawn_s7cmd(&args);

    let status = wait_with_deadline(&mut child, Duration::from_secs(30));
    let stderr = read_stderr(&mut child);

    assert_eq!(
        status.code(),
        Some(0),
        "expected exit 0, got {status:?}\nstderr: {stderr}"
    );
    assert_eq!(fake.pages_served.load(Ordering::SeqCst), 3);
    assert!(
        fake.deletes_served.load(Ordering::SeqCst) >= 1,
        "the run must have sent at least one delete request"
    );
    let deleted = fake.deleted_keys.lock().unwrap();
    for key in ["page-0.txt", "page-1.txt", "page-2.txt"] {
        assert!(
            deleted.iter().any(|deleted_key| deleted_key == key),
            "delete requests missing {key}; deleted: {deleted:?}"
        );
    }
}

#[test]
fn cp_without_sigint_exits_zero() {
    let fake = spawn_fake_s3(Some(1));
    let local_dir = create_temp_dir("cp_control");
    let mut args = vec!["cp".to_string()];
    args.extend(source_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/object.txt"));
    args.push(format!("{}/", local_dir.to_string_lossy()));
    let mut child = spawn_s7cmd(&args);

    let status = wait_with_deadline(&mut child, Duration::from_secs(30));
    let stderr = read_stderr(&mut child);

    assert_eq!(
        status.code(),
        Some(0),
        "expected exit 0, got {status:?}\nstderr: {stderr}"
    );
    let body = std::fs::read_to_string(local_dir.join("object.txt"))
        .unwrap_or_else(|e| panic!("cp must have downloaded object.txt: {e}"));
    assert_eq!(body, OBJECT_BODY, "downloaded body mismatch");
    let _ = std::fs::remove_dir_all(&local_dir);
}

#[test]
fn mv_without_sigint_exits_zero_and_deletes_source() {
    let fake = spawn_fake_s3(Some(1));
    let local_dir = create_temp_dir("mv_control");
    let mut args = vec!["mv".to_string()];
    args.extend(source_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/object.txt"));
    args.push(format!("{}/", local_dir.to_string_lossy()));
    let mut child = spawn_s7cmd(&args);

    let status = wait_with_deadline(&mut child, Duration::from_secs(30));
    let stderr = read_stderr(&mut child);

    assert_eq!(
        status.code(),
        Some(0),
        "expected exit 0, got {status:?}\nstderr: {stderr}"
    );
    let body = std::fs::read_to_string(local_dir.join("object.txt"))
        .unwrap_or_else(|e| panic!("mv must have downloaded object.txt: {e}"));
    assert_eq!(body, OBJECT_BODY, "downloaded body mismatch");
    assert!(
        fake.deletes_served.load(Ordering::SeqCst) >= 1,
        "a completed mv must delete the source object"
    );
    assert!(
        fake.deleted_keys
            .lock()
            .unwrap()
            .iter()
            .any(|key| key == "object.txt"),
        "delete requests missing object.txt; deleted: {:?}",
        fake.deleted_keys.lock().unwrap()
    );
    let _ = std::fs::remove_dir_all(&local_dir);
}

#[test]
fn sync_without_sigint_exits_zero() {
    let fake = spawn_fake_s3(Some(3));
    let local_dir = create_temp_dir("sync_control");
    let mut args = vec!["sync".to_string()];
    args.extend(source_client_args(&fake.endpoint));
    args.push(format!("s3://{BUCKET}/"));
    args.push(format!("{}/", local_dir.to_string_lossy()));
    let mut child = spawn_s7cmd(&args);

    let status = wait_with_deadline(&mut child, Duration::from_secs(30));
    let stderr = read_stderr(&mut child);

    assert_eq!(
        status.code(),
        Some(0),
        "expected exit 0, got {status:?}\nstderr: {stderr}"
    );
    assert_eq!(fake.pages_served.load(Ordering::SeqCst), 3);
    for key in ["page-0.txt", "page-1.txt", "page-2.txt"] {
        let path = local_dir.join(key);
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("sync must have downloaded {key}: {e}"));
        assert_eq!(body, OBJECT_BODY, "downloaded body mismatch for {key}");
    }
    let _ = std::fs::remove_dir_all(&local_dir);
}
