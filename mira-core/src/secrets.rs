//! Secret detection and redaction
//!
//! Detects and redacts common secret patterns in text output.

/// Secret pattern with description
pub struct SecretPattern {
    pub prefix: &'static str,
    pub kind: &'static str,
    pub min_len: usize,
}

/// Known secret patterns (case-insensitive prefix match)
pub const SECRET_PATTERNS: &[SecretPattern] = &[
    // Private keys
    SecretPattern { prefix: "-----BEGIN RSA PRIVATE KEY-----", kind: "private_key", min_len: 50 },
    SecretPattern { prefix: "-----BEGIN EC PRIVATE KEY-----", kind: "private_key", min_len: 50 },
    SecretPattern { prefix: "-----BEGIN OPENSSH PRIVATE KEY-----", kind: "private_key", min_len: 50 },
    SecretPattern { prefix: "-----BEGIN PGP PRIVATE KEY-----", kind: "private_key", min_len: 50 },
    SecretPattern { prefix: "-----BEGIN PRIVATE KEY-----", kind: "private_key", min_len: 50 },
    // API keys
    SecretPattern { prefix: "sk-proj-", kind: "openai_key", min_len: 20 },
    SecretPattern { prefix: "sk-ant-", kind: "anthropic_key", min_len: 20 },
    SecretPattern { prefix: "AIzaSy", kind: "google_api_key", min_len: 30 },
    // GitHub tokens
    SecretPattern { prefix: "ghp_", kind: "github_pat", min_len: 36 },
    SecretPattern { prefix: "github_pat_", kind: "github_pat", min_len: 40 },
    SecretPattern { prefix: "gho_", kind: "github_oauth", min_len: 36 },
    SecretPattern { prefix: "ghu_", kind: "github_user", min_len: 36 },
    SecretPattern { prefix: "ghs_", kind: "github_server", min_len: 36 },
    SecretPattern { prefix: "ghr_", kind: "github_refresh", min_len: 36 },
    // AWS
    SecretPattern { prefix: "AKIA", kind: "aws_access_key", min_len: 20 },
    SecretPattern { prefix: "aws_secret_access_key", kind: "aws_secret", min_len: 10 },
    // Environment patterns
    SecretPattern { prefix: "PRIVATE_KEY=", kind: "env_private_key", min_len: 20 },
    SecretPattern { prefix: "SECRET_KEY=", kind: "env_secret_key", min_len: 20 },
    SecretPattern { prefix: "API_KEY=", kind: "env_api_key", min_len: 15 },
    SecretPattern { prefix: "AUTH_TOKEN=", kind: "env_auth_token", min_len: 15 },
    // Stripe
    SecretPattern { prefix: "sk_live_", kind: "stripe_secret", min_len: 30 },
    SecretPattern { prefix: "sk_test_", kind: "stripe_test", min_len: 30 },
    SecretPattern { prefix: "rk_live_", kind: "stripe_restricted", min_len: 30 },
    // Slack
    SecretPattern { prefix: "xoxb-", kind: "slack_bot", min_len: 50 },
    SecretPattern { prefix: "xoxp-", kind: "slack_user", min_len: 50 },
    SecretPattern { prefix: "xoxa-", kind: "slack_app", min_len: 50 },
    // Discord
    SecretPattern { prefix: "mfa.", kind: "discord_token", min_len: 80 },
    // Twilio
    SecretPattern { prefix: "SK", kind: "twilio_key", min_len: 32 },
    // Generic patterns (last resort)
    SecretPattern { prefix: "bearer ", kind: "bearer_token", min_len: 20 },
    SecretPattern { prefix: "token=", kind: "generic_token", min_len: 20 },
    SecretPattern { prefix: "password=", kind: "password", min_len: 10 },
    SecretPattern { prefix: "passwd=", kind: "password", min_len: 10 },
];

/// Result of secret detection
#[derive(Debug, Clone)]
pub struct SecretMatch {
    pub kind: &'static str,
    pub offset: usize,
    pub length: usize,
}

