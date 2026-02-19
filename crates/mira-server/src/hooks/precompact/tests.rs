// crates/mira-server/src/hooks/precompact/tests.rs
// Tests for precompact hook: transcript parsing, context extraction, merging.

use super::extract::{
    DECISION_KEYWORDS, ISSUE_KEYWORDS, TASK_KEYWORDS, is_continuation_prompt, matches_any,
    matches_issue_keyword,
};
use super::*;
use std::path::PathBuf;

// ── parse_transcript_messages ──────────────────────────────────────────

#[test]
fn parses_string_content() {
    let transcript = r#"{"role":"assistant","content":"I decided to use the builder pattern."}"#;
    let messages = parse_transcript_messages(transcript);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, "assistant");
    assert!(messages[0].text_content.contains("builder pattern"));
}

#[test]
fn parses_array_content_blocks() {
    let transcript = r#"{"role":"assistant","content":[{"type":"text","text":"First block."},{"type":"text","text":"Second block."}]}"#;
    let messages = parse_transcript_messages(transcript);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].text_content.contains("First block."));
    assert!(messages[0].text_content.contains("Second block."));
}

#[test]
fn filters_tool_use_blocks() {
    let transcript = r#"{"role":"assistant","content":[{"type":"text","text":"Let me check."},{"type":"tool_use","id":"t1","name":"Read","input":{}}]}"#;
    let messages = parse_transcript_messages(transcript);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].text_content.contains("Let me check."));
    assert!(!messages[0].text_content.contains("tool_use"));
}

#[test]
fn filters_tool_result_blocks() {
    let transcript = r#"{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"file contents"},{"type":"text","text":"Please continue."}]}"#;
    let messages = parse_transcript_messages(transcript);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].text_content.contains("Please continue."));
    assert!(!messages[0].text_content.contains("file contents"));
}

#[test]
fn skips_system_role() {
    let transcript = r#"{"role":"system","content":"You are a helpful assistant."}"#;
    let messages = parse_transcript_messages(transcript);
    assert!(messages.is_empty());
}

#[test]
fn skips_malformed_jsonl_lines() {
    let transcript = "not json at all\n{\"role\":\"assistant\",\"content\":\"valid line\"}";
    let messages = parse_transcript_messages(transcript);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, "assistant");
}

#[test]
fn handles_empty_transcript() {
    let messages = parse_transcript_messages("");
    assert!(messages.is_empty());
}

#[test]
fn parses_both_user_and_assistant_roles() {
    let transcript = "{\"role\":\"user\",\"content\":\"Hello\"}\n{\"role\":\"assistant\",\"content\":\"Hi there\"}";
    let messages = parse_transcript_messages(transcript);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[1].role, "assistant");
}

#[test]
fn skips_messages_with_empty_content() {
    let transcript = r#"{"role":"assistant","content":""}"#;
    let messages = parse_transcript_messages(transcript);
    assert!(messages.is_empty());
}

#[test]
fn skips_empty_lines_in_transcript() {
    let transcript = "\n\n{\"role\":\"assistant\",\"content\":\"hello\"}\n\n";
    let messages = parse_transcript_messages(transcript);
    assert_eq!(messages.len(), 1);
}

// ── extract_compaction_context ─────────────────────────────────────────

#[test]
fn extracts_decisions() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "We decided to use the builder pattern for config structs.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
    assert!(ctx.decisions[0].contains("decided to"));
}

#[test]
fn extracts_will_use_keyword() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "We will use tokio for async runtime in this project.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

#[test]
fn extracts_approach_keyword() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "approach: batch inserts into a single transaction for performance."
            .to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

#[test]
fn extracts_pending_tasks() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "TODO: add validation for user input in the handler.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.pending_tasks.len(), 1);
    assert!(ctx.pending_tasks[0].contains("TODO:"));
}

#[test]
fn extracts_next_step_keyword() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "The next step is implementing the migration system.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.pending_tasks.len(), 1);
}

#[test]
fn extracts_remaining_keyword() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "remaining: three modules still need refactoring work.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    // Matches both "remaining:" and "still need to"
    assert!(!ctx.pending_tasks.is_empty());
}

#[test]
fn extracts_still_need_to_keyword() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "We still need to add error handling to the API layer.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.pending_tasks.len(), 1);
}

