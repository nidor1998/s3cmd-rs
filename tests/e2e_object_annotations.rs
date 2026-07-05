//! Process-level e2e tests for the object-annotation subcommands
//! (`get-`/`put-`/`delete-object-annotation` and `list-object-annotations`),
//! added with s3util-rs 1.6.0.
//!
//! These hit real AWS and are compiled/run only under `--cfg e2e_test`.

#![cfg(e2e_test)]

mod common;

use common::{
    REGION, TestHelper, create_temp_dir, create_test_file, generate_bucket_name, run, s7cmd_cmd,
};

const PROFILE: &str = "s7cmd-e2e-test";
const ANNOTATION_NAME: &str = "test-note";

// ---- put-object-annotation ----

#[tokio::test]
async fn put_object_annotation_dispatch_success() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper
        .put_object(&bucket, "note.txt", b"object body".to_vec())
        .await;

    let dir = create_temp_dir();
    let payload = b"put annotation payload";
    let payload_file = create_test_file(&dir, "payload.bin", payload);
    let target = format!("s3://{bucket}/note.txt");

    let (code, stdout, stderr) = run(s7cmd_cmd().args([
        "put-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        ANNOTATION_NAME,
        "--annotation-payload",
        payload_file.to_str().unwrap(),
        &target,
    ]));

    assert_eq!(
        code,
        Some(0),
        "put-object-annotation must exit 0; stdout={stdout}\nstderr={stderr}"
    );
    // stdout is AWS-CLI-shape JSON.
    serde_json::from_str::<serde_json::Value>(&stdout).expect("put stdout must be valid JSON");
    // SDK-side verification: the annotation payload landed verbatim.
    let got = helper
        .get_object_annotation_payload(&bucket, "note.txt", None, ANNOTATION_NAME)
        .await;
    assert_eq!(
        got.as_slice(),
        payload,
        "seeded annotation payload mismatch"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn put_object_annotation_dispatch_object_not_found() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let dir = create_temp_dir();
    let payload_file = create_test_file(&dir, "payload.bin", b"x");
    let target = format!("s3://{bucket}/never-existed.txt");

    let (code, _stdout, _stderr) = run(s7cmd_cmd().args([
        "put-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        ANNOTATION_NAME,
        "--annotation-payload",
        payload_file.to_str().unwrap(),
        &target,
    ]));

    assert_eq!(
        code,
        Some(4),
        "put-object-annotation on a missing object must exit 4"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn put_object_annotation_dispatch_bucket_not_found() {
    let bucket = generate_bucket_name(); // never created

    let dir = create_temp_dir();
    let payload_file = create_test_file(&dir, "payload.bin", b"x");
    let target = format!("s3://{bucket}/key.txt");

    let (code, _stdout, _stderr) = run(s7cmd_cmd().args([
        "put-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        ANNOTATION_NAME,
        "--annotation-payload",
        payload_file.to_str().unwrap(),
        &target,
    ]));

    assert_eq!(
        code,
        Some(4),
        "put-object-annotation against a missing bucket must exit 4"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ---- get-object-annotation ----

#[tokio::test]
async fn get_object_annotation_dispatch_success_to_file() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper
        .put_object(&bucket, "note.txt", b"object body".to_vec())
        .await;

    let dir = create_temp_dir();
    let payload = b"hello annotation payload";
    let payload_file = create_test_file(&dir, "payload.bin", payload);
    let out_file = dir.join("got.bin");
    let target = format!("s3://{bucket}/note.txt");

    // Seed via s7cmd put-object-annotation (sends CRC64NVME) so the get can
    // recompute and report it.
    let (put_code, _o, put_err) = run(s7cmd_cmd().args([
        "put-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        ANNOTATION_NAME,
        "--annotation-payload",
        payload_file.to_str().unwrap(),
        &target,
    ]));
    assert_eq!(put_code, Some(0), "seed put must exit 0; stderr={put_err}");

    let (code, stdout, stderr) = run(s7cmd_cmd().args([
        "get-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        ANNOTATION_NAME,
        &target,
        out_file.to_str().unwrap(),
    ]));

    assert_eq!(
        code,
        Some(0),
        "get-object-annotation must exit 0; stdout={stdout}\nstderr={stderr}"
    );
    // Saved file must equal the seeded payload byte-for-byte.
    let got = std::fs::read(&out_file).expect("output file must exist");
    assert_eq!(
        got.as_slice(),
        payload,
        "retrieved payload must equal seeded"
    );
    // stdout is AWS-CLI-shape JSON with metadata + checksum.
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("get stdout must be valid JSON");
    assert!(
        json.get("ContentLength").is_some(),
        "JSON must contain ContentLength; got {json}"
    );
    assert!(
        json.get("ETag").is_some(),
        "JSON must contain ETag; got {json}"
    );
    assert!(
        json.get("ChecksumCRC64NVME").is_some(),
        "JSON must contain ChecksumCRC64NVME; got {json}"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn get_object_annotation_dispatch_success_to_stdout() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper
        .put_object(&bucket, "note.txt", b"object body".to_vec())
        .await;

    let payload = "payload for stdout get";
    helper
        .put_object_annotation(
            &bucket,
            "note.txt",
            None,
            ANNOTATION_NAME,
            payload.as_bytes(),
        )
        .await;

    let target = format!("s3://{bucket}/note.txt");
    let (code, stdout, stderr) = run(s7cmd_cmd().args([
        "get-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        ANNOTATION_NAME,
        &target,
        "-",
    ]));

    assert_eq!(
        code,
        Some(0),
        "get-object-annotation to stdout must exit 0; stderr={stderr}"
    );
    // With `-`, stdout is the raw payload only (no JSON wrapper).
    assert_eq!(stdout, payload, "stdout must be the raw annotation payload");

    helper.delete_bucket_with_cascade(&bucket).await;
}

#[tokio::test]
async fn get_object_annotation_dispatch_object_not_found() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let target = format!("s3://{bucket}/never-existed.txt");
    let (code, _stdout, _stderr) = run(s7cmd_cmd().args([
        "get-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        ANNOTATION_NAME,
        &target,
        "-",
    ]));

    assert_eq!(
        code,
        Some(4),
        "get-object-annotation on a missing object must exit 4"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}

#[tokio::test]
async fn get_object_annotation_dispatch_annotation_not_found() {
    // Object exists, but no annotation under the requested name → exit 4.
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper
        .put_object(&bucket, "note.txt", b"object body".to_vec())
        .await;

    let target = format!("s3://{bucket}/note.txt");
    let (code, _stdout, _stderr) = run(s7cmd_cmd().args([
        "get-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        "no-such-annotation",
        &target,
        "-",
    ]));

    assert_eq!(
        code,
        Some(4),
        "get-object-annotation for a missing annotation name must exit 4"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}

#[tokio::test]
async fn get_object_annotation_dispatch_bucket_not_found() {
    let bucket = generate_bucket_name(); // never created

    let target = format!("s3://{bucket}/key.txt");
    let (code, _stdout, _stderr) = run(s7cmd_cmd().args([
        "get-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        ANNOTATION_NAME,
        &target,
        "-",
    ]));

    assert_eq!(
        code,
        Some(4),
        "get-object-annotation against a missing bucket must exit 4"
    );
}

// ---- list-object-annotations ----

#[tokio::test]
async fn list_object_annotations_dispatch_success() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper
        .put_object(&bucket, "note.txt", b"object body".to_vec())
        .await;
    helper
        .put_object_annotation(&bucket, "note.txt", None, ANNOTATION_NAME, b"payload")
        .await;

    let target = format!("s3://{bucket}/note.txt");
    let (code, stdout, stderr) = run(s7cmd_cmd().args([
        "list-object-annotations",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        &target,
    ]));

    assert_eq!(
        code,
        Some(0),
        "list-object-annotations must exit 0; stdout={stdout}\nstderr={stderr}"
    );
    // stdout is JSON that mentions the seeded annotation name.
    serde_json::from_str::<serde_json::Value>(&stdout).expect("list stdout must be valid JSON");
    assert!(
        stdout.contains(ANNOTATION_NAME),
        "list output must contain the seeded annotation name; stdout={stdout}"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}

#[tokio::test]
async fn list_object_annotations_dispatch_object_not_found() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;

    let target = format!("s3://{bucket}/never-existed.txt");
    let (code, _stdout, _stderr) = run(s7cmd_cmd().args([
        "list-object-annotations",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        &target,
    ]));

    assert_eq!(
        code,
        Some(4),
        "list-object-annotations on a missing object must exit 4"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}

#[tokio::test]
async fn list_object_annotations_dispatch_bucket_not_found() {
    let bucket = generate_bucket_name(); // never created

    let target = format!("s3://{bucket}/key.txt");
    let (code, _stdout, _stderr) = run(s7cmd_cmd().args([
        "list-object-annotations",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        &target,
    ]));

    assert_eq!(
        code,
        Some(4),
        "list-object-annotations against a missing bucket must exit 4"
    );
}

// ---- delete-object-annotation ----

#[tokio::test]
async fn delete_object_annotation_dispatch_success() {
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper
        .put_object(&bucket, "note.txt", b"object body".to_vec())
        .await;
    helper
        .put_object_annotation(&bucket, "note.txt", None, ANNOTATION_NAME, b"payload")
        .await;

    let target = format!("s3://{bucket}/note.txt");
    let (code, stdout, stderr) = run(s7cmd_cmd().args([
        "delete-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        ANNOTATION_NAME,
        &target,
    ]));

    assert_eq!(
        code,
        Some(0),
        "delete-object-annotation must exit 0; stdout={stdout}\nstderr={stderr}"
    );
    // SDK-side verification: the annotation is gone.
    assert!(
        !helper
            .is_object_annotation_exist(&bucket, "note.txt", None, ANNOTATION_NAME)
            .await,
        "annotation should be gone after delete"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}

#[tokio::test]
async fn delete_object_annotation_dispatch_annotation_not_found() {
    // Object exists, but no annotation under the requested name → exit 4.
    let helper = TestHelper::new().await;
    let bucket = generate_bucket_name();
    helper.create_bucket(&bucket, REGION).await;
    helper
        .put_object(&bucket, "note.txt", b"object body".to_vec())
        .await;

    let target = format!("s3://{bucket}/note.txt");
    let (code, _stdout, _stderr) = run(s7cmd_cmd().args([
        "delete-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        "no-such-annotation",
        &target,
    ]));

    assert_eq!(
        code,
        Some(4),
        "delete-object-annotation for a missing annotation name must exit 4"
    );

    helper.delete_bucket_with_cascade(&bucket).await;
}

#[tokio::test]
async fn delete_object_annotation_dispatch_bucket_not_found() {
    let bucket = generate_bucket_name(); // never created

    let target = format!("s3://{bucket}/key.txt");
    let (code, _stdout, _stderr) = run(s7cmd_cmd().args([
        "delete-object-annotation",
        "--target-profile",
        PROFILE,
        "--target-region",
        REGION,
        "--annotation-name",
        ANNOTATION_NAME,
        &target,
    ]));

    assert_eq!(
        code,
        Some(4),
        "delete-object-annotation against a missing bucket must exit 4"
    );
}
