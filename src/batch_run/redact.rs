//! Redact secret argument values from a batch-run line before it is echoed
//! into logs.
//!
//! `batch-run` logs each line's raw text as the `raw` structured field on
//! per-line events (`start` / `success` / `warning` / `skipped` / `failure`
//! / `invalid` / `panicked`) so the user can tell which subcommand each
//! event belongs to. `log_end`'s failure/warning arms and every `invalid`
//! entry emit at `warn`/`error` level — visible at the default verbosity
//! with no `-v` — and `--json-tracing` writes `raw` into machine-readable
//! JSON. A line may legally carry a credential inline (e.g.
//! `cp /f s3://b/k --target-secret-access-key wJalr...` or
//! `--source-sse-c-key <base64-key>`), so without masking the secret would
//! be copied verbatim to stderr / JSON on any non-success outcome.
//!
//! [`redact_secrets`] masks the value of every known secret flag — the same
//! set `hide_credential_env_values` (cli.rs) hides from `--help`: access
//! keys, secret access keys, session tokens, and SSE-C key material.

use std::borrow::Cow;

/// Placeholder written in place of a redacted secret value.
const REDACTED: &str = "****";

/// Flag-name substrings that mark a `--flag` whose value is a credential /
/// secret. Mirrors the id predicate in `hide_credential_env_values`
/// (cli.rs): `access-key` covers `--*-access-key` and
/// `--*-secret-access-key`; `session-token` covers `--*-session-token`;
/// `sse-c-key` covers `--*-sse-c-key` and the `--*-sse-c-key-md5` digests.
const SECRET_FLAG_SUBSTRINGS: [&str; 3] = ["access-key", "session-token", "sse-c-key"];

/// True when `flag` (a token beginning with `-`, minus any trailing
/// `=value`) names a credential/secret argument whose value must not be
/// logged.
fn is_secret_flag(flag: &str) -> bool {
    let name = flag.trim_start_matches('-');
    SECRET_FLAG_SUBSTRINGS.iter().any(|s| name.contains(s))
}

/// Redact secret values from a raw batch-run line before logging.
///
/// Returns [`Cow::Borrowed`] unchanged when the line carries no secret flag
/// (the overwhelmingly common case — a single cheap substring scan, no
/// allocation, no tokenization). When a secret *is* present the line is
/// tokenized and every secret flag's value is replaced with `****`, then
/// space-joined; exact original spacing/quoting is not preserved for those
/// lines, but the command stays identifiable and no secret survives. A line
/// that cannot be tokenized (unbalanced quotes) but still contains a secret
/// flag falls back to a whitespace-level scrub so the secret is masked even
/// on a malformed line.
pub(crate) fn redact_secrets(raw: &str) -> Cow<'_, str> {
    // Fast path: no secret-flag substring anywhere → nothing to redact.
    // Skips tokenization for the ~all lines that carry no credential.
    if !SECRET_FLAG_SUBSTRINGS.iter().any(|s| raw.contains(s)) {
        return Cow::Borrowed(raw);
    }
    match shlex::split(raw) {
        // Substring matched but no token was actually a secret *flag* value
        // (e.g. a key literally named `access-key.txt`): nothing masked, so
        // return the original line untouched.
        Some(tokens) => match redact_tokens(&tokens) {
            Some(redacted) => Cow::Owned(redacted),
            None => Cow::Borrowed(raw),
        },
        // Unbalanced quoting: shlex can't tokenize. Best-effort scrub so a
        // secret on an otherwise-malformed line still does not leak.
        None => match redact_whitespace(raw) {
            Some(redacted) => Cow::Owned(redacted),
            None => Cow::Borrowed(raw),
        },
    }
}

/// Mask secret-flag values in a shlex token vector. Returns `Some(rejoined)`
/// when at least one value was masked, `None` when no token was a secret
/// flag (so the caller can keep the original line verbatim).
fn redact_tokens(tokens: &[String]) -> Option<String> {
    let mut out: Vec<Cow<str>> = Vec::with_capacity(tokens.len());
    let mut masked = false;
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i].as_str();
        // `--flag=value` form: mask everything after the first `=`.
        if tok.starts_with('-')
            && let Some(eq) = tok.find('=')
            && is_secret_flag(&tok[..eq])
        {
            out.push(Cow::Owned(format!("{}={REDACTED}", &tok[..eq])));
            masked = true;
            i += 1;
            continue;
        }
        // `--flag value` form: keep the flag, mask the following token.
        if tok.starts_with('-') && is_secret_flag(tok) {
            out.push(Cow::Borrowed(tok));
            if i + 1 < tokens.len() {
                out.push(Cow::Borrowed(REDACTED));
                masked = true;
                i += 2;
                continue;
            }
            // Secret flag with no following value (malformed line) — nothing
            // to mask for it.
            i += 1;
            continue;
        }
        out.push(Cow::Borrowed(tok));
        i += 1;
    }
    masked.then(|| join_tokens(&out))
}

