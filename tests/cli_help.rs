use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn top_level_help_lists_sync() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("sync"));
}

#[test]
fn sync_help_includes_source_options() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["sync", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Source Options"));
}

#[test]
fn sync_help_includes_lua_passthrough() {
    // Smoke: confirms s3sync's lua_support default feature reaches the user.
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["sync", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--filter-callback-lua-script"));
}

#[test]
fn cp_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["cp", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("AWS Configuration"));
}

#[test]
fn mv_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["mv", "--help"])
        .assert()
        .success();
}

#[test]
fn rm_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["rm", "--help"])
        .assert()
        .success();
}

#[test]
fn top_level_help_lists_cp_mv_rm() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("cp"))
        .stdout(predicate::str::contains("mv"))
        .stdout(predicate::str::contains("rm"));
}

#[test]
fn create_bucket_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["create-bucket", "--help"])
        .assert()
        .success();
}

#[test]
fn delete_bucket_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-bucket", "--help"])
        .assert()
        .success();
}

#[test]
fn head_bucket_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["head-bucket", "--help"])
        .assert()
        .success();
}

#[test]
fn head_object_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["head-object", "--help"])
        .assert()
        .success();
}

#[test]
fn get_object_tagging_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-object-tagging", "--help"])
        .assert()
        .success();
}

#[test]
fn put_object_tagging_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-object-tagging", "--help"])
        .assert()
        .success();
}

#[test]
fn delete_object_tagging_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-object-tagging", "--help"])
        .assert()
        .success();
}
#[test]
fn get_object_annotation_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-object-annotation", "--help"])
        .assert()
        .success();
}
#[test]
fn put_object_annotation_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-object-annotation", "--help"])
        .assert()
        .success();
}
#[test]
fn delete_object_annotation_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-object-annotation", "--help"])
        .assert()
        .success();
}
#[test]
fn list_object_annotations_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["list-object-annotations", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_tagging_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-tagging", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_tagging_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-tagging", "--help"])
        .assert()
        .success();
}
#[test]
fn delete_bucket_tagging_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-bucket-tagging", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_policy_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-policy", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_policy_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-policy", "--help"])
        .assert()
        .success();
}
#[test]
fn delete_bucket_policy_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-bucket-policy", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_versioning_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-versioning", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_versioning_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-versioning", "--help"])
        .assert()
        .success();
}

#[test]
fn get_bucket_lifecycle_configuration_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-lifecycle-configuration", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_lifecycle_configuration_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-lifecycle-configuration", "--help"])
        .assert()
        .success();
}
#[test]
fn delete_bucket_lifecycle_configuration_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-bucket-lifecycle-configuration", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_encryption_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-encryption", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_encryption_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-encryption", "--help"])
        .assert()
        .success();
}
#[test]
fn delete_bucket_encryption_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-bucket-encryption", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_cors_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-cors", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_cors_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-cors", "--help"])
        .assert()
        .success();
}
#[test]
fn delete_bucket_cors_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-bucket-cors", "--help"])
        .assert()
        .success();
}
#[test]
fn get_public_access_block_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-public-access-block", "--help"])
        .assert()
        .success();
}
#[test]
fn put_public_access_block_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-public-access-block", "--help"])
        .assert()
        .success();
}
#[test]
fn delete_public_access_block_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-public-access-block", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_website_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-website", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_website_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-website", "--help"])
        .assert()
        .success();
}
#[test]
fn delete_bucket_website_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-bucket-website", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_logging_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-logging", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_logging_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-logging", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_notification_configuration_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-notification-configuration", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_notification_configuration_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-notification-configuration", "--help"])
        .assert()
        .success();
}

#[test]
fn get_bucket_replication_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-replication", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_replication_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-replication", "--help"])
        .assert()
        .success();
}
#[test]
fn delete_bucket_replication_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["delete-bucket-replication", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_accelerate_configuration_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-accelerate-configuration", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_accelerate_configuration_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-accelerate-configuration", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_request_payment_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-request-payment", "--help"])
        .assert()
        .success();
}
#[test]
fn put_bucket_request_payment_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["put-bucket-request-payment", "--help"])
        .assert()
        .success();
}
#[test]
fn get_bucket_policy_status_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["get-bucket-policy-status", "--help"])
        .assert()
        .success();
}
#[test]
fn restore_object_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["restore-object", "--help"])
        .assert()
        .success();
}

