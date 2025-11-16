// src/prompt/utils.rs
// Utility functions for prompt building

use crate::api::ws::message::MessageMetadata;

/// Check if the current context is code-related
pub fn is_code_related(metadata: Option<&MessageMetadata>) -> bool {
    if let Some(meta) = metadata {
        if meta.file_path.is_some() || meta.file_content.is_some() {
            return true;
        }

        if let Some(lang) = &meta.language {
            let code_languages = [
                "rust",
                "typescript",
                "javascript",
                "python",
                "go",
                "java",
                "cpp",
                "c",
            ];
            if code_languages.contains(&lang.to_lowercase().as_str()) {
                return true;
            }
        }

        if meta.repo_id.is_some() || meta.has_repository == Some(true) {
            return true;
        }
    }
    false
}

/// Derive programming language from file extension
pub fn language_from_extension(file_path: &str) -> &str {
    file_path
        .rsplit('.')
        .next()
        .map(|ext| match ext {
            "rs" => "rust",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "py" => "python",
            "go" => "go",
            "java" => "java",
            "cpp" | "cc" | "cxx" => "cpp",
            "c" | "h" => "c",
            _ => ext,
        })
        .unwrap_or("unknown")
}