#[test]
fn extracts_issues() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "error: connection refused when connecting to database.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.issues.len(), 1);
    assert!(ctx.issues[0].contains("error:"));
}

#[test]
fn extracts_failed_keyword() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "The migration failed: column already exists in table.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.issues.len(), 1);
}

#[test]
fn extracts_bug_keyword() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "bug: duplicate entries created when session restarts.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.issues.len(), 1);
}

#[test]
fn extracts_active_work_from_last_assistant() {
    let messages = vec![
        TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "First assistant message paragraph.".to_string(),
        },
        TranscriptMessage {
            role: "user".to_string(),
            text_content: "User question about the code.".to_string(),
        },
        TranscriptMessage {
            role: "assistant".to_string(),
            text_content:
                "Working on the database migration now.\n\nHere are the details of the change."
                    .to_string(),
        },
    ];
    let ctx = extract_compaction_context(&messages);
    // Now captures up to 2 paragraphs from the last assistant message
    assert_eq!(ctx.active_work.len(), 2);
    assert!(ctx.active_work[0].contains("database migration"));
    assert!(ctx.active_work[1].contains("details of the change"));
}

#[test]
fn caps_items_per_category() {
    let mut paragraphs = Vec::new();
    for i in 0..10 {
        paragraphs.push(format!(
            "We decided to implement feature number {} for testing.",
            i
        ));
    }
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: paragraphs.join("\n\n"),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), MAX_ITEMS_PER_CATEGORY);
}

#[test]
fn filters_short_paragraphs() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "error: x".to_string(), // 8 chars, below MIN_CONTENT_LEN
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.issues.is_empty());
    assert!(ctx.active_work.is_empty());
}

#[test]
fn truncates_long_paragraphs_instead_of_dropping() {
    // "error: " is in the prefix, so even after truncation the keyword matches
    let long_text = format!("error: {}", "x".repeat(800));
    assert!(long_text.len() > MAX_CONTENT_LEN);
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: long_text,
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.issues.len(), 1);
    // The stored text should be truncated to MAX_CONTENT_LEN
    assert!(ctx.issues[0].len() <= MAX_CONTENT_LEN);
}

#[test]
fn accepts_paragraph_at_max_content_len() {
    // "error: " is 7 chars, so pad to exactly MAX_CONTENT_LEN (800)
    let text = format!("error: {}", "x".repeat(MAX_CONTENT_LEN - 7));
    assert_eq!(text.len(), MAX_CONTENT_LEN);
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: text,
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.issues.len(), 1);
}

#[test]
fn case_insensitive_matching() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "DECIDED TO use uppercase keywords in this test.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

#[test]
fn empty_messages_returns_empty_context() {
    let ctx = extract_compaction_context(&[]);
    assert!(ctx.is_empty());
}

#[test]
fn captures_active_work_even_without_keyword_matches() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "This is a normal conversation with no special keywords.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.decisions.is_empty());
    assert!(ctx.issues.is_empty());
    assert!(ctx.pending_tasks.is_empty());
    // Active work captures last assistant's first paragraph regardless of keywords
    assert_eq!(ctx.active_work.len(), 1);
}

#[test]
fn multiple_categories_in_one_message() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content:
            "We decided to refactor the database layer.\n\nTODO: update the migration scripts for the schema.\n\nerror: failed to connect to the test database."
                .to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
    assert_eq!(ctx.pending_tasks.len(), 1);
    assert_eq!(ctx.issues.len(), 1);
}

#[test]
fn mixed_case_keywords_matched() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "Decided To go with the new approach for handling.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

// ── CompactionContext methods ──────────────────────────────────────────

#[test]
fn is_empty_when_all_fields_empty() {
    let ctx = CompactionContext::default();
    assert!(ctx.is_empty());
    assert_eq!(ctx.total_items(), 0);
}

#[test]
fn not_empty_with_decisions() {
    let mut ctx = CompactionContext::default();
    ctx.decisions.push("decided something".to_string());
    assert!(!ctx.is_empty());
    assert_eq!(ctx.total_items(), 1);
}

