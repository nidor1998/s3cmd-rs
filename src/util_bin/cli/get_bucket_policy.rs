// Vendored from s3util-rs@0.2.0
//   src/bin/s3util/cli/get_bucket_policy.rs
// Adjustments: stripped #[cfg(test)] mod tests; rewrote crate::cli → super
//              Report println made pipe-safe (ported from s3util-rs 1.9.2).

use anyhow::Result;

use s3util_rs::config::ClientConfig;
use s3util_rs::config::args::get_bucket_policy::GetBucketPolicyArgs;
use s3util_rs::output::json::get_bucket_policy_to_json;
use s3util_rs::storage::s3::api::{self, HeadError};

use crate::pipe_safe::println_pipe_safe;

use super::ExitStatus;

/// Runtime entry for `s3util get-bucket-policy s3://<BUCKET>`.
///
/// Builds the SDK client from `client_config`, issues `GetBucketPolicy`,
/// and prints the response as pretty-printed JSON followed by a newline.
/// Default output mirrors `aws s3api get-bucket-policy --output json`:
/// `{"Policy": "<escaped-JSON-string>"}` where `Policy` is the raw policy
/// JSON double-encoded as a JSON string. With `--policy-only`, prints just
/// the policy JSON itself (parsed and re-pretty-printed).
///
/// Returns `ExitStatus::NotFound` (exit code 4) when S3 reports
/// `NoSuchBucket` (logged as "bucket … not found") or `NoSuchBucketPolicy`
/// (logged as "policy for … not found").
pub async fn run_get_bucket_policy(
    args: GetBucketPolicyArgs,
    client_config: ClientConfig,
) -> Result<ExitStatus> {
    let bucket = args
        .bucket_name()
        .map_err(|e| anyhow::anyhow!("{}", e.trim_end()))?;
    let client = client_config.create_client().await;
    match api::get_bucket_policy(&client, &bucket).await {
        Ok(out) => {
            let pretty = if args.policy_only {
                render_policy_only(out.policy())?
            } else {
                serde_json::to_string_pretty(&get_bucket_policy_to_json(&out))?
            };
            println_pipe_safe(&pretty)?;
            Ok(ExitStatus::Success)
        }
        Err(HeadError::BucketNotFound) => {
            tracing::warn!("bucket s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::NotFound) => {
            tracing::warn!("policy for s3://{bucket} not found");
            Ok(ExitStatus::NotFound)
        }
        Err(HeadError::Other(e)) => Err(e),
    }
}

/// Render the inner policy for `--policy-only`. Parses the policy string as
/// JSON and pretty-prints it; falls back to the raw string verbatim if S3
/// somehow returned non-JSON. Returns `{}` when no policy field is present
/// (a 200-OK with empty body — should not occur in practice; S3 returns
/// `NoSuchBucketPolicy` instead, which the caller maps to NotFound).
fn render_policy_only(policy: Option<&str>) -> Result<String> {
    let Some(policy) = policy else {
        return Ok("{}".to_string());
    };
    match serde_json::from_str::<serde_json::Value>(policy) {
        Ok(v) => Ok(serde_json::to_string_pretty(&v)?),
        Err(_) => Ok(policy.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_policy_only_none_returns_empty_object() {
        assert_eq!(render_policy_only(None).unwrap(), "{}");
    }

    #[test]
    fn render_policy_only_valid_json_is_pretty_printed() {
        let raw = r#"{"Version":"2012-10-17","Statement":[]}"#;
        let pretty = render_policy_only(Some(raw)).unwrap();
        // Pretty-printed JSON contains newlines and indentation.
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("\"Version\""));
        assert!(pretty.contains("2012-10-17"));
    }

    #[test]
    fn render_policy_only_invalid_json_returns_raw_string() {
        let raw = "not valid json {{";
        let out = render_policy_only(Some(raw)).unwrap();
        assert_eq!(out, raw);
    }

    #[test]
    fn render_policy_only_empty_string_returns_raw_string() {
        // Empty string is not valid JSON → fall through to raw return.
        let out = render_policy_only(Some("")).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn render_policy_only_complex_policy_round_trips() {
        let raw = r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::example/*"}]}"#;
        let pretty = render_policy_only(Some(raw)).unwrap();
        let reparsed: serde_json::Value = serde_json::from_str(&pretty).unwrap();
        let original: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(reparsed, original);
    }
}
