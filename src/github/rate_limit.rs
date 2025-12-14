//! GitHub API rate limit detection and error formatting
//!
//! This module handles rate limit information parsing from GitHub API headers
//! and formats error messages matching the Python version exactly.

use chrono::{DateTime, Utc};

/// GitHub rate limit information
///
/// Represents rate limit information parsed from GitHub API headers.
#[derive(Debug, Clone)]
pub struct GitHubRateLimitInfo {
    /// Total rate limit (60 unauthenticated, 5000 authenticated)
    pub limit: u32,
    /// Remaining requests
    pub remaining: u32,
    /// Reset time as Unix timestamp
    pub reset_epoch: i64,
    /// Reset time as DateTime
    pub reset_time: DateTime<Utc>,
    /// Optional Retry-After header value
    pub retry_after_seconds: Option<u64>,
}

impl GitHubRateLimitInfo {
    /// Parse rate limit info from HTTP response headers
    pub fn from_headers(headers: &reqwest::header::HeaderMap) -> Option<Self> {
        let limit = headers
            .get("x-ratelimit-limit")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok())?;

        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok())?;

        let reset_epoch = headers
            .get("x-ratelimit-reset")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<i64>().ok())?;

        let reset_time = DateTime::from_timestamp(reset_epoch, 0)?;

        let retry_after_seconds = headers
            .get("retry-after")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        Some(Self {
            limit,
            remaining,
            reset_epoch,
            reset_time,
            retry_after_seconds,
        })
    }

    /// Check if rate limit is exceeded
    pub fn is_exceeded(&self) -> bool {
        self.remaining == 0 && Utc::now() < self.reset_time
    }

    /// Format error message matching Python version
    pub fn format_error_message(&self, is_authenticated: bool) -> String {
        let mut lines = vec![];

        lines.push("Error: GitHub API rate limit exceeded".to_string());
        lines.push(String::new());

        if is_authenticated {
            lines.push(format!(
                "Rate limit: {}/{} requests used (authenticated)",
                self.limit - self.remaining,
                self.limit
            ));
        } else {
            lines.push(format!(
                "Rate limit: {}/{} requests used (unauthenticated)",
                self.limit - self.remaining,
                self.limit
            ));
        }

        lines.push(format!(
            "Reset time: {} UTC",
            self.reset_time.format("%Y-%m-%d %H:%M:%S")
        ));

        if let Some(retry_after) = self.retry_after_seconds {
            lines.push(format!("Retry after: {} seconds", retry_after));
        }

        lines.push(String::new());
        lines.push("To resolve:".to_string());
        lines.push("- Wait until reset time, or".to_string());

        if !is_authenticated {
            lines.push("- Use --github-token to increase limit to 5000/hour".to_string());
            lines.push(String::new());
            lines.push("Set token via:".to_string());
            lines.push("  aikit init --github-token <token>".to_string());
            lines.push("  or export GH_TOKEN=<token>".to_string());
        } else {
            lines.push("- Wait for rate limit reset".to_string());
        }

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_info_parsing() {
        use reqwest::header::{HeaderMap, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-limit", HeaderValue::from_static("60"));
        headers.insert("x-ratelimit-remaining", HeaderValue::from_static("0"));
        headers.insert("x-ratelimit-reset", HeaderValue::from_static("1765631145"));

        let info = GitHubRateLimitInfo::from_headers(&headers).unwrap();
        assert_eq!(info.limit, 60);
        assert_eq!(info.remaining, 0);
    }

    #[test]
    fn test_rate_limit_error_formatting() {
        let info = GitHubRateLimitInfo {
            limit: 60,
            remaining: 0,
            reset_epoch: 1765631145,
            reset_time: DateTime::from_timestamp(1765631145, 0).unwrap(),
            retry_after_seconds: Some(3600),
        };

        let message = info.format_error_message(false);
        assert!(message.contains("rate limit exceeded"));
        assert!(message.contains("--github-token"));
    }
}