#[test]
fn total_items_counts_all_categories() {
    let ctx = CompactionContext {
        decisions: vec!["d1".into(), "d2".into()],
        active_work: vec!["a1".into()],
        issues: vec!["i1".into()],
        pending_tasks: vec!["p1".into(), "p2".into(), "p3".into()],
        user_intent: Some("intent".into()),
        files_referenced: vec!["src/main.rs".into()],
    };
    // 2 + 1 + 1 + 3 + 1 (intent) + 1 (file) = 9
    assert_eq!(ctx.total_items(), 9);
}

// ── Serialization round-trip ──────────────────────────────────────────

#[test]
fn compaction_context_serializes_and_deserializes() {
    let ctx = CompactionContext {
        decisions: vec!["chose builder pattern".into()],
        active_work: vec!["working on migration".into()],
        issues: vec!["connection refused".into()],
        pending_tasks: vec!["add validation".into()],
        user_intent: Some("Fix the auth bug".into()),
        files_referenced: vec!["src/main.rs".into(), "src/lib.rs".into()],
    };
    let json = serde_json::to_value(&ctx).unwrap();
    let roundtrip: CompactionContext = serde_json::from_value(json).unwrap();
    assert_eq!(roundtrip.decisions, ctx.decisions);
    assert_eq!(roundtrip.active_work, ctx.active_work);
    assert_eq!(roundtrip.issues, ctx.issues);
    assert_eq!(roundtrip.pending_tasks, ctx.pending_tasks);
    assert_eq!(roundtrip.user_intent, ctx.user_intent);
    assert_eq!(roundtrip.files_referenced, ctx.files_referenced);
}

// ── merge_compaction_contexts ────────────────────────────────────────

#[test]
fn merge_combines_vec_fields() {
    let existing = serde_json::to_value(CompactionContext {
        decisions: vec!["decision A".into()],
        active_work: vec!["work A".into()],
        issues: vec![],
        pending_tasks: vec![],
        user_intent: None,
        files_referenced: vec!["src/a.rs".into()],
    })
    .unwrap();
    let new = serde_json::to_value(CompactionContext {
        decisions: vec!["decision B".into()],
        active_work: vec!["work B".into()],
        issues: vec!["issue B".into()],
        pending_tasks: vec!["task B".into()],
        user_intent: None,
        files_referenced: vec!["src/b.rs".into()],
    })
    .unwrap();
    let merged: CompactionContext =
        serde_json::from_value(merge_compaction_contexts(&existing, &new)).unwrap();
    assert_eq!(merged.decisions, vec!["decision A", "decision B"]);
    assert_eq!(merged.active_work, vec!["work A", "work B"]);
    assert_eq!(merged.issues, vec!["issue B"]);
    assert_eq!(merged.pending_tasks, vec!["task B"]);
    assert_eq!(merged.files_referenced, vec!["src/a.rs", "src/b.rs"]);
}

#[test]
fn merge_deduplicates_exact_strings() {
    let existing = serde_json::to_value(CompactionContext {
        decisions: vec!["dup".into(), "unique old".into()],
        ..Default::default()
    })
    .unwrap();
    let new = serde_json::to_value(CompactionContext {
        decisions: vec!["dup".into(), "unique new".into()],
        ..Default::default()
    })
    .unwrap();
    let merged: CompactionContext =
        serde_json::from_value(merge_compaction_contexts(&existing, &new)).unwrap();
    assert_eq!(merged.decisions, vec!["unique old", "dup", "unique new"]);
}

#[test]
fn merge_keeps_first_user_intent() {
    let existing = serde_json::to_value(CompactionContext {
        user_intent: Some("original intent".into()),
        ..Default::default()
    })
    .unwrap();
    let new = serde_json::to_value(CompactionContext {
        user_intent: Some("later intent".into()),
        ..Default::default()
    })
    .unwrap();
    let merged: CompactionContext =
        serde_json::from_value(merge_compaction_contexts(&existing, &new)).unwrap();
    assert_eq!(merged.user_intent.as_deref(), Some("original intent"));
}

#[test]
fn merge_sets_intent_when_existing_is_none() {
    let existing = serde_json::to_value(CompactionContext {
        user_intent: None,
        ..Default::default()
    })
    .unwrap();
    let new = serde_json::to_value(CompactionContext {
        user_intent: Some("first real intent".into()),
        ..Default::default()
    })
    .unwrap();
    let merged: CompactionContext =
        serde_json::from_value(merge_compaction_contexts(&existing, &new)).unwrap();
    assert_eq!(merged.user_intent.as_deref(), Some("first real intent"));
}

