# End-to-End Tests

## Warning

These tests will create and delete AWS resources (S3 buckets and objects), which will result in costs on your AWS account.

These tests are designed to be run against a real AWS account. If any of the tests fail, they may leave resources in your AWS account, such as S3 buckets and their contents.

## Offline tests vs. E2E tests

This directory contains two kinds of process-level tests:

- `cli_*.rs` and `batch_run.rs` — offline tests. They spawn the real `s7cmd` binary but talk only to a loopback mock endpoint with fake credentials, so they need no AWS account and run with a plain `cargo test`.
- `e2e_*.rs` — live-AWS tests. Every file is gated behind `#![cfg(e2e_test)]`, so they are compiled and run only when the `e2e_test` cfg flag is set as shown below.

## Running the tests against AWS

Before running the tests, you need to set up your AWS credentials. Create a profile named `s7cmd-e2e-test` with the AWS CLI:

```bash
aws configure --profile s7cmd-e2e-test
```

The tests use this profile both for the SDK helper client (setup/verification) and for the spawned `s7cmd` processes (passed via `--target-profile` / `--source-profile`).

Then run the tests with the `e2e_test` cfg flag:

```bash
# Run all E2E tests
RUSTFLAGS='--cfg e2e_test' cargo test --test 'e2e_*'

# Run a specific test suite
RUSTFLAGS='--cfg e2e_test' cargo test --test e2e_bucket_ops
```

### Region

The test helpers hard-code the region and the Express One Zone availability zone in `tests/common/mod.rs`:

- `REGION` — `ap-northeast-1` (used as the `LocationConstraint` when creating buckets and as `--target-region` for spawned `s7cmd` processes)
- `EXPRESS_ONE_ZONE_AZ` — `apne1-az4` (used by the directory-bucket tests)

To run against a different region, edit these constants. The `s7cmd-e2e-test` profile's region should match `REGION`.

The SDK helper client additionally accepts a runtime region override via the `S7CMD_E2E_REGION` environment variable (resolution order: `S7CMD_E2E_REGION` > profile region > `REGION` constant). Note that this only affects the helper client — the tests still pass the compile-time `REGION` constant as the bucket `LocationConstraint` and `--target-region`, so fully retargeting a run to another region requires editing the constants.

### Environment variables

Some tests use environment variables:

| Variable | Used by | Behavior if unset |
|---|---|---|
| `S7CMD_E2E_REGION` | The SDK helper client in `tests/common/mod.rs` | Falls back to the profile region, then the `REGION` constant. |
| `S7CMD_E2E_REPLICATION_ROLE_ARN` | `e2e_dry_run` (`delete_bucket_replication_dry_run_does_not_change_state`) | The test is skipped. Set it to an IAM role ARN whose trust policy allows `s3.amazonaws.com` to `AssumeRole`, and on which the e2e profile has `iam:PassRole` permission. |
| `S7CMD_E2E_ACCOUNT_ID` | `e2e_bucket_ops` (`create_and_delete_account_regional_bucket_round_trip`) | The test is skipped. Set it to your 12-digit AWS account ID to exercise account-regional bucket creation. The account must be enrolled in the account-regional namespace. |
| `S7CMD_E2E_LOCATION_CONSTRAINT` | `e2e_bucket_ops` (`create_and_delete_account_regional_bucket_round_trip`) | Defaults to `ap-northeast-1` (the `REGION` constant). Region used for the `LocationConstraint` and `--target-region` in the account-regional bucket test. |

## Notes

These tests create and delete S3 buckets. Occasionally tests may fail due to eventual consistency in AWS (for example, a newly created bucket may not be immediately visible). In such cases, the tests will typically pass on a subsequent run.

Temporary working files are created under `./playground/` (gitignored) and are not cleaned up automatically.
