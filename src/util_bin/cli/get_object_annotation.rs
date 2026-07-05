// Vendored from s3util-rs@1.6.0
//   src/bin/s3util/cli/get_object_annotation.rs
// Adjustments: none — the upstream file already targets `super::ExitStatus`.

use std::io::Write as _;
use std::path::Path;

use anyhow::{Context, Result};
use aws_sdk_s3::operation::get_object_annotation::GetObjectAnnotationOutput;
use aws_sdk_s3::types::{ChecksumAlgorithm, ChecksumType, ServerSideEncryption};
use tempfile::NamedTempFile;
use tracing::{info, warn};

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::get_object_annotation::GetObjectAnnotationArgs;
use s3util_rs::output::json::get_object_annotation_to_json;
use s3util_rs::storage::annotation;
use s3util_rs::storage::s3::api::{self, GetObjectAnnotationParams, ObjectAnnotationError};

use super::ExitStatus;

/// Pick the single additional checksum S3 returned, mapped to the algorithm we
/// can recompute locally. Returns `None` when no supported checksum is present.
fn detect_checksum(out: &GetObjectAnnotationOutput) -> Option<(ChecksumAlgorithm, String)> {
    if let Some(v) = out.checksum_crc64_nvme() {
        return Some((ChecksumAlgorithm::Crc64Nvme, v.to_string()));
    }
    if let Some(v) = out.checksum_crc32() {
        return Some((ChecksumAlgorithm::Crc32, v.to_string()));
    }
    if let Some(v) = out.checksum_crc32_c() {
        return Some((ChecksumAlgorithm::Crc32C, v.to_string()));
    }
    if let Some(v) = out.checksum_sha1() {
        return Some((ChecksumAlgorithm::Sha1, v.to_string()));
    }
    if let Some(v) = out.checksum_sha256() {
        return Some((ChecksumAlgorithm::Sha256, v.to_string()));
    }
    None
}

/// Result of running the available integrity checks against a byte buffer.
#[derive(Debug)]
enum IntegrityCheck {
    /// At least one applicable check passed.
    Verified,
    /// No check applied (not AES256, and no additional checksum was present).
    Unverifiable,
}

/// Run every available integrity check against `bytes`: content-length equality,
/// the AES256 ETag/MD5, and the additional checksum (if any). Returns `Verified`
/// when at least one check passed, `Unverifiable` when none applied, or `Err` on
/// any mismatch. Used for both the pre-write (in-transit) and post-write
/// (on-disk) verifications so both run identical logic.
fn check_integrity(
    bytes: &[u8],
    content_length: Option<i64>,
    e_tag: Option<&str>,
    sse: Option<&ServerSideEncryption>,
    checksum: Option<&(ChecksumAlgorithm, String)>,
    bucket: &str,
    key: &str,
) -> Result<IntegrityCheck> {
    // Content-length sanity check (always possible; mismatch is fatal).
    if let Some(len) = content_length
        && len != bytes.len() as i64
    {
        anyhow::bail!(
            "content length mismatch for s3://{bucket}/{key}: response said {len} bytes, received {}",
            bytes.len()
        );
    }

    let mut verified = false;
    match annotation::verify_etag_md5(bytes, e_tag, sse) {
        Some(true) => verified = true,
        Some(false) => anyhow::bail!("ETag (MD5) verification failed for s3://{bucket}/{key}"),
        None => {}
    }
    if let Some((algo, expected)) = checksum {
        if annotation::verify_additional_checksum(bytes, algo.clone(), expected) {
            verified = true;
        } else {
            anyhow::bail!(
                "{} checksum verification failed for s3://{bucket}/{key}",
                algo.as_str()
            );
        }
    }

    Ok(if verified {
        IntegrityCheck::Verified
    } else {
        IntegrityCheck::Unverifiable
    })
}

/// Re-read the saved file from disk and re-run the integrity checks against its
/// on-disk bytes — the `cp`-style, recompute-from-disk verification. Runs after
/// the temp file has been renamed into place; a mismatch here can only mean the
/// write corrupted the data, so the error is wrapped with context saying the
/// saved file may be corrupted. `Verified`/`Unverifiable` are both success (the
/// pre-write step already warned about un-verifiability).
fn verify_saved_file(
    path: &Path,
    content_length: Option<i64>,
    e_tag: Option<&str>,
    sse: Option<&ServerSideEncryption>,
    checksum: Option<&(ChecksumAlgorithm, String)>,
    bucket: &str,
    key: &str,
) -> Result<()> {
    let on_disk = std::fs::read(path)
        .with_context(|| format!("re-reading saved file {} for verification", path.display()))?;
    check_integrity(&on_disk, content_length, e_tag, sse, checksum, bucket, key).with_context(
        || {
            format!(
                "post-write verification failed for s3://{bucket}/{key}: the saved file {} may be corrupted",
                path.display()
            )
        },
    )?;
    Ok(())
}