#[test]
fn merge_caps_vec_at_max_items() {
    let existing = serde_json::to_value(CompactionContext {
        decisions: (0..4).map(|i| format!("old decision {i}")).collect(),
        ..Default::default()
    })
    .unwrap();
    let new = serde_json::to_value(CompactionContext {
        decisions: (0..4).map(|i| format!("new decision {i}")).collect(),
        ..Default::default()
    })
    .unwrap();
    let merged: CompactionContext =
        serde_json::from_value(merge_compaction_contexts(&existing, &new)).unwrap();
    // MAX_ITEMS_PER_CATEGORY = 5, 8 unique items -> keep last 5
    assert_eq!(merged.decisions.len(), MAX_ITEMS_PER_CATEGORY);
    // Should prefer recent (last 5 of combined)
    assert_eq!(merged.decisions[4], "new decision 3");
}

#[test]
fn merge_caps_files_at_max_file_refs() {
    let existing = serde_json::to_value(CompactionContext {
        files_referenced: (0..8).map(|i| format!("src/old_{i}.rs")).collect(),
        ..Default::default()
    })
    .unwrap();
    let new = serde_json::to_value(CompactionContext {
        files_referenced: (0..8).map(|i| format!("src/new_{i}.rs")).collect(),
        ..Default::default()
    })
    .unwrap();
    let merged: CompactionContext =
        serde_json::from_value(merge_compaction_contexts(&existing, &new)).unwrap();
    // MAX_FILE_REFS = 10, 16 unique items -> keep last 10
    assert_eq!(merged.files_referenced.len(), MAX_FILE_REFS);
    assert_eq!(merged.files_referenced[9], "src/new_7.rs");
}

#[test]
fn merge_handles_empty_existing() {
    let existing = serde_json::json!({});
    let new = serde_json::to_value(CompactionContext {
        decisions: vec!["decision A".into()],
        user_intent: Some("intent".into()),
        ..Default::default()
    })
    .unwrap();
    let merged: CompactionContext =
        serde_json::from_value(merge_compaction_contexts(&existing, &new)).unwrap();
    assert_eq!(merged.decisions, vec!["decision A"]);
    assert_eq!(merged.user_intent.as_deref(), Some("intent"));
}

#[test]
fn merge_handles_null_existing() {
    let existing = serde_json::Value::Null;
    let new = serde_json::to_value(CompactionContext {
        issues: vec!["bug".into()],
        ..Default::default()
    })
    .unwrap();
    let merged: CompactionContext =
        serde_json::from_value(merge_compaction_contexts(&existing, &new)).unwrap();
    assert_eq!(merged.issues, vec!["bug"]);
}

// ── merge_vec_field ─────────────────────────────────────────────────

#[test]
fn merge_vec_field_basic_combine() {
    let result = merge_vec_field(&["a".into()], &["b".into()], 5);
    assert_eq!(result, vec!["a", "b"]);
}

#[test]
fn merge_vec_field_dedup_keeps_later() {
    let result = merge_vec_field(&["x".into(), "y".into()], &["y".into(), "z".into()], 5);
    // "y" appears in both -- the later (new) occurrence wins position
    assert_eq!(result, vec!["x", "y", "z"]);
}

#[test]
fn merge_vec_field_trims_oldest() {
    let result = merge_vec_field(
        &["a".into(), "b".into(), "c".into()],
        &["d".into(), "e".into()],
        3,
    );
    // 5 unique items, max 3 -> keep last 3
    assert_eq!(result, vec!["c", "d", "e"]);
}

// ── Transcript path validation ───────────────────────────────────────

#[test]
fn validate_transcript_path_under_tmp() {
    let path = PathBuf::from("/tmp/claude/transcript.jsonl");
    assert!(path.starts_with("/tmp"));
}

#[test]
fn validate_transcript_path_rejects_arbitrary_path() {
    let path = PathBuf::from("/etc/passwd");
    assert!(!path.starts_with("/tmp"));
}

// ── New keyword coverage ──────────────────────────────────────────────

#[test]
fn extracts_i_chose_decision() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "I chose thiserror over anyhow for the public API surface.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

