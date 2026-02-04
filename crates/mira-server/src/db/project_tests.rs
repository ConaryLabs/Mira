// crates/mira-server/src/db/project_tests.rs
// Tests for project and server state operations

use super::test_support::setup_test_pool;
use super::{
    clear_active_project_sync, delete_server_state_sync, get_last_active_project_sync,
    get_or_create_project_sync, get_project_briefing_sync, get_project_info_sync,
    get_projects_for_briefing_check_sync, get_server_state_sync, list_projects_sync,
    mark_session_for_briefing_sync, save_active_project_sync, set_server_state_sync,
    update_project_briefing_sync,
};

#[cfg(test)]
mod tests {
    use super::*;

    // ═══════════════════════════════════════
    // get_or_create_project Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_or_create_project_basic() {
        let pool = setup_test_pool().await;
        let (id, name) = db!(pool, |conn| get_or_create_project_sync(conn, "/test/path", Some("test-project")).map_err(Into::into));
        assert!(id > 0);
        assert_eq!(name, Some("test-project".to_string()));
    }

    #[tokio::test]
    async fn test_get_or_create_project_upsert() {
        let pool = setup_test_pool().await;
        let (id1, name1) = db!(pool, |conn| get_or_create_project_sync(conn, "/test/path", Some("project-one")).map_err(Into::into));
        let (id2, name2) = db!(pool, |conn| get_or_create_project_sync(conn, "/test/path", Some("project-two")).map_err(Into::into));
        assert_eq!(id1, id2);
        assert_eq!(name1, Some("project-one".to_string()));
        assert_eq!(name2, Some("project-one".to_string()));
    }

    #[tokio::test]
    async fn test_get_or_create_project_no_name() {
        let pool = setup_test_pool().await;
        let (id, name) = db!(pool, |conn| get_or_create_project_sync(conn, "/test/path", None).map_err(Into::into));
        assert!(id > 0);
        assert_eq!(name, None);
    }

    #[tokio::test]
    async fn test_get_or_create_project_different_paths() {
        let pool = setup_test_pool().await;
        let (id1, _) = db!(pool, |conn| get_or_create_project_sync(conn, "/path1", None).map_err(Into::into));
        let (id2, _) = db!(pool, |conn| get_or_create_project_sync(conn, "/path2", None).map_err(Into::into));
        assert_ne!(id1, id2);
    }

    // ═══════════════════════════════════════
    // get_project_info Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_get_project_info_existing() {
        let pool = setup_test_pool().await;
        let (id, _) = db!(pool, |conn| get_or_create_project_sync(conn, "/test/path", Some("test-project")).map_err(Into::into));
        let info = db!(pool, |conn| get_project_info_sync(conn, id).map_err(Into::into)).unwrap();
        assert_eq!(info.0, Some("test-project".to_string()));
        assert_eq!(info.1, "/test/path");
    }

    #[tokio::test]
    async fn test_get_project_info_nonexistent() {
        let pool = setup_test_pool().await;
        let info = db!(pool, |conn| get_project_info_sync(conn, 99999).map_err(Into::into));
        assert!(info.is_none());
    }

    // ═══════════════════════════════════════
    // list_projects Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_list_projects_empty() {
        let pool = setup_test_pool().await;
        let projects = db!(pool, |conn| list_projects_sync(conn).map_err(Into::into));
        assert_eq!(projects.len(), 0);
    }

    #[tokio::test]
    async fn test_list_projects_multiple() {
        let pool = setup_test_pool().await;
        db!(pool, |conn| get_or_create_project_sync(conn, "/path1", Some("project1")).map_err(Into::into));
        db!(pool, |conn| get_or_create_project_sync(conn, "/path2", Some("project2")).map_err(Into::into));
        db!(pool, |conn| get_or_create_project_sync(conn, "/path3", Some("project3")).map_err(Into::into));

        let projects = db!(pool, |conn| list_projects_sync(conn).map_err(Into::into));
        assert_eq!(projects.len(), 3);
        assert_eq!(projects[0].1, "/path3");
        assert_eq!(projects[1].1, "/path2");
        assert_eq!(projects[2].1, "/path1");
    }

    #[tokio::test]
    async fn test_list_projects_with_names() {
        let pool = setup_test_pool().await;
        db!(pool, |conn| get_or_create_project_sync(conn, "/path1", Some("First Project")).map_err(Into::into));
        db!(pool, |conn| get_or_create_project_sync(conn, "/path2", None).map_err(Into::into));

        let projects = db!(pool, |conn| list_projects_sync(conn).map_err(Into::into));
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].2, None);
        assert_eq!(projects[1].2, Some("First Project".to_string()));
    }

    // ═══════════════════════════════════════
    // project_briefing Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_update_and_get_project_briefing() {
        let pool = setup_test_pool().await;
        let (project_id, _) = db!(pool, |conn| get_or_create_project_sync(conn, "/test/path", Some("test")).map_err(Into::into));
        db!(pool, |conn| update_project_briefing_sync(conn, project_id, "abc123", Some("New changes in the project")).map_err(Into::into));

        let briefing = db!(pool, |conn| get_project_briefing_sync(conn, project_id).map_err(Into::into)).unwrap();
        assert_eq!(briefing.project_id, project_id);
        assert_eq!(briefing.last_known_commit, Some("abc123".to_string()));
        assert_eq!(briefing.briefing_text, Some("New changes in the project".to_string()));
        assert!(briefing.generated_at.is_some());
    }

    #[tokio::test]
    async fn test_get_project_briefing_none() {
        let pool = setup_test_pool().await;
        let (project_id, _) = db!(pool, |conn| get_or_create_project_sync(conn, "/test/path", Some("test")).map_err(Into::into));
        let briefing = db!(pool, |conn| get_project_briefing_sync(conn, project_id).map_err(Into::into));
        assert!(briefing.is_none());
    }

    #[tokio::test]
    async fn test_update_project_briefing_upsert() {
        let pool = setup_test_pool().await;
        let (project_id, _) = db!(pool, |conn| get_or_create_project_sync(conn, "/test/path", Some("test")).map_err(Into::into));
        db!(pool, |conn| update_project_briefing_sync(conn, project_id, "commit1", Some("First briefing")).map_err(Into::into));
        db!(pool, |conn| update_project_briefing_sync(conn, project_id, "commit2", Some("Second briefing")).map_err(Into::into));

        let briefing = db!(pool, |conn| get_project_briefing_sync(conn, project_id).map_err(Into::into)).unwrap();
        assert_eq!(briefing.last_known_commit, Some("commit2".to_string()));
        assert_eq!(briefing.briefing_text, Some("Second briefing".to_string()));
    }

    #[tokio::test]
    async fn test_update_project_briefing_no_text() {
        let pool = setup_test_pool().await;
        let (project_id, _) = db!(pool, |conn| get_or_create_project_sync(conn, "/test/path", Some("test")).map_err(Into::into));
        db!(pool, |conn| update_project_briefing_sync(conn, project_id, "abc123", None).map_err(Into::into));

        let briefing = db!(pool, |conn| get_project_briefing_sync(conn, project_id).map_err(Into::into)).unwrap();
        assert_eq!(briefing.last_known_commit, Some("abc123".to_string()));
        assert_eq!(briefing.briefing_text, None);
    }

    #[tokio::test]
    async fn test_mark_session_clears_briefing() {
        let pool = setup_test_pool().await;
        let (project_id, _) = db!(pool, |conn| get_or_create_project_sync(conn, "/test/path", Some("test")).map_err(Into::into));
        db!(pool, |conn| update_project_briefing_sync(conn, project_id, "abc123", Some("Briefing text")).map_err(Into::into));
        db!(pool, |conn| mark_session_for_briefing_sync(conn, project_id).map_err(Into::into));

        let briefing = db!(pool, |conn| get_project_briefing_sync(conn, project_id).map_err(Into::into)).unwrap();
        assert_eq!(briefing.briefing_text, None);
        assert!(briefing.last_session_at.is_some());
    }

    #[tokio::test]
    async fn test_get_projects_for_briefing_check() {
        let pool = setup_test_pool().await;
        let (project1, _) = db!(pool, |conn| get_or_create_project_sync(conn, "/path1", Some("p1")).map_err(Into::into));
        db!(pool, |conn| get_or_create_project_sync(conn, "/path2", Some("p2")).map_err(Into::into));
        db!(pool, |conn| mark_session_for_briefing_sync(conn, project1).map_err(Into::into));

        let projects = db!(pool, |conn| get_projects_for_briefing_check_sync(conn).map_err(Into::into));
        assert!(projects.len() >= 2);
    }

    // ═══════════════════════════════════════
    // server_state Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_set_and_get_server_state() {
        let pool = setup_test_pool().await;
        db!(pool, |conn| set_server_state_sync(conn, "test_key", "test_value").map_err(Into::into));
        let value = db!(pool, |conn| get_server_state_sync(conn, "test_key").map_err(Into::into));
        assert_eq!(value, Some("test_value".to_string()));
    }

    #[tokio::test]
    async fn test_get_server_state_nonexistent() {
        let pool = setup_test_pool().await;
        let value = db!(pool, |conn| get_server_state_sync(conn, "nonexistent").map_err(Into::into));
        assert!(value.is_none());
    }

    #[tokio::test]
    async fn test_set_server_state_upsert() {
        let pool = setup_test_pool().await;
        db!(pool, |conn| set_server_state_sync(conn, "key", "value1").map_err(Into::into));
        db!(pool, |conn| set_server_state_sync(conn, "key", "value2").map_err(Into::into));
        let value = db!(pool, |conn| get_server_state_sync(conn, "key").map_err(Into::into));
        assert_eq!(value, Some("value2".to_string()));
    }

    #[tokio::test]
    async fn test_delete_server_state() {
        let pool = setup_test_pool().await;
        db!(pool, |conn| set_server_state_sync(conn, "key", "value").map_err(Into::into));
        let deleted = db!(pool, |conn| delete_server_state_sync(conn, "key").map_err(Into::into));
        assert!(deleted);
        let value = db!(pool, |conn| get_server_state_sync(conn, "key").map_err(Into::into));
        assert!(value.is_none());
    }

    #[tokio::test]
    async fn test_delete_server_state_nonexistent() {
        let pool = setup_test_pool().await;
        let deleted = db!(pool, |conn| delete_server_state_sync(conn, "nonexistent").map_err(Into::into));
        assert!(!deleted);
    }

    // ═══════════════════════════════════════
    // active_project Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_save_and_get_active_project() {
        let pool = setup_test_pool().await;
        db!(pool, |conn| save_active_project_sync(conn, "/my/project").map_err(Into::into));
        let project = db!(pool, |conn| get_last_active_project_sync(conn).map_err(Into::into));
        assert_eq!(project, Some("/my/project".to_string()));
    }

    #[tokio::test]
    async fn test_clear_active_project() {
        let pool = setup_test_pool().await;
        db!(pool, |conn| save_active_project_sync(conn, "/my/project").map_err(Into::into));
        db!(pool, |conn| clear_active_project_sync(conn).map_err(Into::into));
        let project = db!(pool, |conn| get_last_active_project_sync(conn).map_err(Into::into));
        assert!(project.is_none());
    }

    #[tokio::test]
    async fn test_update_active_project() {
        let pool = setup_test_pool().await;
        db!(pool, |conn| save_active_project_sync(conn, "/project1").map_err(Into::into));
        db!(pool, |conn| save_active_project_sync(conn, "/project2").map_err(Into::into));
        let project = db!(pool, |conn| get_last_active_project_sync(conn).map_err(Into::into));
        assert_eq!(project, Some("/project2".to_string()));
    }

    // ═══════════════════════════════════════
    // Integration Tests
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_full_project_lifecycle() {
        let pool = setup_test_pool().await;
        let (project_id, _) = db!(pool, |conn| get_or_create_project_sync(conn, "/my/project", Some("MyProject")).map_err(Into::into));

        let projects = db!(pool, |conn| list_projects_sync(conn).map_err(Into::into));
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].0, project_id);

        let info = db!(pool, |conn| get_project_info_sync(conn, project_id).map_err(Into::into)).unwrap();
        assert_eq!(info.0, Some("MyProject".to_string()));
        assert_eq!(info.1, "/my/project");

        db!(pool, |conn| update_project_briefing_sync(conn, project_id, "commit123", Some("Changes made")).map_err(Into::into));
        let briefing = db!(pool, |conn| get_project_briefing_sync(conn, project_id).map_err(Into::into)).unwrap();
        assert_eq!(briefing.briefing_text, Some("Changes made".to_string()));

        db!(pool, |conn| mark_session_for_briefing_sync(conn, project_id).map_err(Into::into));
        let briefing = db!(pool, |conn| get_project_briefing_sync(conn, project_id).map_err(Into::into)).unwrap();
        assert!(briefing.briefing_text.is_none());
    }

    // ═══════════════════════════════════════
    // Edge Cases
    // ═══════════════════════════════════════

    #[tokio::test]
    async fn test_empty_project_path() {
        let pool = setup_test_pool().await;
        let (id, _name) = db!(pool, |conn| get_or_create_project_sync(conn, "", None).map_err(Into::into));
        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_very_long_project_name() {
        let pool = setup_test_pool().await;
        let long_name = "a".repeat(1000);
        let long_name_clone = long_name.clone();
        let (id, stored_name) = db!(pool, |conn| get_or_create_project_sync(conn, "/test", Some(&long_name_clone)).map_err(Into::into));
        assert!(id > 0);
        assert_eq!(stored_name, Some(long_name));
    }

    #[tokio::test]
    async fn test_special_characters_in_path() {
        let pool = setup_test_pool().await;
        let paths = vec![
            "/path/with spaces",
            "/path/with-dashes",
            "/path/with_underscores",
            "/path/with.dots",
        ];
        for path in paths {
            let path_str = path.to_string();
            let (id, _) = db!(pool, |conn| get_or_create_project_sync(conn, &path_str, None).map_err(Into::into));
            assert!(id > 0, "Failed for path: {}", path);
        }
    }

    #[tokio::test]
    async fn test_empty_server_state_key() {
        let pool = setup_test_pool().await;
        db!(pool, |conn| set_server_state_sync(conn, "", "value").map_err(Into::into));
        let value = db!(pool, |conn| get_server_state_sync(conn, "").map_err(Into::into));
        assert_eq!(value, Some("value".to_string()));
    }

    #[tokio::test]
    async fn test_empty_server_state_value() {
        let pool = setup_test_pool().await;
        db!(pool, |conn| set_server_state_sync(conn, "key", "").map_err(Into::into));
        let value = db!(pool, |conn| get_server_state_sync(conn, "key").map_err(Into::into));
        assert_eq!(value, Some("".to_string()));
    }

    #[tokio::test]
    async fn test_unicode_project_name() {
        let pool = setup_test_pool().await;
        let unicode_name = "🎉 项目 🚀";
        let (id, stored_name) = db!(pool, |conn| get_or_create_project_sync(conn, "/test", Some(unicode_name)).map_err(Into::into));
        assert!(id > 0);
        assert_eq!(stored_name, Some(unicode_name.to_string()));
    }
}