#[test]
fn presign_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["presign", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--expires-in"));
}

#[test]
fn top_level_help_lists_new_bucket_subcommands() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "get-bucket-lifecycle-configuration",
        ))
        .stdout(predicate::str::contains("get-bucket-encryption"))
        .stdout(predicate::str::contains("get-bucket-cors"))
        .stdout(predicate::str::contains("get-public-access-block"))
        .stdout(predicate::str::contains("get-bucket-website"))
        .stdout(predicate::str::contains("get-bucket-logging"))
        .stdout(predicate::str::contains(
            "get-bucket-notification-configuration",
        ));
}

#[test]
fn top_level_help_lists_v1_3_subcommands() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("get-bucket-replication"))
        .stdout(predicate::str::contains("put-bucket-replication"))
        .stdout(predicate::str::contains("delete-bucket-replication"))
        .stdout(predicate::str::contains(
            "get-bucket-accelerate-configuration",
        ))
        .stdout(predicate::str::contains(
            "put-bucket-accelerate-configuration",
        ))
        .stdout(predicate::str::contains("get-bucket-request-payment"))
        .stdout(predicate::str::contains("put-bucket-request-payment"))
        .stdout(predicate::str::contains("get-bucket-policy-status"))
        .stdout(predicate::str::contains("restore-object"));
}

#[test]
fn top_level_help_lists_v1_4_subcommands() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("presign"));
}

#[test]
fn top_level_help_lists_annotation_subcommands() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("get-object-annotation"))
        .stdout(predicate::str::contains("put-object-annotation"))
        .stdout(predicate::str::contains("delete-object-annotation"))
        .stdout(predicate::str::contains("list-object-annotations"));
}

#[test]
fn top_level_help_lists_auto_complete_shell() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--auto-complete-shell"));
}

#[test]
fn top_level_auto_complete_shell_runs() {
    // Smoke: top-level --auto-complete-shell bash should exit 0 with
    // non-empty stdout (the shell completion script).
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["--auto-complete-shell", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete")); // bash completion scripts contain `complete -F`
}

#[test]
fn ls_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["ls", "--help"])
        .assert()
        .success();
}

#[test]
fn clean_help_works() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .args(["clean", "--help"])
        .assert()
        .success();
}

#[test]
fn top_level_help_lists_ls_and_clean() {
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("ls"))
        .stdout(predicate::str::contains("clean"));
}

#[test]
fn version_short_flag_prints_pkg_version() {
    let expected = format!("s7cmd {}", env!("CARGO_PKG_VERSION"));
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("-V")
        .assert()
        .success()
        // The version output should at least contain the crate name and the
        // semver from Cargo.toml. Whether it includes commit/target/rustc
        // depends on whether the `version` feature was compiled in.
        .stdout(predicate::str::contains(expected));
}

#[test]
fn version_long_flag_prints_pkg_version() {
    let expected = format!("s7cmd {}", env!("CARGO_PKG_VERSION"));
    Command::cargo_bin("s7cmd")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(expected));
}

// ---------------------------------------------------------------------------
// Security: credential env values must not leak into --help output.
//
// clap renders the current value of an env-backed arg into help text unless
// the arg is marked hide_env_values. Upstream adds that marker at the derive
// level (s3sync PR#246, s3rm-rs PR#94, s3ls-rs PR#29, s3util-rs PR#25); until
// those land in released crates, s7cmd enforces it in cli::build_cli_command.
// These tests drive the real binary with secrets in the process environment —
// one subcommand per upstream parser — and assert the values never appear
// while the env var *names* still do.
// ---------------------------------------------------------------------------

/// Run `s7cmd <sub> --help` with credential env vars set; assert every
/// (name, value) pair shows the name but never the value. A non-secret
/// control var proves clap really does render env values at help time —
/// without it, a clap behavior change could green these tests vacuously.
fn assert_help_hides_credential_values(sub: &str, vars: &[(&str, &str)]) {
    let mut cmd = Command::cargo_bin("s7cmd").unwrap();
    cmd.args([sub, "--help"]);
    for (name, value) in vars {
        cmd.env(name, value);
    }
    // Control: TARGET_REGION is env-backed on every subcommand under test
    // and is not a secret, so its value must render into help output.
    cmd.env("TARGET_REGION", "us-west-2");

    let assert = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();

    assert!(
        stdout.contains("us-west-2"),
        "{sub}: expected the non-secret TARGET_REGION value in help output \
         (control that env values render at all)"
    );
    for (name, value) in vars {
        assert!(
            stdout.contains(name),
            "{sub}: env var name {name} should still appear in help"
        );
        assert!(
            !stdout.contains(value),
            "{sub}: credential value for {name} leaked into --help output"
        );
    }
}