#[test]
fn extracts_lets_go_with_decision() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "Let's go with the builder pattern for config structs.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

#[test]
fn extracts_went_with_decision() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "We went with SQLite instead of PostgreSQL for this.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

#[test]
fn extracts_opted_for_decision() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "We opted for the async approach to keep things responsive.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

#[test]
fn extracts_settled_on_decision() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "After discussion, settled on using thiserror for the crate.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

#[test]
fn extracts_going_with_decision() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "Going with the builder pattern for configuration structs.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

#[test]
fn extracts_switched_to_decision() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "Switched to using DatabasePool after the connection leak.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
}

#[test]
fn extracts_blocked_on_task() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "We're blocked on the upstream API providing auth tokens.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.pending_tasks.len(), 1);
}

#[test]
fn extracts_will_need_to_task() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "We will need to update the schema after this migration.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.pending_tasks.len(), 1);
}

#[test]
fn extracts_checkbox_task() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "- [ ] Add unit tests for the new handler code.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.pending_tasks.len(), 1);
}

#[test]
fn extracts_doesnt_work_issue() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "The hybrid search doesn't work when embeddings are missing.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.issues.len(), 1);
}

#[test]
fn extracts_panicked_at_issue() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "Thread panicked at 'index out of bounds' in the parser.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.issues.len(), 1);
}

#[test]
fn extracts_regression_issue() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "This looks like a regression from the recent refactor work.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.issues.len(), 1);
}

#[test]
fn extracts_workaround_issue() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "workaround: manually flush the buffer before closing conn.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.issues.len(), 1);
}

// ── matches_any helper ──────────────────────────────────────────────

#[test]
fn matches_any_finds_substring() {
    assert!(matches_any("we decided to use it", DECISION_KEYWORDS));
}

#[test]
fn matches_any_returns_false_on_no_match() {
    assert!(!matches_any("this is a normal sentence", DECISION_KEYWORDS));
}

#[test]
fn matches_any_case_sensitive_on_lowered_input() {
    // matches_any expects pre-lowered input
    assert!(matches_any("opted for the new way", DECISION_KEYWORDS));
    assert!(!matches_any("OPTED FOR the new way", DECISION_KEYWORDS));
}

// ── Precision: should NOT match (false-positive guards) ─────────────

#[test]
fn no_false_positive_on_will_use_behavior() {
    // "will use" was tightened to "we will use" -- bare "will use" matches behavior descriptions
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "This function will use the cached value from the pool.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.decisions.is_empty());
}

#[test]
fn no_false_positive_on_choosing() {
    // "choosing" was removed -- matches non-decision prose
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "Choosing a variable name from the suggestions list.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.decisions.is_empty());
}

#[test]
fn no_false_positive_on_picked() {
    // "picked" was removed -- too ambiguous
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "I picked up the variable name from the existing code.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.decisions.is_empty());
}

#[test]
fn no_false_positive_on_moving_to() {
    // "moving to" was removed -- matches navigation prose
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "Moving to the next file in the directory listing.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.decisions.is_empty());
}

#[test]
fn no_false_positive_on_should_also() {
    // "should also" was removed -- matches any suggestion
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "You should also note that this function is pure.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.pending_tasks.is_empty());
}

#[test]
fn no_false_positive_on_after_that() {
    // "after that" was removed -- too conversational
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "After that the function returns the computed result.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.pending_tasks.is_empty());
}

#[test]
fn no_false_positive_on_unexpected() {
    // "unexpected" was removed -- matches discussion prose
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "This unexpected finding is actually quite interesting.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.issues.is_empty());
}

#[test]
fn no_false_positive_on_wrong() {
    // "wrong " was removed -- matches opinions, not errors
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "That's the wrong approach but it still compiles fine.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.issues.is_empty());
}

#[test]
fn no_false_positive_on_cannot() {
    // "cannot " was removed -- too conversational
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "We cannot use that pattern here, but there are alternatives.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.issues.is_empty());
}

#[test]
fn no_false_positive_on_warning_discussion() {
    // "warning:" was removed -- discussing warnings isn't the same as having issues
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "The warning: unused variable lint can be silenced with underscore."
            .to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.issues.is_empty());
}

