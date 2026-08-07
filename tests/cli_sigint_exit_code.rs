//! Process-level regression tests for SIGINT (Ctrl+C) exit-code handling
//! in the pipeline subcommands (`ls`, `clean`, `sync`) and in `batch-run`.
//!
//! Each of these subcommands catches Ctrl+C, cancels its pipeline, and must
//! then exit gracefully with code 130 (128 + SIGINT), the conventional shell
//! encoding for a run interrupted by the user — matching what `cp`/`mv`
//! already do via `ExitStatus::Cancelled`. These tests run the real binary
//! against a minimal in-process S3 endpoint (no AWS access needed): the
//! endpoint keeps returning truncated list pages so the run stays busy until
//! the test sends SIGINT, and answers object `GET` (for `sync` downloads),
//! batch `DeleteObjects`, and single-object `DeleteObject` requests so the
//! later pipeline stages stay active as well.
//!
//! Covers the `is_ctrl_c_received()` → `SIGINT_EXIT_CODE` paths in
//! `src/ls_bin/mod.rs`, `src/clean_bin/mod.rs`, and
//! `src/sync_bin/cli/mod.rs` (ported from nidor1998/s3rm-rs#100 /
//! nidor1998/s3ls-rs#34), plus batch-run's severity-ranked aggregation of a
//! line that returns 130.

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BUCKET: &str = "fake-sigint-bucket";

/// One-byte object body served for every object `GET`; the listing
/// advertises `Size` 1 and the matching MD5 ETag so `sync` downloads it
/// without complaint.
const OBJECT_BODY: &str = "x";
const OBJECT_ETAG: &str = "\"9dd4e461268c8034f5c8564e155c67a6\"";

/// Handle to a fake S3 endpoint running on a background thread.
struct FakeS3 {
    endpoint: String,
    pages_served: Arc<AtomicUsize>,
    object_gets_served: Arc<AtomicUsize>,
    deletes_served: Arc<AtomicUsize>,
    deleted_keys: Arc<Mutex<Vec<String>>>,
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
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind fake S3 listener");
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let pages_served = Arc::new(AtomicUsize::new(0));
    let object_gets_served = Arc::new(AtomicUsize::new(0));
    let deletes_served = Arc::new(AtomicUsize::new(0));
    let deleted_keys = Arc::new(Mutex::new(Vec::new()));

    let pages = Arc::clone(&pages_served);
    let gets = Arc::clone(&object_gets_served);
    let deletes = Arc::clone(&deletes_served);
    let keys = Arc::clone(&deleted_keys);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
            let Some((request_line, request_body)) = read_request(&mut stream) else {
                continue;
            };

            let response = route_request(
                &request_line,
                &request_body,
                total_pages,
                &pages,
                &gets,
                &deletes,
                &keys,
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

fn route_request(
    request_line: &str,
    request_body: &str,
    total_pages: Option<usize>,
    pages_served: &AtomicUsize,
    object_gets_served: &AtomicUsize,
    deletes_served: &AtomicUsize,
    deleted_keys: &Mutex<Vec<String>>,
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
