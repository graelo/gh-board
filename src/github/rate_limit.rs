//! Rate-limit detection utilities for GitHub API responses.
//!
//! GitHub signals rate limits through:
//! - HTTP 403 with "API rate limit exceeded" in the body
//! - HTTP 429 (secondary rate limit)
//! - GraphQL errors containing "rate limit"

/// Check whether an error message indicates a GitHub rate limit.
#[allow(dead_code)]
pub(crate) fn is_rate_limited(error: &anyhow::Error) -> bool {
    let msg = format!("{error:#}").to_lowercase();
    msg.contains("rate limit")
        || msg.contains("api rate limit exceeded")
        || msg.contains("secondary rate limit")
        || msg.contains("status code: 429")
        || msg.contains("status code: 403")
}

/// Format a user-friendly message for a rate-limit error.
#[allow(dead_code)]
pub(crate) fn format_rate_limit_message(error: &anyhow::Error) -> String {
    let msg = format!("{error:#}");
    if msg.to_lowercase().contains("secondary rate limit") {
        "Secondary rate limit hit — wait a moment then press [r] to retry".to_owned()
    } else {
        "API rate limit exceeded — press [r] to retry".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn detects_rate_limit_exceeded() {
        let err = anyhow!("API rate limit exceeded for user");
        assert!(is_rate_limited(&err));
    }

    #[test]
    fn detects_secondary_rate_limit() {
        let err = anyhow!("You have exceeded a secondary rate limit");
        assert!(is_rate_limited(&err));
    }

    #[test]
    fn detects_429_status() {
        let err = anyhow!("HTTP status code: 429");
        assert!(is_rate_limited(&err));
    }

    #[test]
    fn non_rate_limit_error_is_not_detected() {
        let err = anyhow!("network timeout");
        assert!(!is_rate_limited(&err));
    }

    #[test]
    fn format_secondary_rate_limit() {
        let err = anyhow!("secondary rate limit exceeded");
        let msg = format_rate_limit_message(&err);
        assert!(msg.contains("Secondary rate limit"));
    }

    #[test]
    fn format_primary_rate_limit() {
        let err = anyhow!("API rate limit exceeded");
        let msg = format_rate_limit_message(&err);
        assert!(msg.contains("rate limit exceeded"));
    }
}