#[test]
fn no_false_positive_on_regression_tests() {
    // "regression" alone was removed -- "a regression" requires the article
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "We should add regression tests for this module later.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.issues.is_empty());
}

#[test]
fn no_false_positive_on_fixme_without_colon() {
    // "fixme" alone was tightened to "fixme:" to avoid matching prose
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "The fixme comments in the codebase should be reviewed.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.issues.is_empty());
}

#[test]
fn no_false_positive_normal_prose() {
    // A completely normal paragraph should not match any category
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "The function takes a reference and returns an owned string from the input."
            .to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.decisions.is_empty());
    assert!(ctx.pending_tasks.is_empty());
    assert!(ctx.issues.is_empty());
}

// ── Constants ────────────────────────────────────────────────────────

#[test]
fn constants_have_expected_values() {
    assert_eq!(MAX_ITEMS_PER_CATEGORY, 5);
    assert_eq!(MIN_CONTENT_LEN, 10);
    assert_eq!(MAX_CONTENT_LEN, 800);
    assert!((COMPACTION_CONFIDENCE - 0.3).abs() < f64::EPSILON);
}

// ── Keyword list sanity ─────────────────────────────────────────────

#[test]
fn keyword_lists_are_non_empty() {
    assert!(!DECISION_KEYWORDS.is_empty());
    assert!(!TASK_KEYWORDS.is_empty());
    assert!(!ISSUE_KEYWORDS.is_empty());
}

#[test]
fn keyword_lists_are_lowercase() {
    for kw in DECISION_KEYWORDS
        .iter()
        .chain(TASK_KEYWORDS)
        .chain(ISSUE_KEYWORDS)
    {
        assert_eq!(*kw, kw.to_lowercase(), "Keyword '{}' must be lowercase", kw);
    }
}

// ── Reverse iteration (recency bias) ────────────────────────────────

#[test]
fn reverse_iteration_captures_most_recent() {
    // Create 10 decision messages; only the last 5 should be kept
    let mut messages = Vec::new();
    for i in 0..10 {
        messages.push(TranscriptMessage {
            role: "assistant".to_string(),
            text_content: format!("We decided to implement feature number {} for testing.", i),
        });
    }
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), MAX_ITEMS_PER_CATEGORY);
    // Should have the LAST 5 (indices 5-9), in chronological order
    assert!(ctx.decisions[0].contains("5"));
    assert!(ctx.decisions[4].contains("9"));
}

#[test]
fn reverse_iteration_restores_chronological_order() {
    let messages = vec![
        TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We decided to use pattern A for the first module.".to_string(),
        },
        TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "We decided to use pattern B for the second module.".to_string(),
        },
    ];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 2);
    // After reverse collection + re-reverse, chronological order is preserved
    assert!(ctx.decisions[0].contains("pattern A"));
    assert!(ctx.decisions[1].contains("pattern B"));
}

// ── user_intent extraction ──────────────────────────────────────────

#[test]
fn extracts_user_intent_from_first_user_message() {
    let messages = vec![
        TranscriptMessage {
            role: "user".to_string(),
            text_content: "Fix the authentication bug in the login handler.".to_string(),
        },
        TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "Looking into the auth handler now.".to_string(),
        },
    ];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(
        ctx.user_intent.as_deref(),
        Some("Fix the authentication bug in the login handler.")
    );
}

#[test]
fn user_intent_takes_first_paragraph_only() {
    let messages = vec![TranscriptMessage {
        role: "user".to_string(),
        text_content: "Refactor the database layer for clarity.\n\nAlso fix the tests.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(
        ctx.user_intent.as_deref(),
        Some("Refactor the database layer for clarity.")
    );
}

#[test]
fn user_intent_skips_too_short_content() {
    let messages = vec![TranscriptMessage {
        role: "user".to_string(),
        text_content: "ok".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.user_intent.is_none());
}

#[test]
fn user_intent_truncates_long_content() {
    let long_intent = "x".repeat(1000);
    let messages = vec![TranscriptMessage {
        role: "user".to_string(),
        text_content: long_intent,
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.user_intent.is_some());
    assert!(ctx.user_intent.as_ref().unwrap().len() <= MAX_CONTENT_LEN);
}

// ── files_referenced extraction ─────────────────────────────────────

#[test]
fn extracts_file_paths_from_assistant_messages() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "I updated src/hooks/precompact.rs and crates/mira-server/Cargo.toml."
            .to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(
        ctx.files_referenced
            .contains(&"src/hooks/precompact.rs".to_string())
    );
    assert!(
        ctx.files_referenced
            .contains(&"crates/mira-server/Cargo.toml".to_string())
    );
}

#[test]
fn file_paths_are_deduplicated() {
    let messages = vec![
        TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "I read src/main.rs first.".to_string(),
        },
        TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "Then I edited src/main.rs again.".to_string(),
        },
    ];
    let ctx = extract_compaction_context(&messages);
    let main_count = ctx
        .files_referenced
        .iter()
        .filter(|p| *p == "src/main.rs")
        .count();
    assert_eq!(main_count, 1);
}

