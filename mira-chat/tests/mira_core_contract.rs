//! Contract tests for mira-core shared functionality
//!
//! These tests ensure mira-chat and mira-core stay in sync.
//! If these fail, it means shared behavior has drifted.

use mira_core::{
    detect_secrets, redact_secrets,
    create_grep_excerpt, create_diff_excerpt, create_smart_excerpt,
    safe_utf8_slice,
    ARTIFACT_THRESHOLD_BYTES, INLINE_MAX_BYTES, MAX_ARTIFACT_SIZE,
    MAX_GREP_MATCHES, MAX_DIFF_FILES, EXCERPT_HEAD_CHARS, EXCERPT_TAIL_CHARS,
};

// ============================================================================
// Secret Detection Contract
// ============================================================================

#[test]
fn contract_secret_detection_openai_key() {
    // OpenAI keys must be detected
    let text = "key: sk-proj-abc123def456xyz789012345";
    let result = detect_secrets(text);
    assert!(result.is_some(), "OpenAI key should be detected");
    assert_eq!(result.unwrap().kind, "openai_key");
}

#[test]
fn contract_secret_detection_github_pat() {
    // GitHub PATs must be detected
    let text = "token=ghp_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
    let result = detect_secrets(text);
    assert!(result.is_some(), "GitHub PAT should be detected");
    assert_eq!(result.unwrap().kind, "github_pat");
}

#[test]
fn contract_secret_detection_private_key() {
    // Private keys must be detected
    let text = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...";
    let result = detect_secrets(text);
    assert!(result.is_some(), "Private key should be detected");
    assert_eq!(result.unwrap().kind, "private_key");
}

#[test]
fn contract_secret_detection_anthropic_key() {
    // Anthropic keys must be detected
    let text = "ANTHROPIC_API_KEY=sk-ant-api03-xxxxxxxxxxxxx";
    let result = detect_secrets(text);
    assert!(result.is_some(), "Anthropic key should be detected");
    assert_eq!(result.unwrap().kind, "anthropic_key");
}

#[test]
fn contract_secret_detection_false_negatives() {
    // Normal text should NOT trigger detection
    let safe_texts = [
        "just some normal code",
        "const apiKey = process.env.API_KEY",
        "sk-not-long-enough",  // Too short
        "ghp_short",  // Too short
    ];
    for text in safe_texts {
        let result = detect_secrets(text);
        assert!(result.is_none(), "False positive on: {}", text);
    }
}

#[test]
fn contract_secret_redaction() {
    // Redaction must replace secrets with markers
    let text = "token: sk-proj-abc123def456xyz789012345 end";
    let redacted = redact_secrets(text);
    assert!(redacted.contains("[REDACTED:"), "Should contain redaction marker");
    assert!(!redacted.contains("sk-proj-"), "Should not contain secret prefix");
}

// ============================================================================
// Excerpt Shape Contract
// ============================================================================

#[test]
fn contract_grep_excerpt_shape() {
    // Grep excerpts must show top N matches with "more matches" note
    let lines: Vec<String> = (1..=50).map(|i| format!("file.rs:{}:match {}", i, i)).collect();
    let content = lines.join("\n");

    let excerpt = create_grep_excerpt(&content, 10);

    // Must contain first matches
    assert!(excerpt.contains("file.rs:1:match 1"), "Should contain first match");
    assert!(excerpt.contains("file.rs:10:match 10"), "Should contain 10th match");

    // Must NOT contain later matches
    assert!(!excerpt.contains("file.rs:11:match 11"), "Should not contain 11th match");

    // Must have truncation note
    assert!(excerpt.contains("more matches"), "Should indicate more matches available");
}

#[test]
fn contract_grep_excerpt_short_passthrough() {
    // Short grep output should pass through unchanged
    let short = "file.rs:1:match";
    let excerpt = create_grep_excerpt(short, MAX_GREP_MATCHES);
    assert_eq!(excerpt, short, "Short content should pass through");
}