/// Detect secrets in text, returns first match found
pub fn detect_secrets(text: &str) -> Option<SecretMatch> {
    let text_lower = text.to_lowercase();

    for pattern in SECRET_PATTERNS {
        if let Some(pos) = text_lower.find(&pattern.prefix.to_lowercase()) {
            // Check if there's enough content after the prefix
            let remaining = text.len() - pos;
            if remaining >= pattern.min_len {
                return Some(SecretMatch {
                    kind: pattern.kind,
                    offset: pos,
                    length: remaining.min(pattern.prefix.len() + 50), // Capture prefix + some content
                });
            }
        }
    }

    None
}

/// Detect all secrets in text
pub fn detect_all_secrets(text: &str) -> Vec<SecretMatch> {
    let text_lower = text.to_lowercase();
    let mut matches = Vec::new();

    for pattern in SECRET_PATTERNS {
        let mut search_start = 0;
        while let Some(pos) = text_lower[search_start..].find(&pattern.prefix.to_lowercase()) {
            let absolute_pos = search_start + pos;
            let remaining = text.len() - absolute_pos;

            if remaining >= pattern.min_len {
                matches.push(SecretMatch {
                    kind: pattern.kind,
                    offset: absolute_pos,
                    length: remaining.min(pattern.prefix.len() + 50),
                });
            }

            search_start = absolute_pos + pattern.prefix.len();
        }
    }

    // Sort by offset and dedupe overlapping matches
    matches.sort_by_key(|m| m.offset);
    matches
}

/// Redact secrets in text, replacing with [REDACTED: kind]
pub fn redact_secrets(text: &str) -> String {
    let matches = detect_all_secrets(text);
    if matches.is_empty() {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut last_end = 0;

    for secret in matches {
        // Skip if this overlaps with previous redaction
        if secret.offset < last_end {
            continue;
        }

        // Add text before this secret
        result.push_str(&text[last_end..secret.offset]);

        // Add redaction marker
        result.push_str(&format!("[REDACTED: {}]", secret.kind));

        // Find end of secret (until whitespace or end of line)
        let secret_text = &text[secret.offset..];
        let end_offset = secret_text
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '`')
            .unwrap_or(secret_text.len());

        last_end = secret.offset + end_offset;
    }

    // Add remaining text
    result.push_str(&text[last_end..]);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_openai_key() {
        let text = "API key: sk-proj-abc123def456xyz789";
        let result = detect_secrets(text);
        assert!(result.is_some());
        assert_eq!(result.unwrap().kind, "openai_key");
    }

    #[test]
    fn test_detect_github_pat() {
        let text = "token=ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let result = detect_secrets(text);
        assert!(result.is_some());
        assert_eq!(result.unwrap().kind, "github_pat");
    }

    #[test]
    fn test_detect_private_key() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...";
        let result = detect_secrets(text);
        assert!(result.is_some());
        assert_eq!(result.unwrap().kind, "private_key");
    }

    #[test]
    fn test_no_secrets() {
        let text = "Just some normal code without any secrets";
        let result = detect_secrets(text);
        assert!(result.is_none());
    }

    #[test]
    fn test_redact_secrets() {
        // Test with a clear OpenAI key (no API_KEY= prefix)
        let text = "token: sk-proj-abc123def456xyz789012345";
        let redacted = redact_secrets(text);
        assert!(redacted.contains("[REDACTED: openai_key]"));
        assert!(!redacted.contains("sk-proj-"));
    }

    #[test]
    fn test_redact_multiple() {
        let text = "ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa and sk-proj-bbbbbbbbbbbbbbbbbbbb";
        let redacted = redact_secrets(text);
        assert!(redacted.contains("[REDACTED: github_pat]"));
        assert!(redacted.contains("[REDACTED: openai_key]"));
        assert!(!redacted.contains("ghp_"));
        assert!(!redacted.contains("sk-proj-"));
    }
}