#[test]
fn file_paths_capped_at_max() {
    // Generate more than MAX_FILE_REFS unique file paths
    let mut lines = Vec::new();
    for i in 0..20 {
        lines.push(format!("src/module_{}.rs", i));
    }
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: lines.join(" and "),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.files_referenced.len(), MAX_FILE_REFS);
}

#[test]
fn file_paths_skip_user_messages() {
    let messages = vec![TranscriptMessage {
        role: "user".to_string(),
        text_content: "Look at src/secret.rs please.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.files_referenced.is_empty());
}

#[test]
fn file_paths_skip_short_matches() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "The a.rs file is too short to be a real path reference.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    // "a.rs" is 4 chars, below MIN_FILE_PATH_LEN
    assert!(ctx.files_referenced.is_empty());
}

#[test]
fn file_paths_skip_url_fragments() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "See https://docs.rs/tokio/latest/tokio/index.html for the docs.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    // The regex matches "//docs.rs/tokio/latest/tokio/index.html" but the
    // "//" prefix filter should catch it.
    assert!(
        ctx.files_referenced.is_empty(),
        "URL fragment should be filtered: {:?}",
        ctx.files_referenced
    );
}

#[test]
fn file_paths_match_dotfiles() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "Check .github/workflows/ci.yml for the CI config.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(
        ctx.files_referenced.iter().any(|p| p.contains("ci.yml")),
        "Should match dotfile paths: {:?}",
        ctx.files_referenced
    );
}

#[test]
fn file_paths_match_relative_paths() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "The file at ./src/main.rs needs updating.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(!ctx.files_referenced.is_empty());
}

// ── is_continuation_prompt ──────────────────────────────────────────

#[test]
fn continuation_prompt_exact_match() {
    assert!(is_continuation_prompt("continue"));
    assert!(is_continuation_prompt("keep going"));
    assert!(is_continuation_prompt("yes"));
    assert!(is_continuation_prompt("ok"));
    assert!(is_continuation_prompt("sounds good"));
    assert!(is_continuation_prompt("lgtm"));
}

#[test]
fn continuation_prompt_with_punctuation() {
    assert!(is_continuation_prompt("continue."));
    assert!(is_continuation_prompt("yes!"));
    assert!(is_continuation_prompt("ok."));
    assert!(is_continuation_prompt("sure!"));
}

#[test]
fn continuation_prompt_case_insensitive() {
    assert!(is_continuation_prompt("Continue"));
    assert!(is_continuation_prompt("YES"));
    assert!(is_continuation_prompt("Keep Going"));
}

#[test]
fn continuation_prompt_rejects_real_requests() {
    assert!(!is_continuation_prompt("fix the auth bug"));
    assert!(!is_continuation_prompt("continue working on the migration"));
    assert!(!is_continuation_prompt("yes, also add tests for it"));
    assert!(!is_continuation_prompt(
        "ok now let's refactor the database layer"
    ));
}

#[test]
fn user_intent_skips_continuation_and_takes_next() {
    let messages = vec![
        TranscriptMessage {
            role: "user".to_string(),
            text_content: "continue".to_string(),
        },
        TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "Sure, continuing from where we left off.".to_string(),
        },
        TranscriptMessage {
            role: "user".to_string(),
            text_content: "Actually, let's fix the auth bug instead.".to_string(),
        },
    ];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(
        ctx.user_intent.as_deref(),
        Some("Actually, let's fix the auth bug instead.")
    );
}

// ── Keyword extraction assistant-only ────────────────────────────────

