// crates/mira-server/src/db/cross_project_tests.rs
// Tests for cross-project knowledge surfacing

use super::cross_project::*;
use super::test_support::*;
use super::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Cross-project preferences (pure SQL, no embeddings needed)
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_cross_project_preferences_no_overlap() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("ProjectA")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("ProjectB")).map_err(Into::into)
    })
    .0;

    // Store different preferences in each project
    db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid1),
            Some("pref-a"),
            "Use tabs for indentation",
            "preference",
            Some("preference"),
            0.8,
        )
    });
    db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid2),
            Some("pref-b"),
            "Use spaces for indentation",
            "preference",
            Some("preference"),
            0.8,
        )
    });

    // No overlapping preferences
    let prefs = db!(pool, |conn| {
        get_cross_project_preferences_sync(conn, pid1, 5).map_err(Into::into)
    });
    assert!(prefs.is_empty());
}

#[tokio::test]
async fn test_cross_project_preferences_with_overlap() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("ProjectA")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("ProjectB")).map_err(Into::into)
    })
    .0;

    // Store same preference in both projects
    db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid1),
            Some("pref-builder-a"),
            "Use builder pattern for config structs",
            "preference",
            Some("preference"),
            0.9,
        )
    });
    db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid2),
            Some("pref-builder-b"),
            "Use builder pattern for config structs",
            "preference",
            Some("preference"),
            0.85,
        )
    });

    // Should find the overlapping preference from either project's perspective
    let prefs = db!(pool, |conn| {
        get_cross_project_preferences_sync(conn, pid1, 5).map_err(Into::into)
    });
    assert_eq!(prefs.len(), 1);
    assert!(prefs[0].content.contains("builder pattern"));
    assert_eq!(prefs[0].project_count, 2);
    assert!(prefs[0].projects.contains("ProjectA"));
    assert!(prefs[0].projects.contains("ProjectB"));

    // Same from project B's perspective
    let prefs_b = db!(pool, |conn| {
        get_cross_project_preferences_sync(conn, pid2, 5).map_err(Into::into)
    });
    assert_eq!(prefs_b.len(), 1);
}

#[tokio::test]
async fn test_cross_project_preferences_excludes_archived() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("ProjectA")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("ProjectB")).map_err(Into::into)
    })
    .0;

    // Store same preference in both, then archive one
    let _id1 = db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid1),
            Some("pref-arch-a"),
            "Use async everywhere",
            "preference",
            Some("preference"),
            0.8,
        )
    });
    let id2 = db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid2),
            Some("pref-arch-b"),
            "Use async everywhere",
            "preference",
            Some("preference"),
            0.8,
        )
    });

    // Archive the second project's memory
    db!(pool, |conn| {
        conn.execute(
            "UPDATE memory_facts SET status = 'archived' WHERE id = ?",
            [id2],
        )
        .map_err(Into::into)
    });

    // Archived memory should not count toward cross-project overlap
    let prefs = db!(pool, |conn| {
        get_cross_project_preferences_sync(conn, pid1, 5).map_err(Into::into)
    });
    assert!(prefs.is_empty());
}

#[tokio::test]
async fn test_cross_project_preferences_excludes_suspicious_in_current_project() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("ProjectA")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("ProjectB")).map_err(Into::into)
    })
    .0;

    // Store same preference in both projects
    let id1 = db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid1),
            Some("pref-sus-a"),
            "Use suspicious pattern",
            "preference",
            Some("preference"),
            0.8,
        )
    });
    db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid2),
            Some("pref-sus-b"),
            "Use suspicious pattern",
            "preference",
            Some("preference"),
            0.8,
        )
    });

    // Mark the current-project memory as suspicious
    db!(pool, |conn| {
        conn.execute(
            "UPDATE memory_facts SET suspicious = 1 WHERE id = ?",
            [id1],
        )
        .map_err(Into::into)
    });

    // Suspicious current-project memory should not qualify the preference
    let prefs = db!(pool, |conn| {
        get_cross_project_preferences_sync(conn, pid1, 5).map_err(Into::into)
    });
    assert!(
        prefs.is_empty(),
        "Suspicious current-project memory should not qualify preference"
    );
}

#[tokio::test]
async fn test_cross_project_preferences_excludes_archived_in_current_project() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("ProjectA")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("ProjectB")).map_err(Into::into)
    })
    .0;

    // Store same preference in both projects
    let id1 = db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid1),
            Some("pref-arch-cur-a"),
            "Use archived pattern",
            "preference",
            Some("preference"),
            0.8,
        )
    });
    db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid2),
            Some("pref-arch-cur-b"),
            "Use archived pattern",
            "preference",
            Some("preference"),
            0.8,
        )
    });

    // Archive the CURRENT project's memory (not the other project's)
    db!(pool, |conn| {
        conn.execute(
            "UPDATE memory_facts SET status = 'archived' WHERE id = ?",
            [id1],
        )
        .map_err(Into::into)
    });

    // Archived current-project memory should not qualify the preference
    let prefs = db!(pool, |conn| {
        get_cross_project_preferences_sync(conn, pid1, 5).map_err(Into::into)
    });
    assert!(
        prefs.is_empty(),
        "Archived current-project memory should not qualify preference"
    );
}

