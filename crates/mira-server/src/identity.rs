// crates/mira-server/src/identity.rs
// User identity detection for multi-user memory sharing

use std::process::Command;

/// User identity information
#[derive(Debug, Clone)]
pub struct UserIdentity {
    /// Unique identifier (e.g., "John Doe <john@example.com>")
    pub identity: String,
    /// Display name extracted from identity
    pub display_name: Option<String>,
    /// Email extracted from identity
    pub email: Option<String>,
    /// Source of identity detection
    pub source: IdentitySource,
}

/// How the identity was determined
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySource {
    /// From git config (user.name + user.email)
    GitConfig,
    /// From MIRA_USER_ID environment variable
    Environment,
    /// From system username
    SystemUser,
    /// No identity could be determined
    Unknown,
}

impl UserIdentity {
    /// Detect current user identity using fallback chain:
    /// 1. Git config (user.name <user.email>)
    /// 2. MIRA_USER_ID environment variable
    /// 3. System username
    pub fn detect() -> Option<Self> {
        // Try git config first
        if let Some(identity) = Self::from_git_config() {
            return Some(identity);
        }

        // Try environment variable
        if let Some(identity) = Self::from_env() {
            return Some(identity);
        }

        // Fall back to system username
        Self::from_system_user()
    }

    /// Get identity from git config
    fn from_git_config() -> Option<Self> {
        let name = Command::new("git")
            .args(["config", "--get", "user.name"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())?;

        let email = Command::new("git")
            .args(["config", "--get", "user.email"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty());

        let identity = if let Some(ref e) = email {
            format!("{} <{}>", name, e)
        } else {
            name.clone()
        };

        Some(Self {
            identity,
            display_name: Some(name),
            email,
            source: IdentitySource::GitConfig,
        })
    }

    /// Get identity from MIRA_USER_ID environment variable
    fn from_env() -> Option<Self> {
        let identity = std::env::var("MIRA_USER_ID").ok().filter(|s| !s.is_empty())?;

        // Try to parse "Name <email>" format
        let (display_name, email) = parse_identity_string(&identity);

        Some(Self {
            identity,
            display_name,
            email,
            source: IdentitySource::Environment,
        })
    }

    /// Get identity from system username
    fn from_system_user() -> Option<Self> {
        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .ok()
            .filter(|s| !s.is_empty())?;

        Some(Self {
            identity: username.clone(),
            display_name: Some(username),
            email: None,
            source: IdentitySource::SystemUser,
        })
    }

    /// Get just the identity string for storage/comparison
    pub fn as_str(&self) -> &str {
        &self.identity
    }
}

/// Parse identity string in "Name <email>" format
fn parse_identity_string(s: &str) -> (Option<String>, Option<String>) {
    if let Some(bracket_start) = s.find('<') {
        if let Some(bracket_end) = s.find('>') {
            let name = s[..bracket_start].trim();
            let email = s[bracket_start + 1..bracket_end].trim();
            return (
                if name.is_empty() { None } else { Some(name.to_string()) },
                if email.is_empty() { None } else { Some(email.to_string()) },
            );
        }
    }
    // No email format, treat whole string as name
    (Some(s.to_string()), None)
}

/// Get current user identity string (convenience function)
pub fn get_current_user_identity() -> Option<String> {
    UserIdentity::detect().map(|u| u.identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_identity_string() {
        let (name, email) = parse_identity_string("John Doe <john@example.com>");
        assert_eq!(name, Some("John Doe".to_string()));
        assert_eq!(email, Some("john@example.com".to_string()));

        let (name, email) = parse_identity_string("Just Name");
        assert_eq!(name, Some("Just Name".to_string()));
        assert_eq!(email, None);

        let (name, email) = parse_identity_string("<email@only.com>");
        assert_eq!(name, None);
        assert_eq!(email, Some("email@only.com".to_string()));
    }

    #[test]
    fn test_detect_identity() {
        // This will use whatever is available on the system
        // Just verify it doesn't panic
        let identity = UserIdentity::detect();
        if let Some(id) = identity {
            assert!(!id.identity.is_empty());
        }
    }
}