/// Runtime entry for
/// `s3util get-object-annotation s3://<BUCKET>/<KEY> <OUTFILE> --annotation-name N`.
///
/// Fetches the annotation payload (checksum-mode ENABLED), buffers it in memory
/// (≤1 MiB), verifies content length, the ETag/MD5 (only for AES256 objects),
/// and the additional checksum (if any); warns when neither check applies. Then
/// writes the payload to a temp file and atomically renames it to `<OUTFILE>`
/// (or streams to stdout when `<OUTFILE>` is `-`). For file output it then
/// re-reads the saved file and recomputes the same ETag / additional checksum
/// from disk (`cp`-style, rename-then-verify order) so a corrupt write is
/// caught; a post-write mismatch leaves the file in place and returns `Err`.
/// Finally it prints AWS-CLI-shape JSON metadata (file mode only). Returns
/// `ExitStatus::NotFound` (exit 4) when the bucket, object, or version does not
/// exist, or when the object exists but has no annotation under the requested
/// name (`NoSuchAnnotation`, logged as "annotation … not found"); a
/// verification mismatch returns `Err` (exit 1).
pub async fn run_get_object_annotation(
    args: GetObjectAnnotationArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let (bucket, key) = args
        .bucket_key()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;

    let annotation_name = args
        .annotation_name
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--annotation-name is required"))?;
    annotation::validate_annotation_name(annotation_name)?;
    let outfile = args
        .outfile
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("outfile is required"))?;

    let request_payer = client_config.request_payer.clone();
    let client = client_config.create_client().await;

    let params = GetObjectAnnotationParams {
        bucket: &bucket,
        key: &key,
        annotation_name,
        version_id: args.target_version_id.as_deref(),
        request_payer,
    };

    let out = match api::get_object_annotation(&client, params).await {
        Ok(out) => out,
        Err(ObjectAnnotationError::BucketNotFound) => {
            tracing::error!("bucket s3://{bucket} not found");
            return Ok(ExitStatus::NotFound);
        }
        Err(ObjectAnnotationError::NotFound) => {
            match args.target_version_id.as_deref() {
                Some(v) => tracing::error!("s3://{bucket}/{key} (versionId={v}) not found"),
                None => tracing::error!("object s3://{bucket}/{key} not found"),
            }
            return Ok(ExitStatus::NotFound);
        }
        Err(ObjectAnnotationError::AnnotationNotFound) => {
            match args.target_version_id.as_deref() {
                Some(v) => tracing::error!(
                    "annotation {annotation_name} not found for s3://{bucket}/{key} (versionId={v})"
                ),
                None => {
                    tracing::error!(
                        "annotation {annotation_name} not found for s3://{bucket}/{key}"
                    )
                }
            }
            return Ok(ExitStatus::NotFound);
        }
        Err(ObjectAnnotationError::Other(e)) => return Err(e),
    };

    // Gather everything we need from `out` before consuming its payload body.
    let json = get_object_annotation_to_json(&out);
    let e_tag = out.e_tag().map(str::to_string);
    let sse = out.server_side_encryption().cloned();
    let content_length = out.content_length();
    let is_composite = matches!(out.checksum_type(), Some(ct) if *ct == ChecksumType::Composite);
    let checksum = if is_composite {
        None
    } else {
        detect_checksum(&out)
    };

    // Consume the payload body with a bounded read (moves the public
    // `annotation_payload` field). Streaming with a hard cap means a buggy or
    // hostile endpoint returning more than the 1 MiB annotation limit can't OOM
    // the process before the content-length check runs.
    let mut body = out.annotation_payload;
    let cap = annotation::MAX_ANNOTATION_PAYLOAD_LEN;
    let mut payload: Vec<u8> = Vec::new();
    while let Some(chunk) = body
        .try_next()
        .await
        .context("reading annotation payload body")?
    {
        payload.extend_from_slice(&chunk);
        if payload.len() > cap {
            anyhow::bail!(
                "annotation payload for s3://{bucket}/{key} exceeds the 1 MiB limit ({cap} bytes)"
            );
        }
    }

    // Integrity verification against the in-transit payload (pre-write). A
    // mismatch is fatal and stops us writing known-bad data; when no check
    // applies we warn but still write.
    let verified = match check_integrity(
        &payload,
        content_length,
        e_tag.as_deref(),
        sse.as_ref(),
        checksum.as_ref(),
        &bucket,
        &key,
    )? {
        IntegrityCheck::Verified => true,
        IntegrityCheck::Unverifiable => {
            warn!(
                bucket = %bucket,
                key = %key,
                "payload integrity could not be verified (no AES256 ETag and no additional checksum)."
            );
            false
        }
    };

    // Output.
    if outfile == "-" {
        std::io::stdout()
            .write_all(&payload)
            .context("writing annotation payload to stdout")?;
        return Ok(ExitStatus::Success);
    }

    let path = Path::new(outfile);
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mut tmp = NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temp file next to {outfile}"))?;
    tmp.write_all(&payload)
        .context("writing annotation payload to temp file")?;
    tmp.flush()
        .context("flushing annotation payload temp file")?;
    tmp.persist(path)
        .map_err(|e| anyhow::anyhow!("persisting annotation payload to {outfile}: {e}"))?;

    // Post-write verification: re-read the saved file and recompute the ETag /
    // additional checksum from disk (cp-style, rename-then-verify order). A
    // mismatch can only mean the write corrupted the data; the file is left in
    // place and we return Err.
    verify_saved_file(
        path,
        content_length,
        e_tag.as_deref(),
        sse.as_ref(),
        checksum.as_ref(),
        &bucket,
        &key,
    )?;

    println!("{}", serde_json::to_string_pretty(&json)?);
    let outcome = if verified {
        "written and verified"
    } else {
        "written, but integrity could NOT be verified"
    };
    info!(
        bucket = %bucket,
        key = %key,
        annotation_name = %annotation_name,
        outfile = %outfile,
        "Annotation payload {}.",
        outcome
    );
    Ok(ExitStatus::Success)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_s3::primitives::ByteStream;

    // `detect_checksum` ignores the payload, but `annotation_payload` is a
    // required builder field, so every output carries an empty stream.
    fn empty_payload() -> ByteStream {
        ByteStream::from_static(b"")
    }

    #[test]
    fn detect_checksum_none_when_no_checksum_present() {
        let out = GetObjectAnnotationOutput::builder()
            .annotation_payload(empty_payload())
            .build();
        assert!(detect_checksum(&out).is_none());
    }

    #[test]
    fn detect_checksum_crc64nvme() {
        let out = GetObjectAnnotationOutput::builder()
            .annotation_payload(empty_payload())
            .checksum_crc64_nvme("crc64val")
            .build();
        let (algo, val) = detect_checksum(&out).expect("checksum present");
        assert_eq!(algo, ChecksumAlgorithm::Crc64Nvme);
        assert_eq!(val, "crc64val");
    }

    #[test]
    fn detect_checksum_crc32() {
        let out = GetObjectAnnotationOutput::builder()
            .annotation_payload(empty_payload())
            .checksum_crc32("crc32val")
            .build();
        let (algo, val) = detect_checksum(&out).expect("checksum present");
        assert_eq!(algo, ChecksumAlgorithm::Crc32);
        assert_eq!(val, "crc32val");
    }

    #[test]
    fn detect_checksum_crc32c() {
        let out = GetObjectAnnotationOutput::builder()
            .annotation_payload(empty_payload())
            .checksum_crc32_c("crc32cval")
            .build();
        let (algo, val) = detect_checksum(&out).expect("checksum present");
        assert_eq!(algo, ChecksumAlgorithm::Crc32C);
        assert_eq!(val, "crc32cval");
    }

    #[test]
    fn detect_checksum_sha1() {
        let out = GetObjectAnnotationOutput::builder()
            .annotation_payload(empty_payload())
            .checksum_sha1("sha1val")
            .build();
        let (algo, val) = detect_checksum(&out).expect("checksum present");
        assert_eq!(algo, ChecksumAlgorithm::Sha1);
        assert_eq!(val, "sha1val");
    }

    #[test]
    fn detect_checksum_sha256() {
        let out = GetObjectAnnotationOutput::builder()
            .annotation_payload(empty_payload())
            .checksum_sha256("sha256val")
            .build();
        let (algo, val) = detect_checksum(&out).expect("checksum present");
        assert_eq!(algo, ChecksumAlgorithm::Sha256);
        assert_eq!(val, "sha256val");
    }

    // CRC64NVME outranks every other algorithm when several are present.
    #[test]
    fn detect_checksum_prefers_crc64nvme_over_others() {
        let out = GetObjectAnnotationOutput::builder()
            .annotation_payload(empty_payload())
            .checksum_crc64_nvme("crc64val")
            .checksum_crc32("crc32val")
            .checksum_crc32_c("crc32cval")
            .checksum_sha1("sha1val")
            .checksum_sha256("sha256val")
            .build();
        let (algo, val) = detect_checksum(&out).expect("checksum present");
        assert_eq!(algo, ChecksumAlgorithm::Crc64Nvme);
        assert_eq!(val, "crc64val");
    }

    // Absent CRC64NVME, CRC32 outranks the SHA family.
    #[test]
    fn detect_checksum_prefers_crc32_over_sha() {
        let out = GetObjectAnnotationOutput::builder()
            .annotation_payload(empty_payload())
            .checksum_crc32("crc32val")
            .checksum_sha1("sha1val")
            .checksum_sha256("sha256val")
            .build();
        let (algo, val) = detect_checksum(&out).expect("checksum present");
        assert_eq!(algo, ChecksumAlgorithm::Crc32);
        assert_eq!(val, "crc32val");
    }

    #[test]
    fn check_integrity_verified_on_matching_checksum() {
        let payload = b"hello world";
        let checksum = (
            ChecksumAlgorithm::Crc64Nvme,
            annotation::compute_checksum_base64(payload, ChecksumAlgorithm::Crc64Nvme),
        );
        let res = check_integrity(
            payload,
            Some(payload.len() as i64),
            None,
            None,
            Some(&checksum),
            "b",
            "k",
        )
        .unwrap();
        assert!(matches!(res, IntegrityCheck::Verified));
    }

    #[test]
    fn check_integrity_verified_on_matching_aes256_etag() {
        let payload = b"hello";
        let etag = format!("\"{:x}\"", md5::compute(payload));
        let res = check_integrity(
            payload,
            None,
            Some(&etag),
            Some(&ServerSideEncryption::Aes256),
            None,
            "b",
            "k",
        )
        .unwrap();
        assert!(matches!(res, IntegrityCheck::Verified));
    }

    #[test]
    fn check_integrity_unverifiable_when_nothing_applies() {
        let res = check_integrity(b"hello", Some(5), None, None, None, "b", "k").unwrap();
        assert!(matches!(res, IntegrityCheck::Unverifiable));
    }

    #[test]
    fn check_integrity_err_on_content_length_mismatch() {
        let err = check_integrity(b"hello", Some(999), None, None, None, "b", "k").unwrap_err();
        assert!(format!("{err:#}").contains("content length mismatch"));
    }

    #[test]
    fn check_integrity_err_on_etag_mismatch() {
        // AES256 + a plain (non-multipart) ETag that does not match the payload.
        let err = check_integrity(
            b"hello",
            None,
            Some("\"00000000000000000000000000000000\""),
            Some(&ServerSideEncryption::Aes256),
            None,
            "b",
            "k",
        )
        .unwrap_err();
        assert!(format!("{err:#}").contains("ETag (MD5) verification failed"));
    }

    #[test]
    fn check_integrity_err_on_checksum_mismatch() {
        let checksum = (ChecksumAlgorithm::Crc64Nvme, "AAAAAAAAAAA=".to_string());
        let err = check_integrity(b"hello world", None, None, None, Some(&checksum), "b", "k")
            .unwrap_err();
        assert!(format!("{err:#}").contains("checksum verification failed"));
    }

    #[test]
    fn verify_saved_file_ok_when_bytes_match_checksum() {
        let payload = b"hello world";
        let checksum = (
            ChecksumAlgorithm::Crc64Nvme,
            annotation::compute_checksum_base64(payload, ChecksumAlgorithm::Crc64Nvme),
        );
        let tmp = NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), payload).unwrap();
        let res = verify_saved_file(
            tmp.path(),
            Some(payload.len() as i64),
            None,
            None,
            Some(&checksum),
            "b",
            "k",
        );
        assert!(res.is_ok(), "expected ok, got: {res:?}");
    }

    #[test]
    fn verify_saved_file_err_when_bytes_corrupted() {
        let payload = b"hello world";
        // Checksum computed over the *correct* payload; the file on disk holds
        // different bytes, simulating a corrupt write.
        let checksum = (
            ChecksumAlgorithm::Crc64Nvme,
            annotation::compute_checksum_base64(payload, ChecksumAlgorithm::Crc64Nvme),
        );
        let tmp = NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"totally different bytes").unwrap();
        let err =
            verify_saved_file(tmp.path(), None, None, None, Some(&checksum), "b", "k").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("may be corrupted"), "got: {msg}");
    }

    #[test]
    fn verify_saved_file_ok_when_nothing_verifiable() {
        // No AES256 ETag and no additional checksum: nothing to recompute, so a
        // correctly-written file must still pass (Unverifiable is treated as
        // success, matching the pre-write path). Content-length still matches.
        let payload = b"hello world";
        let tmp = NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), payload).unwrap();
        let res = verify_saved_file(
            tmp.path(),
            Some(payload.len() as i64),
            None,
            None,
            None,
            "b",
            "k",
        );
        assert!(res.is_ok(), "expected ok, got: {res:?}");
    }
}