#[tokio::test]
async fn test_cross_project_preferences_three_projects() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("Alpha")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("Beta")).map_err(Into::into)
    })
    .0;
    let pid3 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-c", Some("Gamma")).map_err(Into::into)
    })
    .0;

    // Same preference in all three projects
    for (pid, key) in [(pid1, "pref-1"), (pid2, "pref-2"), (pid3, "pref-3")] {
        db!(pool, |conn| {
            store_memory_helper(
                conn,
                Some(pid),
                Some(key),
                "Never use unwrap in production code",
                "preference",
                Some("preference"),
                0.9,
            )
        });
    }

    let prefs = db!(pool, |conn| {
        get_cross_project_preferences_sync(conn, pid1, 5).map_err(Into::into)
    });
    assert_eq!(prefs.len(), 1);
    assert_eq!(prefs[0].project_count, 3);
}

#[tokio::test]
async fn test_cross_project_preferences_respects_limit() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("Alpha")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("Beta")).map_err(Into::into)
    })
    .0;

    // Create 5 overlapping preferences
    for i in 0..5 {
        let content = format!("Preference number {}", i);
        let key1 = format!("pref-a-{}", i);
        let key2 = format!("pref-b-{}", i);
        let content_clone = content.clone();
        db!(pool, |conn| {
            store_memory_helper(
                conn,
                Some(pid1),
                Some(&key1),
                &content,
                "preference",
                Some("preference"),
                0.8,
            )
        });
        db!(pool, |conn| {
            store_memory_helper(
                conn,
                Some(pid2),
                Some(&key2),
                &content_clone,
                "preference",
                Some("preference"),
                0.8,
            )
        });
    }

    // Limit to 2
    let prefs = db!(pool, |conn| {
        get_cross_project_preferences_sync(conn, pid1, 2).map_err(Into::into)
    });
    assert_eq!(prefs.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Semantic cross-project recall (needs vec_memory embeddings)
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_recall_cross_project_no_other_projects() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("ProjectA")).map_err(Into::into)
    })
    .0;

    // Store a memory with embedding in project A
    let fact_id = db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid1),
            Some("mem-a"),
            "Use connection pooling",
            "decision",
            Some("patterns"),
            0.9,
        )
    });

    // Store a fake embedding
    let fake_embedding: Vec<f32> = vec![0.1; 1536];
    let embedding_bytes = crate::search::embedding_to_bytes(&fake_embedding);
    let embedding_bytes_clone = embedding_bytes.clone();
    db!(pool, |conn| {
        store_fact_embedding_sync(conn, fact_id, "embedded content", &embedding_bytes_clone).map_err(Into::into)
    });

    // Query from the same project — should find nothing (cross-project excludes current)
    let results = db!(pool, |conn| {
        recall_cross_project_sync(conn, &embedding_bytes, pid1, 5).map_err(Into::into)
    });
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_recall_cross_project_finds_other_project() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("ProjectA")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("ProjectB")).map_err(Into::into)
    })
    .0;

    // Store a memory with embedding in project B
    let fact_id = db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid2),
            Some("mem-b"),
            "Solved auth with JWT middleware",
            "decision",
            Some("patterns"),
            0.9,
        )
    });

    let fake_embedding: Vec<f32> = vec![0.1; 1536];
    let embedding_bytes = crate::search::embedding_to_bytes(&fake_embedding);
    let embedding_bytes_clone = embedding_bytes.clone();
    db!(pool, |conn| {
        store_fact_embedding_sync(conn, fact_id, "embedded content", &embedding_bytes_clone).map_err(Into::into)
    });

    // Query from project A — should find project B's memory
    let results = db!(pool, |conn| {
        recall_cross_project_sync(conn, &embedding_bytes, pid1, 5).map_err(Into::into)
    });
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].project_name, "ProjectB");
    assert!(results[0].content.contains("JWT middleware"));
    assert_eq!(results[0].project_id, pid2);
}