#[test]
fn keywords_only_match_assistant_messages() {
    let messages = vec![
        TranscriptMessage {
            role: "user".to_string(),
            text_content: "I decided to use the builder pattern for this.".to_string(),
        },
        TranscriptMessage {
            role: "assistant".to_string(),
            text_content: "I'll look into the implementation details.".to_string(),
        },
    ];
    let ctx = extract_compaction_context(&messages);
    // User saying "decided to" should NOT be captured as a decision
    assert!(ctx.decisions.is_empty());
}

// ── matches_issue_keyword (prefix matching) ─────────────────────────

#[test]
fn issue_keyword_in_prefix_matches() {
    assert!(matches_issue_keyword(
        "error: something went wrong in the handler"
    ));
}

#[test]
fn issue_keyword_beyond_prefix_does_not_match() {
    // Place the keyword well past the 80-char prefix
    let text = format!("{} error: this should not match", "x".repeat(100));
    assert!(!matches_issue_keyword(&text));
}

#[test]
fn issue_prefix_matching_does_not_affect_decision_matching() {
    // Decisions still use full-text matching via matches_any
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: format!("{} we decided to change the approach.", "x".repeat(100)),
    }];
    let ctx = extract_compaction_context(&messages);
    // Decision keyword is past 80 chars but matches_any searches the whole text
    assert_eq!(ctx.decisions.len(), 1);
}

// ── Truncate instead of drop ────────────────────────────────────────

#[test]
fn long_decision_paragraph_is_truncated_and_kept() {
    let long_text = format!("We decided to {}", "refactor ".repeat(200));
    assert!(long_text.len() > MAX_CONTENT_LEN);
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: long_text,
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.decisions.len(), 1);
    assert!(ctx.decisions[0].len() <= MAX_CONTENT_LEN);
}

// ── Active work improvements ────────────────────────────────────────

#[test]
fn active_work_takes_two_paragraphs() {
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content:
            "Working on the database migration system now.\n\nThe schema changes affect three tables in the database."
                .to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.active_work.len(), 2);
}

#[test]
fn active_work_skips_short_paragraphs() {
    // First paragraph is > 30 chars, second is <= 30 chars
    let messages = vec![TranscriptMessage {
        role: "assistant".to_string(),
        text_content: "Working on the database migration system now.\n\nShort note.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert_eq!(ctx.active_work.len(), 1);
    assert!(ctx.active_work[0].contains("database migration"));
}

#[test]
fn active_work_skips_user_messages() {
    // Only user messages, no assistant -- no active work
    let messages = vec![TranscriptMessage {
        role: "user".to_string(),
        text_content: "This is a user message that should be skipped for active work.".to_string(),
    }];
    let ctx = extract_compaction_context(&messages);
    assert!(ctx.active_work.is_empty());
}

// ── is_empty with new fields ────────────────────────────────────────

#[test]
fn not_empty_with_user_intent() {
    let ctx = CompactionContext {
        user_intent: Some("Fix the bug".into()),
        ..Default::default()
    };
    assert!(!ctx.is_empty());
    assert_eq!(ctx.total_items(), 1);
}

#[test]
fn not_empty_with_files_referenced() {
    let ctx = CompactionContext {
        files_referenced: vec!["src/main.rs".into()],
        ..Default::default()
    };
    assert!(!ctx.is_empty());
    assert_eq!(ctx.total_items(), 1);
}

// ── Backward-compatible deserialization ──────────────────────────────

#[test]
fn deserializes_without_new_fields() {
    // Old format without user_intent and files_referenced
    let json = serde_json::json!({
        "decisions": ["chose builder"],
        "active_work": [],
        "issues": [],
        "pending_tasks": []
    });
    let ctx: CompactionContext = serde_json::from_value(json).unwrap();
    assert_eq!(ctx.decisions.len(), 1);
    assert!(ctx.user_intent.is_none());
    assert!(ctx.files_referenced.is_empty());
}

// ── Constants ───────────────────────────────────────────────────────

#[test]
fn new_constants_have_expected_values() {
    assert_eq!(MAX_TRANSCRIPT_BYTES, 50 * 1024 * 1024);
    assert_eq!(MAX_FILE_REFS, 10);
    assert_eq!(MIN_FILE_PATH_LEN, 5);
}