#[test]
fn sync_help_hides_credential_env_values() {
    assert_help_hides_credential_values(
        "sync",
        &[
            ("SOURCE_ACCESS_KEY", "AKIA_sync_should_not_appear"),
            ("SOURCE_SECRET_ACCESS_KEY", "SECRET_sync_should_not_appear"),
            ("SOURCE_SESSION_TOKEN", "TOKEN_sync_should_not_appear"),
            ("TARGET_ACCESS_KEY", "AKIA_sync_tgt_should_not_appear"),
            (
                "TARGET_SECRET_ACCESS_KEY",
                "SECRET_sync_tgt_should_not_appear",
            ),
            ("TARGET_SESSION_TOKEN", "TOKEN_sync_tgt_should_not_appear"),
            ("SOURCE_SSE_C_KEY", "SSECKEY_sync_should_not_appear"),
            ("SOURCE_SSE_C_KEY_MD5", "SSECMD5_sync_should_not_appear"),
            ("TARGET_SSE_C_KEY", "SSECKEY_sync_tgt_should_not_appear"),
            ("TARGET_SSE_C_KEY_MD5", "SSECMD5_sync_tgt_should_not_appear"),
        ],
    );
}

#[test]
fn clean_help_hides_credential_env_values() {
    assert_help_hides_credential_values(
        "clean",
        &[
            ("TARGET_ACCESS_KEY", "AKIA_clean_should_not_appear"),
            ("TARGET_SECRET_ACCESS_KEY", "SECRET_clean_should_not_appear"),
            ("TARGET_SESSION_TOKEN", "TOKEN_clean_should_not_appear"),
        ],
    );
}

#[test]
fn ls_help_hides_credential_env_values() {
    assert_help_hides_credential_values(
        "ls",
        &[
            ("TARGET_ACCESS_KEY", "AKIA_ls_should_not_appear"),
            ("TARGET_SECRET_ACCESS_KEY", "SECRET_ls_should_not_appear"),
            ("TARGET_SESSION_TOKEN", "TOKEN_ls_should_not_appear"),
        ],
    );
}

#[test]
fn cp_help_hides_credential_env_values() {
    assert_help_hides_credential_values(
        "cp",
        &[
            ("SOURCE_ACCESS_KEY", "AKIA_cp_should_not_appear"),
            ("SOURCE_SECRET_ACCESS_KEY", "SECRET_cp_should_not_appear"),
            ("SOURCE_SESSION_TOKEN", "TOKEN_cp_should_not_appear"),
            ("TARGET_ACCESS_KEY", "AKIA_cp_tgt_should_not_appear"),
            (
                "TARGET_SECRET_ACCESS_KEY",
                "SECRET_cp_tgt_should_not_appear",
            ),
            ("TARGET_SESSION_TOKEN", "TOKEN_cp_tgt_should_not_appear"),
            ("SOURCE_SSE_C_KEY", "SSECKEY_cp_should_not_appear"),
            ("SOURCE_SSE_C_KEY_MD5", "SSECMD5_cp_should_not_appear"),
            ("TARGET_SSE_C_KEY", "SSECKEY_cp_tgt_should_not_appear"),
            ("TARGET_SSE_C_KEY_MD5", "SSECMD5_cp_tgt_should_not_appear"),
        ],
    );
}

#[test]
fn head_object_help_hides_credential_env_values() {
    assert_help_hides_credential_values(
        "head-object",
        &[
            ("TARGET_ACCESS_KEY", "AKIA_ho_should_not_appear"),
            ("TARGET_SECRET_ACCESS_KEY", "SECRET_ho_should_not_appear"),
            ("TARGET_SESSION_TOKEN", "TOKEN_ho_should_not_appear"),
            ("SOURCE_SSE_C_KEY", "SSECKEY_ho_should_not_appear"),
            ("SOURCE_SSE_C_KEY_MD5", "SSECMD5_ho_should_not_appear"),
        ],
    );
}