/// Whitespace-level fallback used when a line cannot be shlex-tokenized
/// (unbalanced quotes) but still contains a secret-flag substring. Splits on
/// ASCII whitespace and masks the value after any secret flag, both
/// `--flag value` and `--flag=value`. Returns `Some` only when a value was
/// masked. Quoting is not interpreted — acceptable because this path is only
/// reached for an already-malformed line whose exact formatting is moot; the
/// sole goal is that the secret does not survive.
fn redact_whitespace(raw: &str) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    let mut masked = false;
    let mut expect_value = false;
    for word in raw.split_whitespace() {
        if expect_value {
            out.push(REDACTED.to_string());
            masked = true;
            expect_value = false;
            continue;
        }
        if word.starts_with('-')
            && let Some(eq) = word.find('=')
            && is_secret_flag(&word[..eq])
        {
            out.push(format!("{}={REDACTED}", &word[..eq]));
            masked = true;
            continue;
        }
        if word.starts_with('-') && is_secret_flag(word) {
            out.push(word.to_string());
            expect_value = true;
            continue;
        }
        out.push(word.to_string());
    }
    masked.then(|| out.join(" "))
}

/// Re-join redacted tokens into a single space-separated line. The result is
/// used only as the human-readable `raw` field on a log event — it is never
/// re-parsed or executed — so exact shell quoting is unnecessary (and shell
/// quoting would wrap the `****` placeholder in quotes). A value that
/// originally contained spaces simply appears as separate words; per the
/// [`redact_secrets`] contract, exact spacing/quoting is not preserved for a
/// redacted line.
fn join_tokens(tokens: &[Cow<str>]) -> String {
    tokens
        .iter()
        .map(|c| c.as_ref())
        .collect::<Vec<&str>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- is_secret_flag ----

    #[test]
    fn is_secret_flag_matches_every_credential_variant() {
        for flag in [
            "--target-access-key",
            "--source-access-key",
            "--target-secret-access-key",
            "--source-secret-access-key",
            "--target-session-token",
            "--source-session-token",
            "--source-sse-c-key",
            "--target-sse-c-key",
            "--source-sse-c-key-md5",
            "--target-sse-c-key-md5",
        ] {
            assert!(is_secret_flag(flag), "{flag} should be treated as secret");
        }
    }

    #[test]
    fn is_secret_flag_rejects_non_credential_flags() {
        for flag in [
            "--target-region",
            "--target-endpoint-url",
            "--dry-run",
            "--tagging",
            "--target-profile",
            "--json-tracing",
            "--",
            "-v",
        ] {
            assert!(!is_secret_flag(flag), "{flag} must not be masked");
        }
    }

    // ---- redact_secrets: nothing to redact ----

    #[test]
    fn no_secret_flag_returns_borrowed_unchanged() {
        let raw = "cp s3://b/key /tmp/dst --target-region us-east-1";
        let out = redact_secrets(raw);
        assert!(
            matches!(out, Cow::Borrowed(_)),
            "expected zero-alloc borrow"
        );
        assert_eq!(out, raw);
    }

    #[test]
    fn empty_line_is_borrowed_unchanged() {
        let out = redact_secrets("");
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out, "");
    }

    #[test]
    fn substring_in_non_flag_token_is_not_masked() {
        // A positional key that merely contains "access-key" must survive:
        // it is not a `--flag`, so nothing is redacted and the original line
        // is returned untouched.
        let raw = "cp s3://b/my-access-key-notes.txt /tmp/out";
        let out = redact_secrets(raw);
        assert_eq!(out, raw);
    }

    // ---- redact_secrets: `--flag value` form ----

    #[test]
    fn masks_secret_access_key_space_form() {
        let out = redact_secrets(
            "head-bucket s3://b --target-secret-access-key wJalrXUtnFEMISECRET --target-region us-east-1",
        );
        assert!(!out.contains("wJalrXUtnFEMISECRET"), "secret leaked: {out}");
        assert!(
            out.contains("--target-secret-access-key ****"),
            "got: {out}"
        );
        // Non-secret parts survive.
        assert!(out.contains("head-bucket"));
        assert!(out.contains("s3://b"));
        assert!(out.contains("--target-region us-east-1"));
    }

    #[test]
    fn masks_access_key_id_session_token_and_sse_c_key() {
        for (flag, value) in [
            ("--target-access-key", "AKIAIOSFODNN7EXAMPLE"),
            ("--source-session-token", "FQoGZXIvYXdzSESSIONTOKEN"),
            ("--source-sse-c-key", "MDEyMzQ1Njc4OTAxMjM0NTY3ODkwMTI="),
            ("--target-sse-c-key-md5", "abcdEXAMPLEmd5=="),
        ] {
            let raw = format!("cp s3://b/k /tmp/o {flag} {value}");
            let out = redact_secrets(&raw);
            assert!(!out.contains(value), "{flag}: secret leaked: {out}");
            assert!(out.contains(&format!("{flag} ****")), "{flag}: got: {out}");
        }
    }

    #[test]
    fn masks_multiple_secrets_in_one_line() {
        let out = redact_secrets(
            "cp /f s3://b/k --target-access-key AKIAEXAMPLEID --target-secret-access-key SECRETVAL --target-session-token TOKVAL",
        );
        assert!(!out.contains("AKIAEXAMPLEID"), "got: {out}");
        assert!(!out.contains("SECRETVAL"), "got: {out}");
        assert!(!out.contains("TOKVAL"), "got: {out}");
        assert_eq!(out.matches("****").count(), 3, "all three masked: {out}");
    }

    #[test]
    fn secret_flag_as_last_token_without_value_does_not_panic_or_change() {
        // Missing value (malformed line): there is no secret to mask, so the
        // line is returned unchanged rather than reconstructed.
        let raw = "create-bucket s3://b --target-secret-access-key";
        let out = redact_secrets(raw);
        assert_eq!(out, raw);
    }

    // ---- redact_secrets: `--flag=value` form ----

    #[test]
    fn masks_secret_equals_form() {
        let out = redact_secrets("head-bucket s3://b --target-secret-access-key=wJalrSECRETeq");
        assert!(!out.contains("wJalrSECRETeq"), "got: {out}");
        assert!(
            out.contains("--target-secret-access-key=****"),
            "got: {out}"
        );
    }

    #[test]
    fn masks_empty_equals_value() {
        // `--flag=` with an empty value still gets the placeholder; harmless
        // (no secret was present) and keeps the branch uniform.
        let out = redact_secrets("head-bucket s3://b --target-secret-access-key=");
        assert!(
            out.contains("--target-secret-access-key=****"),
            "got: {out}"
        );
    }

    // ---- redact_secrets: quoted values with spaces ----

    #[test]
    fn masks_quoted_value_with_spaces() {
        // Real AWS secrets never contain spaces, but a quoted value must not
        // slip through the token boundary: the whole quoted token is masked.
        let out = redact_secrets(r#"head-bucket s3://b --source-sse-c-key "a b c secret""#);
        assert!(!out.contains("a b c secret"), "got: {out}");
        assert!(out.contains("****"), "got: {out}");
    }

    // ---- redact_secrets: malformed (unbalanced quote) fallback ----

    #[test]
    fn masks_secret_even_when_line_cannot_be_tokenized() {
        // Unbalanced quote → shlex::split returns None → whitespace fallback.
        let out = redact_secrets(r#"cp --target-secret-access-key SECRETUNBALANCED "unterminated"#);
        assert!(!out.contains("SECRETUNBALANCED"), "secret leaked: {out}");
        assert!(out.contains("****"), "got: {out}");
    }

    #[test]
    fn malformed_line_without_secret_flag_is_unchanged() {
        // Substring present (in a non-flag token) AND unbalanced quotes: the
        // fallback finds no secret flag, so the line is returned as-is.
        let raw = r#"cp "unterminated s3://b/access-key-thing"#;
        let out = redact_secrets(raw);
        assert_eq!(out, raw);
    }

    // ---- identifiability: the subcommand token survives redaction ----

    #[test]
    fn redacted_line_keeps_leading_subcommand_token() {
        let out = redact_secrets("cp s3://b/k /tmp/o --target-secret-access-key SECRET");
        assert!(
            out.starts_with("cp "),
            "command must stay identifiable: {out}"
        );
    }

    #[test]
    fn non_secret_equals_flag_passes_through_while_secret_is_masked() {
        // A non-secret `--flag=value` (has `=` but is not a credential) must
        // survive untouched on a line that also carries a masked secret —
        // covers the `is_secret_flag(&tok[..eq]) == false` fall-through in the
        // `--flag=value` branch.
        let out = redact_secrets(
            "cp s3://b/k --target-region=us-east-1 --target-secret-access-key SECRETXYZ",
        );
        assert!(out.contains("--target-region=us-east-1"), "got: {out}");
        assert!(!out.contains("SECRETXYZ"), "secret leaked: {out}");
        assert!(
            out.contains("--target-secret-access-key ****"),
            "got: {out}"
        );
    }

    #[test]
    fn masks_equals_form_secret_when_line_cannot_be_tokenized() {
        // Malformed (unbalanced quote) line carrying the `--flag=value` secret
        // form: the whitespace fallback must still mask it (covers the
        // equals-form branch of `redact_whitespace`).
        let out = redact_secrets(r#"cp --target-secret-access-key=SECRETEQMALF "unterminated"#);
        assert!(!out.contains("SECRETEQMALF"), "secret leaked: {out}");
        assert!(
            out.contains("--target-secret-access-key=****"),
            "got: {out}"
        );
    }
}
