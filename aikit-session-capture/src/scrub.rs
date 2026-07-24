//! `SecretScrubber`: the single chokepoint that enforces "no secrets escape
//! the adapter". Every `ToolEvent`'s `input` and `output` passes through
//! `scrub()` before the parse result is returned.
//!
//! See spec 010 §9. Default patterns cover the common credential shapes;
//! hosts add their own via [`SecretScrubber::with_pattern`].

use regex::Regex;

/// Bump when `SecretScrubber::default_patterns()` changes.
pub const SCRUBBER_PATTERN_VERSION: u32 = 1;

/// Replaces credential matches in adapter output with `[REDACTED:<label>]`.
///
/// Cloning is cheap (patterns are shared via `Arc`). The same scrubber
/// instance SHOULD be shared across all adapters built from one
/// [`Registry`][crate::Registry].
#[derive(Debug, Clone)]
pub struct SecretScrubber {
    patterns: Vec<(Regex, &'static str)>,
}

impl Default for SecretScrubber {
    fn default() -> Self {
        Self::default_patterns()
    }
}

impl SecretScrubber {
    /// Build with the default credential patterns: AWS access key + secret,
    /// GitHub PAT, JWT, Bearer token, private key block, Anthropic + OpenAI
    /// API key prefixes, connection strings with embedded passwords.
    pub fn default_patterns() -> Self {
        // Patterns are intentionally conservative: prefer false negatives
        // (let a non-secret through) over false positives (corrupt a legit
        // value). Hosts tighten with `with_pattern`.
        let raw: &[(&str, &str)] = &[
            // AWS access key id: AKIA + 16 uppercase alphanumerics.
            (r"AKIA[0-9A-Z]{16}", "aws_access_key"),
            // AWS secret access key: 40 base64 chars after a heuristic anchor.
            // Anchor on `aws_secret` or a quoted key to avoid matching
            // arbitrary base64 blobs (e.g. embedded file hashes).
            (
                r#"(?i)(aws_secret(?:_access)?_key["'\s:=]+)["A-Za-z0-9/+=]{40}"#,
                "aws_secret",
            ),
            // GitHub tokens: PAT (ghp_), OAuth (gho_), user (ghu_), server (ghs_),
            // refresh (ghr_), fine-grained (github_pat_). 36+ chars.
            (r"gh[pousr]_[A-Za-z0-9]{36,}", "github_pat"),
            (r"github_pat_[A-Za-z0-9_]{22,}", "github_pat"),
            // JWT: three base64url segments separated by dots. The header
            // starts with ey... (base64 of `{"...`) which is a strong anchor.
            (
                r"eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
                "jwt",
            ),
            // Bearer token in an Authorization header.
            (r"(?i)bearer\s+[A-Za-z0-9._\-+/=]{20,}", "bearer_token"),
            // PEM private key block (any algorithm).
            (
                r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
                "private_key",
            ),
            // Anthropic API key.
            (r"sk-ant-[A-Za-z0-9_\-]{20,}", "anthropic_api_key"),
            // OpenAI API key (project-scoped and legacy).
            (r"sk-proj-[A-Za-z0-9_\-]{20,}", "openai_api_key"),
            (r"sk-[A-Za-z0-9]{20,}", "openai_api_key"),
            // Connection string with embedded password.
            // Covers: postgres://user:pass@host, mongodb://user:pass@host, etc.
            (
                r#"(?i)(?:postgres|mysql|mongodb|redis|amqp|mssql)://[^:/\s"']+:[^@/\s"']+@"#,
                "connstring_password",
            ),
        ];
        let patterns = raw
            .iter()
            .filter_map(|(re, label)| Regex::new(re).ok().map(|r| (r, *label)))
            .collect();
        Self { patterns }
    }

    /// Add a caller-supplied pattern. The scrubber is shared across all
    /// adapters built from the same registry, so adding a pattern affects
    /// every adapter.
    pub fn with_pattern(mut self, re: Regex, label: &'static str) -> Self {
        self.patterns.push((re, label));
        self
    }

    /// Replace every credential match with `[REDACTED:<label>]`. The
    /// original bytes are dropped — never logged, never persisted.
    pub fn scrub(&self, input: &str) -> String {
        let mut out = input.to_string();
        for (re, label) in &self.patterns {
            // Regex::replace_all with a literal replacement; the label is
            // caller-controlled (defaults are static strings).
            out = re
                .replace_all(&out, format!("[REDACTED:{}]", label))
                .into_owned();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_patterns_match_canonical_fixtures() {
        let s = SecretScrubber::default();
        // AWS access key
        assert!(s
            .scrub("AKIAIOSFODNN7EXAMPLE")
            .contains("[REDACTED:aws_access_key]"));
        // GitHub PAT
        assert!(s
            .scrub("ghp_0123456789012345678901234567890abcdefgh")
            .contains("[REDACTED:github_pat]"));
        // JWT (three base64url segments)
        assert!(s
            .scrub("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIx.eyJpc3MiOiJ4")
            .contains("[REDACTED:jwt]"));
        // Bearer token
        assert!(s
            .scrub("Authorization: Bearer abcdefghijklmnopqrstuvwxyz123456")
            .contains("[REDACTED:bearer_token]"));
        // Private key block
        let pk =
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
        assert!(s.scrub(pk).contains("[REDACTED:private_key]"));
        // Anthropic
        assert!(s
            .scrub("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789")
            .contains("[REDACTED:anthropic_api_key]"));
        // Connection string
        assert!(s
            .scrub("postgres://user:secretpass@host:5432/db")
            .contains("[REDACTED:connstring_password]"));
    }

    #[test]
    fn non_secrets_pass_through() {
        let s = SecretScrubber::default();
        let benign = "Read /home/u/repo/src/main.rs — no secrets here, just code.";
        assert_eq!(s.scrub(benign), benign);
        // Base64-looking but not anchored as AWS secret:
        let hash = "sha256: a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        // The connection-string pattern won't match (no scheme://), the AWS
        // secret pattern won't match (no `aws_secret` anchor) — passes through.
        assert_eq!(s.scrub(hash), hash);
    }

    #[test]
    fn with_pattern_adds_custom() {
        let s = SecretScrubber::default()
            .with_pattern(Regex::new(r"MY_SECRET=\w+").unwrap(), "my_custom");
        assert!(s.scrub("MY_SECRET=abcdef").contains("[REDACTED:my_custom]"));
        // Default patterns still active.
        assert!(s
            .scrub("ghp_0123456789012345678901234567890abcdefgh")
            .contains("[REDACTED:github_pat]"));
    }
}
