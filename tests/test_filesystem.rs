// tests/test_filesystem.rs
// Integration test for the filesystem WebSocket handler

#[cfg(test)]
mod tests {
    use serde_json::json;
    use mira_backend::api::ws::filesystem::handle_filesystem_command;
    use std::sync::Arc;
    use tempfile::TempDir;
    use std::fs;
    
    #[tokio::test]
    async fn test_file_save_and_read() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let test_file_path = temp_dir.path().join("test.txt");
        
        // Create a mock app_state (simplified for testing)
        // In real tests, you'd create a proper AppState
        let app_state = Arc::new(());  // Placeholder
        
        // Test saving a file
        let save_params = json!({
            "path": test_file_path.to_str().unwrap(),
            "content": "Hello from Mira test!"
        });
        
        let save_result = handle_filesystem_command(
            "file.save",
            save_params,
            app_state.clone()
        ).await;
        
        assert!(save_result.is_ok(), "Failed to save file: {:?}", save_result);
        
        // Verify file was created
        assert!(test_file_path.exists(), "File was not created");
        
        // Verify content
        let content = fs::read_to_string(&test_file_path).unwrap();
        assert_eq!(content, "Hello from Mira test!");
        
        // Test reading the file back
        let read_params = json!({
            "path": test_file_path.to_str().unwrap()
        });
        
        let read_result = handle_filesystem_command(
            "file.read",
            read_params,
            app_state.clone()
        ).await;
        
        assert!(read_result.is_ok(), "Failed to read file: {:?}", read_result);
        
        // Clean up is automatic with TempDir
    }
    
    #[tokio::test]
    async fn test_file_save_security() {
        let app_state = Arc::new(());  // Placeholder
        
        // Test directory traversal protection
        let malicious_params = json!({
            "path": "../../../etc/passwd",
            "content": "hacked!"
        });
        
        let result = handle_filesystem_command(
            "file.save",
            malicious_params,
            app_state.clone()
        ).await;
        
        assert!(result.is_err(), "Should have blocked directory traversal");
        
        // Test absolute path outside allowed directories
        let forbidden_params = json!({
            "path": "/etc/passwd",
            "content": "hacked!"
        });
        
        let result = handle_filesystem_command(
            "file.save",
            forbidden_params,
            app_state.clone()
        ).await;
        
        assert!(result.is_err(), "Should have blocked forbidden path");
    }
}