#[test]
fn contract_diff_excerpt_shape() {
    // Diff excerpts must show file headers + first hunk
    let diff = r#"diff --git a/foo.rs b/foo.rs
index abc123..def456 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,5 +1,6 @@
 fn main() {
+    println!("hello");
 }
diff --git a/bar.rs b/bar.rs
index 111..222 100644
--- a/bar.rs
+++ b/bar.rs
@@ -1,2 +1,3 @@
+// comment
 fn bar() {}
"#;

    let excerpt = create_diff_excerpt(diff, 1);

    // Must contain first file
    assert!(excerpt.contains("diff --git a/foo.rs"), "Should contain first file header");
    assert!(excerpt.contains("println!"), "Should contain first file changes");

    // Must NOT contain second file content
    assert!(!excerpt.contains("diff --git a/bar.rs"), "Should not contain second file");

    // Must note additional files
    assert!(excerpt.contains("more files"), "Should indicate more files");
}

#[test]
fn contract_diff_excerpt_non_diff_passthrough() {
    // Non-diff content should pass through unchanged
    let not_diff = "this is not a diff";
    let excerpt = create_diff_excerpt(not_diff, MAX_DIFF_FILES);
    assert_eq!(excerpt, not_diff, "Non-diff content should pass through");
}

#[test]
fn contract_smart_excerpt_routing() {
    // Smart excerpt must route to correct handler
    let long_content = "x".repeat(5000);

    // Default handler for unknown tools
    let bash_excerpt = create_smart_excerpt("bash", &long_content);
    assert!(bash_excerpt.contains("truncated"), "Should use default truncation");
}

// ============================================================================
// UTF-8 Safety Contract
// ============================================================================

#[test]
fn contract_utf8_slice_basic() {
    let text = "hello world";
    let (slice, start, end) = safe_utf8_slice(text, 0, 5);
    assert_eq!(slice, "hello");
    assert_eq!(start, 0);
    assert_eq!(end, 5);
}

#[test]
fn contract_utf8_slice_unicode_safety() {
    // Must not panic or corrupt on multi-byte chars
    let text = "héllo wörld 日本語";

    // Slice through middle of multi-byte char should adjust boundaries
    let (slice, start, _) = safe_utf8_slice(text, 1, 10);
    assert!(text.is_char_boundary(start), "Start must be valid char boundary");
    assert!(slice.is_ascii() || slice.chars().all(|c| c.len_utf8() >= 1), "Result must be valid UTF-8");
}

#[test]
fn contract_utf8_slice_past_end() {
    let text = "short";
    let (slice, _, _) = safe_utf8_slice(text, 100, 50);
    assert_eq!(slice, "", "Past-end slice should be empty");
}

// ============================================================================
// Limits Contract
// ============================================================================

#[test]
fn contract_limits_values() {
    // These limits are part of the API contract - changes require migration
    assert_eq!(INLINE_MAX_BYTES, 2048, "Inline max should be 2KB");
    assert_eq!(ARTIFACT_THRESHOLD_BYTES, 4096, "Artifact threshold should be 4KB");
    assert_eq!(MAX_ARTIFACT_SIZE, 10 * 1024 * 1024, "Max artifact should be 10MB");
    assert_eq!(MAX_GREP_MATCHES, 20, "Max grep matches should be 20");
    assert_eq!(MAX_DIFF_FILES, 10, "Max diff files should be 10");
    assert_eq!(EXCERPT_HEAD_CHARS, 1200, "Excerpt head should be 1200 chars");
    assert_eq!(EXCERPT_TAIL_CHARS, 800, "Excerpt tail should be 800 chars");
}

#[test]
fn contract_limits_relationships() {
    // Sanity checks on limit relationships
    assert!(ARTIFACT_THRESHOLD_BYTES > INLINE_MAX_BYTES,
        "Artifact threshold must exceed inline max");
    assert!(MAX_ARTIFACT_SIZE > ARTIFACT_THRESHOLD_BYTES,
        "Max artifact must exceed threshold");
    assert!(EXCERPT_HEAD_CHARS > EXCERPT_TAIL_CHARS,
        "Head excerpt should be larger than tail");
}