#[tokio::test]
async fn test_recall_cross_project_excludes_archived() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("ProjectA")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("ProjectB")).map_err(Into::into)
    })
    .0;

    // Store an archived memory in project B
    let fact_id = db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid2),
            Some("mem-archived"),
            "Old pattern no longer used",
            "decision",
            Some("patterns"),
            0.9,
        )
    });

    let fake_embedding: Vec<f32> = vec![0.1; 1536];
    let embedding_bytes = crate::search::embedding_to_bytes(&fake_embedding);
    let embedding_bytes_clone = embedding_bytes.clone();
    db!(pool, |conn| {
        store_fact_embedding_sync(conn, fact_id, "embedded content", &embedding_bytes_clone).map_err(Into::into)
    });

    // Archive it
    db!(pool, |conn| {
        conn.execute(
            "UPDATE memory_facts SET status = 'archived' WHERE id = ?",
            [fact_id],
        )
        .map_err(Into::into)
    });

    // Should not find archived memories
    let results = db!(pool, |conn| {
        recall_cross_project_sync(conn, &embedding_bytes, pid1, 5).map_err(Into::into)
    });
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_find_solved_in_other_project() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("ProjectA")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("ProjectB")).map_err(Into::into)
    })
    .0;

    // Store a high-confidence decision in project B
    let fact_id = db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid2),
            Some("solved-b"),
            "Use connection pooling with max 8 connections for SQLite",
            "decision",
            Some("patterns"),
            0.9,
        )
    });

    // Use the exact same embedding for a perfect match (distance ≈ 0)
    let fake_embedding: Vec<f32> = vec![0.5; 1536];
    let embedding_bytes = crate::search::embedding_to_bytes(&fake_embedding);
    let embedding_bytes_clone = embedding_bytes.clone();
    db!(pool, |conn| {
        store_fact_embedding_sync(conn, fact_id, "embedded content", &embedding_bytes_clone).map_err(Into::into)
    });

    // Exact same embedding → distance ≈ 0, well within 0.25 threshold
    let results = db!(pool, |conn| {
        find_solved_in_other_project_sync(conn, &embedding_bytes, pid1, 0.25, 5)
            .map_err(Into::into)
    });
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].project_name, "ProjectB");
    assert_eq!(results[0].fact_type, "decision");
}

#[tokio::test]
async fn test_find_solved_excludes_low_confidence() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("ProjectA")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("ProjectB")).map_err(Into::into)
    })
    .0;

    // Store a LOW confidence decision in project B
    let fact_id = db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid2),
            Some("low-conf"),
            "Maybe use Redis for caching",
            "decision",
            Some("patterns"),
            0.3, // Below 0.7 threshold
        )
    });

    let fake_embedding: Vec<f32> = vec![0.5; 1536];
    let embedding_bytes = crate::search::embedding_to_bytes(&fake_embedding);
    let embedding_bytes_clone = embedding_bytes.clone();
    db!(pool, |conn| {
        store_fact_embedding_sync(conn, fact_id, "embedded content", &embedding_bytes_clone).map_err(Into::into)
    });

    // Should not find low-confidence memories even with perfect embedding match
    let results = db!(pool, |conn| {
        find_solved_in_other_project_sync(conn, &embedding_bytes, pid1, 0.25, 5)
            .map_err(Into::into)
    });
    assert!(results.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════
// Session recap integration
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_session_recap_includes_cross_project_preferences() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("Alpha")).map_err(Into::into)
    })
    .0;
    let pid2 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-b", Some("Beta")).map_err(Into::into)
    })
    .0;

    // Same preference in both projects
    db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid1),
            Some("recap-pref-a"),
            "Always use debug builds during development",
            "preference",
            Some("preference"),
            0.9,
        )
    });
    db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid2),
            Some("recap-pref-b"),
            "Always use debug builds during development",
            "preference",
            Some("preference"),
            0.9,
        )
    });

    // Build recap for project A
    let recap = db!(pool, |conn| {
        Ok::<_, anyhow::Error>(build_session_recap_sync(conn, Some(pid1)))
    });

    assert!(
        recap.contains("Cross-project patterns:"),
        "Recap should include cross-project section. Got: {}",
        recap
    );
    assert!(
        recap.contains("debug builds"),
        "Recap should include the preference content"
    );
}

#[tokio::test]
async fn test_session_recap_no_cross_project_when_single_project() {
    let pool = setup_test_pool().await;
    let pid1 = db!(pool, |conn| {
        get_or_create_project_sync(conn, "/project-a", Some("Alpha")).map_err(Into::into)
    })
    .0;

    // Only one project with preferences
    db!(pool, |conn| {
        store_memory_helper(
            conn,
            Some(pid1),
            Some("solo-pref"),
            "Use debug builds",
            "preference",
            Some("preference"),
            0.9,
        )
    });

    let recap = db!(pool, |conn| {
        Ok::<_, anyhow::Error>(build_session_recap_sync(conn, Some(pid1)))
    });

    assert!(
        !recap.contains("Cross-project patterns:"),
        "Recap should NOT include cross-project section with only one project"
    );
}
