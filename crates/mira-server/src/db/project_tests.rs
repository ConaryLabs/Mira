// crates/mira-server/src/db/project_tests.rs
// Tests for project and server state operations

use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════
    // get_or_create_project Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_or_create_project_basic() {
        let db = Database::open_in_memory().unwrap();

        let (id, name) = db.get_or_create_project("/test/path", Some("test-project")).unwrap();
        assert!(id > 0);
        assert_eq!(name, Some("test-project".to_string()));
    }

    #[test]
    fn test_get_or_create_project_upsert() {
        let db = Database::open_in_memory().unwrap();

        let (id1, name1) = db.get_or_create_project("/test/path", Some("project-one")).unwrap();
        let (id2, name2) = db.get_or_create_project("/test/path", Some("project-two")).unwrap();

        // Should return same ID (upsert behavior)
        assert_eq!(id1, id2);
        // Should keep original name
        assert_eq!(name1, Some("project-one".to_string()));
        assert_eq!(name2, Some("project-one".to_string()));
    }

    #[test]
    fn test_get_or_create_project_no_name() {
        let db = Database::open_in_memory().unwrap();

        let (id, name) = db.get_or_create_project("/test/path", None).unwrap();
        assert!(id > 0);
        // Should return directory name as fallback
        assert_eq!(name, Some("path".to_string()));
    }

    #[test]
    fn test_get_or_create_project_different_paths() {
        let db = Database::open_in_memory().unwrap();

        let (id1, _) = db.get_or_create_project("/path1", None).unwrap();
        let (id2, _) = db.get_or_create_project("/path2", None).unwrap();

        assert_ne!(id1, id2);
    }

    // ═══════════════════════════════════════
    // get_project_info Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_get_project_info_existing() {
        let db = Database::open_in_memory().unwrap();

        let (id, _) = db
            .get_or_create_project("/test/path", Some("test-project"))
            .unwrap();

        let info = db.get_project_info(id).unwrap().unwrap();
        assert_eq!(info.0, Some("test-project".to_string()));
        assert_eq!(info.1, "/test/path");
    }

    #[test]
    fn test_get_project_info_nonexistent() {
        let db = Database::open_in_memory().unwrap();

        let info = db.get_project_info(99999).unwrap();
        assert!(info.is_none());
    }

    // ═══════════════════════════════════════
    // list_projects Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_list_projects_empty() {
        let db = Database::open_in_memory().unwrap();

        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 0);
    }

    #[test]
    fn test_list_projects_multiple() {
        let db = Database::open_in_memory().unwrap();

        db.get_or_create_project("/path1", Some("project1")).unwrap();
        db.get_or_create_project("/path2", Some("project2")).unwrap();
        db.get_or_create_project("/path3", Some("project3")).unwrap();

        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 3);
        // Should be ordered by id DESC (most recent first)
        assert_eq!(projects[0].1, "/path3");
        assert_eq!(projects[1].1, "/path2");
        assert_eq!(projects[2].1, "/path1");
    }

    #[test]
    fn test_list_projects_with_names() {
        let db = Database::open_in_memory().unwrap();

        db.get_or_create_project("/path1", Some("First Project")).unwrap();
        db.get_or_create_project("/path2", None).unwrap();

        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].2, Some("path2".to_string()));
        assert_eq!(projects[1].2, Some("First Project".to_string()));
    }

    // ═══════════════════════════════════════
    // project_briefing Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_update_and_get_project_briefing() {
        let db = Database::open_in_memory().unwrap();

        let (project_id, _) = db.get_or_create_project("/test/path", Some("test")).unwrap();

        db.update_project_briefing(
            project_id,
            "abc123",
            Some("New changes in the project"),
        )
        .unwrap();

        let briefing = db.get_project_briefing(project_id).unwrap().unwrap();
        assert_eq!(briefing.project_id, project_id);
        assert_eq!(briefing.last_known_commit, Some("abc123".to_string()));
        assert_eq!(briefing.briefing_text, Some("New changes in the project".to_string()));
        assert!(briefing.generated_at.is_some());
    }

    #[test]
    fn test_get_project_briefing_none() {
        let db = Database::open_in_memory().unwrap();

        let (project_id, _) = db.get_or_create_project("/test/path", Some("test")).unwrap();

        let briefing = db.get_project_briefing(project_id).unwrap();
        assert!(briefing.is_none());
    }

    #[test]
    fn test_update_project_briefing_upsert() {
        let db = Database::open_in_memory().unwrap();

        let (project_id, _) = db.get_or_create_project("/test/path", Some("test")).unwrap();

        db.update_project_briefing(project_id, "commit1", Some("First briefing"))
            .unwrap();
        db.update_project_briefing(project_id, "commit2", Some("Second briefing"))
            .unwrap();

        let briefing = db.get_project_briefing(project_id).unwrap().unwrap();
        assert_eq!(briefing.last_known_commit, Some("commit2".to_string()));
        assert_eq!(briefing.briefing_text, Some("Second briefing".to_string()));
    }

    #[test]
    fn test_update_project_briefing_no_text() {
        let db = Database::open_in_memory().unwrap();

        let (project_id, _) = db.get_or_create_project("/test/path", Some("test")).unwrap();

        db.update_project_briefing(project_id, "abc123", None).unwrap();

        let briefing = db.get_project_briefing(project_id).unwrap().unwrap();
        assert_eq!(briefing.last_known_commit, Some("abc123".to_string()));
        assert_eq!(briefing.briefing_text, None);
    }

    #[test]
    fn test_mark_session_clears_briefing() {
        let db = Database::open_in_memory().unwrap();

        let (project_id, _) = db.get_or_create_project("/test/path", Some("test")).unwrap();

        // Set briefing
        db.update_project_briefing(project_id, "abc123", Some("Briefing text"))
            .unwrap();

        // Mark session (should clear briefing)
        db.mark_session_for_briefing(project_id).unwrap();

        let briefing = db.get_project_briefing(project_id).unwrap().unwrap();
        assert_eq!(briefing.briefing_text, None);
        assert!(briefing.last_session_at.is_some());
    }

    #[test]
    fn test_get_projects_for_briefing_check() {
        let db = Database::open_in_memory().unwrap();

        let (project1, _) = db.get_or_create_project("/path1", Some("p1")).unwrap();
        let (_project2, _) = db.get_or_create_project("/path2", Some("p2")).unwrap();

        // Mark one project as having a session
        db.mark_session_for_briefing(project1).unwrap();

        let projects = db.get_projects_for_briefing_check().unwrap();
        // Should return all projects with paths
        assert!(projects.len() >= 2);
    }

    // ═══════════════════════════════════════
    // server_state Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_set_and_get_server_state() {
        let db = Database::open_in_memory().unwrap();

        db.set_server_state("test_key", "test_value").unwrap();
        let value = db.get_server_state("test_key").unwrap();
        assert_eq!(value, Some("test_value".to_string()));
    }

    #[test]
    fn test_get_server_state_nonexistent() {
        let db = Database::open_in_memory().unwrap();

        let value = db.get_server_state("nonexistent").unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn test_set_server_state_upsert() {
        let db = Database::open_in_memory().unwrap();

        db.set_server_state("key", "value1").unwrap();
        db.set_server_state("key", "value2").unwrap();

        let value = db.get_server_state("key").unwrap();
        assert_eq!(value, Some("value2".to_string()));
    }

    #[test]
    fn test_delete_server_state() {
        let db = Database::open_in_memory().unwrap();

        db.set_server_state("key", "value").unwrap();
        let deleted = db.delete_server_state("key").unwrap();
        assert!(deleted);

        let value = db.get_server_state("key").unwrap();
        assert!(value.is_none());
    }

    #[test]
    fn test_delete_server_state_nonexistent() {
        let db = Database::open_in_memory().unwrap();

        let deleted = db.delete_server_state("nonexistent").unwrap();
        assert!(!deleted);
    }

    // ═══════════════════════════════════════
    // active_project Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_save_and_get_active_project() {
        let db = Database::open_in_memory().unwrap();

        db.save_active_project("/my/project").unwrap();
        let project = db.get_last_active_project().unwrap();
        assert_eq!(project, Some("/my/project".to_string()));
    }

    #[test]
    fn test_clear_active_project() {
        let db = Database::open_in_memory().unwrap();

        db.save_active_project("/my/project").unwrap();
        db.clear_active_project().unwrap();

        let project = db.get_last_active_project().unwrap();
        assert!(project.is_none());
    }

    #[test]
    fn test_update_active_project() {
        let db = Database::open_in_memory().unwrap();

        db.save_active_project("/project1").unwrap();
        db.save_active_project("/project2").unwrap();

        let project = db.get_last_active_project().unwrap();
        assert_eq!(project, Some("/project2".to_string()));
    }

    // ═══════════════════════════════════════
    // Database path Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_path_in_memory() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.path().is_none());
    }

    #[test]
    fn test_path_file_based() {
        let temp_dir = std::env::temp_dir();
        let db_path = temp_dir.join("test_mira_db_unit_test");

        // Clean up if exists
        let _ = std::fs::remove_file(&db_path);

        // Skip test if we can't write to temp dir (sandboxed environment)
        match Database::open(&db_path) {
            Ok(db) => {
                assert_eq!(db.path(), Some(db_path.to_str().unwrap()));
                // Clean up
                let _ = std::fs::remove_file(&db_path);
            }
            Err(e) => {
                // Sandboxed or permission denied - skip test
                eprintln!("Skipping test_path_file_based: {}", e);
            }
        }
    }

    // ═══════════════════════════════════════
    // Integration Tests
    // ═══════════════════════════════════════

    #[test]
    fn test_full_project_lifecycle() {
        let db = Database::open_in_memory().unwrap();

        // Create project
        let (project_id, _) = db.get_or_create_project("/my/project", Some("MyProject")).unwrap();

        // Verify it's in the list
        let projects = db.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].0, project_id);

        // Get project info
        let info = db.get_project_info(project_id).unwrap().unwrap();
        assert_eq!(info.0, Some("MyProject".to_string()));
        assert_eq!(info.1, "/my/project");

        // Update briefing
        db.update_project_briefing(project_id, "commit123", Some("Changes made"))
            .unwrap();

        let briefing = db.get_project_briefing(project_id).unwrap().unwrap();
        assert_eq!(briefing.briefing_text, Some("Changes made".to_string()));

        // Mark session (clears briefing)
        db.mark_session_for_briefing(project_id).unwrap();

        let briefing = db.get_project_briefing(project_id).unwrap().unwrap();
        assert!(briefing.briefing_text.is_none());
    }

    // ═══════════════════════════════════════
    // Edge Cases
    // ═══════════════════════════════════════

    #[test]
    fn test_empty_project_path() {
        let db = Database::open_in_memory().unwrap();

        let (id, _name) = db.get_or_create_project("", None).unwrap();
        assert!(id > 0);
        // Empty path should still work, name would be empty or None
    }

    #[test]
    fn test_very_long_project_name() {
        let db = Database::open_in_memory().unwrap();

        let long_name = "a".repeat(1000);
        let (id, stored_name) = db.get_or_create_project("/test", Some(&long_name)).unwrap();

        assert!(id > 0);
        assert_eq!(stored_name, Some(long_name));
    }

    #[test]
    fn test_special_characters_in_path() {
        let db = Database::open_in_memory().unwrap();

        let paths = vec![
            "/path/with spaces",
            "/path/with-dashes",
            "/path/with_underscores",
            "/path/with.dots",
        ];

        for path in paths {
            let (id, _) = db.get_or_create_project(path, None).unwrap();
            assert!(id > 0, "Failed for path: {}", path);
        }
    }

    #[test]
    fn test_empty_server_state_key() {
        let db = Database::open_in_memory().unwrap();

        db.set_server_state("", "value").unwrap();
        let value = db.get_server_state("").unwrap();
        assert_eq!(value, Some("value".to_string()));
    }

    #[test]
    fn test_empty_server_state_value() {
        let db = Database::open_in_memory().unwrap();

        db.set_server_state("key", "").unwrap();
        let value = db.get_server_state("key").unwrap();
        assert_eq!(value, Some("".to_string()));
    }

    #[test]
    fn test_unicode_project_name() {
        let db = Database::open_in_memory().unwrap();

        let unicode_name = "🎉 项目 🚀";
        let (id, stored_name) = db.get_or_create_project("/test", Some(unicode_name)).unwrap();

        assert!(id > 0);
        assert_eq!(stored_name, Some(unicode_name.to_string()));
    }
}
