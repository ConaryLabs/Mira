// src/memory/features/code_intelligence/storage.rs
// Database operations for code intelligence data

use crate::memory::features::code_intelligence::types::*;
use anyhow::Result;
use sqlx::SqlitePool;
use tracing::info;

/// Storage operations for code intelligence
pub struct CodeIntelligenceStorage {
    pool: SqlitePool,
}

impl CodeIntelligenceStorage {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Store complete file analysis results
    pub async fn store_file_analysis(
        &self,
        file_id: i64,
        language: &str,
        analysis: &FileAnalysis,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // No more casting - use i64 directly (SQLite's native INTEGER)
        let element_count = analysis.elements.len() as i64;
        let complexity_score = analysis.complexity_score;

        // Update repository_files with analysis metadata
        sqlx::query!(
            r#"
            UPDATE repository_files 
            SET language = ?, ast_analyzed = TRUE, 
                element_count = ?, complexity_score = ?, last_analyzed = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
            language,
            element_count,
            complexity_score,
            file_id
        )
        .execute(&mut *tx)
        .await?;

        // Store code elements
        for element in &analysis.elements {
            // No casting - all i64 now
            let start_line = element.start_line;
            let end_line = element.end_line;
            let element_complexity = element.complexity_score;

            let element_id = sqlx::query!(
                r#"
                INSERT INTO code_elements (
                    file_id, language, element_type, name, full_path, visibility,
                    start_line, end_line, content, signature_hash, complexity_score,
                    is_test, is_async, documentation, metadata
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(file_id, name, start_line) DO UPDATE SET
                    content = excluded.content,
                    signature_hash = excluded.signature_hash,
                    complexity_score = excluded.complexity_score,
                    analyzed_at = CURRENT_TIMESTAMP
                "#,
                file_id,
                language,
                element.element_type,
                element.name,
                element.full_path,
                element.visibility,
                start_line,
                end_line,
                element.content,
                element.signature_hash,
                element_complexity,
                element.is_test,
                element.is_async,
                element.documentation,
                element.metadata
            )
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();

            // Store quality issues for this element
            for issue in &analysis.quality_issues {
                sqlx::query!(
                    r#"
                    INSERT INTO code_quality_issues (
                        element_id, issue_type, severity, title, description, 
                        suggested_fix, fix_confidence, is_auto_fixable
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                    "#,
                    element_id,
                    issue.issue_type,
                    issue.severity,
                    issue.title,
                    issue.description,
                    issue.suggested_fix,
                    issue.fix_confidence,
                    issue.is_auto_fixable
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        // Store external dependencies (simplified for Phase 1)
        for (element_idx, element) in analysis.elements.iter().enumerate() {
            for dep in &analysis.dependencies {
                if element_idx == 0 {
                    // Only attach to first element for now
                    let imported_symbols_json =
                        serde_json::to_string(&dep.imported_symbols).unwrap_or_default();

                    sqlx::query!(
                        r#"
                        INSERT INTO external_dependencies (element_id, import_path, imported_symbols, dependency_type)
                        SELECT id, ?, ?, ? FROM code_elements 
                        WHERE file_id = ? AND name = ?
                        "#,
                        dep.import_path,
                        imported_symbols_json,
                        dep.dependency_type,
                        file_id,
                        element.name
                    )
                    .execute(&mut *tx)
                    .await?;
                }
            }
        }

        tx.commit().await?;
        Ok(())
    }

    /// Delete all code intelligence data for a repository
    pub async fn delete_repository_data(&self, attachment_id: &str) -> Result<i64> {
        let mut tx = self.pool.begin().await?;

        // Get file IDs for this attachment
        let rows = sqlx::query!(
            "SELECT id FROM repository_files WHERE attachment_id = ?",
            attachment_id
        )
        .fetch_all(&mut *tx)
        .await?;

        let file_ids: Vec<i64> = rows.into_iter().filter_map(|row| row.id).collect();

        if file_ids.is_empty() {
            tx.commit().await?;
            return Ok(0);
        }

        let file_ids_str = file_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");

        // Delete code quality issues
        let delete_issues_query = format!(
            "DELETE FROM code_quality_issues WHERE element_id IN (SELECT id FROM code_elements WHERE file_id IN ({}))",
            file_ids_str
        );
        sqlx::query(&delete_issues_query).execute(&mut *tx).await?;

        // Delete external dependencies
        let delete_deps_query = format!(
            "DELETE FROM external_dependencies WHERE element_id IN (SELECT id FROM code_elements WHERE file_id IN ({}))",
            file_ids_str
        );
        sqlx::query(&delete_deps_query).execute(&mut *tx).await?;

        // Delete code elements and count them
        let delete_elements_query = format!(
            "DELETE FROM code_elements WHERE file_id IN ({})",
            file_ids_str
        );
        let result = sqlx::query(&delete_elements_query)
            .execute(&mut *tx)
            .await?;
        let deleted_count = result.rows_affected() as i64;

        // Reset repository_files analysis status
        sqlx::query!(
            r#"
            UPDATE repository_files 
            SET ast_analyzed = FALSE, element_count = 0, complexity_score = 0, last_analyzed = NULL
            WHERE attachment_id = ?
            "#,
            attachment_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        info!(
            "Deleted {} code elements for attachment {}",
            deleted_count, attachment_id
        );

        Ok(deleted_count)
    }

    /// Get all elements for a file
    pub async fn get_file_elements(&self, file_id: i64) -> Result<Vec<CodeElement>> {
        let rows = sqlx::query!(
            "SELECT * FROM code_elements WHERE file_id = ? ORDER BY start_line",
            file_id
        )
        .fetch_all(&self.pool)
        .await?;

        let mut elements = Vec::new();
        for row in rows {
            elements.push(CodeElement {
                element_type: row.element_type,
                name: row.name,
                full_path: row.full_path.unwrap_or_default(),
                visibility: row.visibility.unwrap_or_default(),
                start_line: row.start_line, // i64 -> i64 (no cast!)
                end_line: row.end_line,     // i64 -> i64 (no cast!)
                content: row.content.unwrap_or_default(),
                signature_hash: row.signature_hash.unwrap_or_default(),
                complexity_score: row.complexity_score.unwrap_or(0.0) as i64,
                is_test: row.is_test.unwrap_or(false),
                is_async: row.is_async.unwrap_or(false),
                documentation: row.documentation,
                metadata: row.metadata,
            });
        }

        Ok(elements)
    }

    /// Search for elements by name pattern (project-scoped)
    pub async fn search_elements_for_project(
        &self,
        pattern: &str,
        project_id: &str,
        limit: i32,
    ) -> Result<Vec<CodeElement>> {
        let search_pattern = format!("%{}%", pattern);
        let prefix_pattern = format!("{}%", pattern);

        let rows = sqlx::query!(
            r#"
            SELECT ce.* FROM code_elements ce
            JOIN repository_files rf ON ce.file_id = rf.id
            JOIN git_repo_attachments gra ON rf.attachment_id = gra.id
            WHERE gra.project_id = ? AND (ce.name LIKE ? OR ce.full_path LIKE ?)
            ORDER BY 
                CASE WHEN ce.name = ? THEN 0 
                     WHEN ce.name LIKE ? THEN 1 
                     ELSE 2 END,
                ce.name
            LIMIT ?
            "#,
            project_id,
            search_pattern,
            search_pattern,
            pattern,
            prefix_pattern,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let mut elements = Vec::new();
        for row in rows {
            elements.push(CodeElement {
                element_type: row.element_type,
                name: row.name,
                full_path: row.full_path.unwrap_or_default(),
                visibility: row.visibility.unwrap_or_default(),
                start_line: row.start_line,
                end_line: row.end_line,
                content: row.content.unwrap_or_default(),
                signature_hash: row.signature_hash.unwrap_or_default(),
                complexity_score: row.complexity_score.unwrap_or(0.0) as i64,
                is_test: row.is_test.unwrap_or(false),
                is_async: row.is_async.unwrap_or(false),
                documentation: row.documentation,
                metadata: row.metadata,
            });
        }

        Ok(elements)
    }

    /// Get quality issues for a file
    pub async fn get_file_quality_issues(&self, file_id: i64) -> Result<Vec<QualityIssue>> {
        let rows = sqlx::query!(
            r#"
            SELECT cqi.* FROM code_quality_issues cqi
            JOIN code_elements ce ON cqi.element_id = ce.id
            WHERE ce.file_id = ?
            ORDER BY cqi.severity DESC, cqi.detected_at DESC
            "#,
            file_id
        )
        .fetch_all(&self.pool)
        .await?;

        let mut issues = Vec::new();
        for row in rows {
            issues.push(QualityIssue {
                issue_type: row.issue_type,
                severity: row.severity,
                title: row.title.unwrap_or_default(),
                description: row.description.unwrap_or_default(),
                suggested_fix: row.suggested_fix,
                fix_confidence: row.fix_confidence.unwrap_or(0.0),
                is_auto_fixable: row.is_auto_fixable.unwrap_or(false),
            });
        }

        Ok(issues)
    }

    /// Get elements by type (functions, structs, etc.) - project-scoped
    pub async fn get_elements_by_type_for_project(
        &self,
        element_type: &str,
        project_id: &str,
        limit: Option<i32>,
    ) -> Result<Vec<CodeElement>> {
        let limit = limit.unwrap_or(100);

        let rows = sqlx::query!(
            r#"
            SELECT ce.* FROM code_elements ce
            JOIN repository_files rf ON ce.file_id = rf.id
            JOIN git_repo_attachments gra ON rf.attachment_id = gra.id
            WHERE gra.project_id = ? AND ce.element_type = ?
            ORDER BY ce.name 
            LIMIT ?
            "#,
            project_id,
            element_type,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let mut elements = Vec::new();
        for row in rows {
            elements.push(CodeElement {
                element_type: row.element_type,
                name: row.name,
                full_path: row.full_path.unwrap_or_default(),
                visibility: row.visibility.unwrap_or_default(),
                start_line: row.start_line,
                end_line: row.end_line,
                content: row.content.unwrap_or_default(),
                signature_hash: row.signature_hash.unwrap_or_default(),
                complexity_score: row.complexity_score.unwrap_or(0.0) as i64,
                is_test: row.is_test.unwrap_or(false),
                is_async: row.is_async.unwrap_or(false),
                documentation: row.documentation,
                metadata: row.metadata,
            });
        }

        Ok(elements)
    }

    /// Get analysis statistics for a repository
    pub async fn get_repo_stats(&self, attachment_id: &str) -> Result<RepoStats> {
        let stats = sqlx::query!(
            r#"
            SELECT
                COUNT(*) as "total_files: i64",
                SUM(CASE WHEN ast_analyzed = TRUE THEN 1 ELSE 0 END) as "analyzed_files: i64",
                SUM(element_count) as "total_elements: i64",
                AVG(complexity_score) as "avg_complexity: f64"
            FROM repository_files
            WHERE attachment_id = ?
            "#,
            attachment_id
        )
        .fetch_one(&self.pool)
        .await?;

        let quality_stats = sqlx::query!(
            r#"
            SELECT
                COUNT(*) as "total_issues: i64",
                SUM(CASE WHEN severity = 'critical' THEN 1 ELSE 0 END) as "critical_issues: i64",
                SUM(CASE WHEN severity = 'high' THEN 1 ELSE 0 END) as "high_issues: i64"
            FROM code_quality_issues cqi
            JOIN code_elements ce ON cqi.element_id = ce.id
            JOIN repository_files rf ON ce.file_id = rf.id
            WHERE rf.attachment_id = ?
            "#,
            attachment_id
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(RepoStats {
            total_files: stats.total_files,
            analyzed_files: stats.analyzed_files.unwrap_or(0),
            total_elements: stats.total_elements.unwrap_or(0),
            avg_complexity: stats.avg_complexity.unwrap_or(0.0),
            total_quality_issues: quality_stats.total_issues,
            critical_issues: quality_stats.critical_issues.unwrap_or(0),
            high_issues: quality_stats.high_issues.unwrap_or(0),
        })
    }

    /// Find the most complex functions across a project - project-scoped
    pub async fn get_complexity_hotspots_for_project(
        &self,
        project_id: &str,
        limit: i32,
    ) -> Result<Vec<CodeElement>> {
        let rows = sqlx::query!(
            r#"
            SELECT ce.* FROM code_elements ce
            JOIN repository_files rf ON ce.file_id = rf.id
            JOIN git_repo_attachments gra ON rf.attachment_id = gra.id
            WHERE gra.project_id = ? AND ce.element_type = 'function' AND ce.complexity_score > 5
            ORDER BY ce.complexity_score DESC 
            LIMIT ?
            "#,
            project_id,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let mut elements = Vec::new();
        for row in rows {
            elements.push(CodeElement {
                element_type: row.element_type,
                name: row.name,
                full_path: row.full_path.unwrap_or_default(),
                visibility: row.visibility.unwrap_or_default(),
                start_line: row.start_line,
                end_line: row.end_line,
                content: row.content.unwrap_or_default(),
                signature_hash: row.signature_hash.unwrap_or_default(),
                complexity_score: row.complexity_score.unwrap_or(0.0) as i64,
                is_test: row.is_test.unwrap_or(false),
                is_async: row.is_async.unwrap_or(false),
                documentation: row.documentation,
                metadata: row.metadata,
            });
        }

        Ok(elements)
    }

    /// Search for elements by name pattern (global search - all projects)
    pub async fn search_elements(&self, pattern: &str, limit: i32) -> Result<Vec<CodeElement>> {
        let search_pattern = format!("%{}%", pattern);
        let prefix_pattern = format!("{}%", pattern);

        let rows = sqlx::query!(
            r#"
            SELECT * FROM code_elements
            WHERE name LIKE ? OR full_path LIKE ?
            ORDER BY 
                CASE WHEN name = ? THEN 0 
                     WHEN name LIKE ? THEN 1 
                     ELSE 2 END,
                name
            LIMIT ?
            "#,
            search_pattern,
            search_pattern,
            pattern,
            prefix_pattern,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let mut elements = Vec::new();
        for row in rows {
            elements.push(CodeElement {
                element_type: row.element_type,
                name: row.name,
                full_path: row.full_path.unwrap_or_default(),
                visibility: row.visibility.unwrap_or_default(),
                start_line: row.start_line,
                end_line: row.end_line,
                content: row.content.unwrap_or_default(),
                signature_hash: row.signature_hash.unwrap_or_default(),
                complexity_score: row.complexity_score.unwrap_or(0.0) as i64,
                is_test: row.is_test.unwrap_or(false),
                is_async: row.is_async.unwrap_or(false),
                documentation: row.documentation,
                metadata: row.metadata,
            });
        }

        Ok(elements)
    }

    /// Get complexity hotspots (global - all projects)
    pub async fn get_complexity_hotspots(&self, limit: i32) -> Result<Vec<CodeElement>> {
        let rows = sqlx::query!(
            r#"
            SELECT * FROM code_elements
            WHERE complexity_score > 5
            ORDER BY complexity_score DESC, name
            LIMIT ?
            "#,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let mut elements = Vec::new();
        for row in rows {
            elements.push(CodeElement {
                element_type: row.element_type,
                name: row.name,
                full_path: row.full_path.unwrap_or_default(),
                visibility: row.visibility.unwrap_or_default(),
                start_line: row.start_line,
                end_line: row.end_line,
                content: row.content.unwrap_or_default(),
                signature_hash: row.signature_hash.unwrap_or_default(),
                complexity_score: row.complexity_score.unwrap_or(0.0) as i64,
                is_test: row.is_test.unwrap_or(false),
                is_async: row.is_async.unwrap_or(false),
                documentation: row.documentation,
                metadata: row.metadata,
            });
        }

        Ok(elements)
    }

    /// Get elements by type (global - all projects)
    pub async fn get_elements_by_type(
        &self,
        element_type: &str,
        limit: Option<i32>,
    ) -> Result<Vec<CodeElement>> {
        let limit = limit.unwrap_or(20);

        let rows = sqlx::query!(
            r#"
            SELECT * FROM code_elements
            WHERE element_type = ?
            ORDER BY name
            LIMIT ?
            "#,
            element_type,
            limit
        )
        .fetch_all(&self.pool)
        .await?;

        let mut elements = Vec::new();
        for row in rows {
            elements.push(CodeElement {
                element_type: row.element_type,
                name: row.name,
                full_path: row.full_path.unwrap_or_default(),
                visibility: row.visibility.unwrap_or_default(),
                start_line: row.start_line,
                end_line: row.end_line,
                content: row.content.unwrap_or_default(),
                signature_hash: row.signature_hash.unwrap_or_default(),
                complexity_score: row.complexity_score.unwrap_or(0.0) as i64,
                is_test: row.is_test.unwrap_or(false),
                is_async: row.is_async.unwrap_or(false),
                documentation: row.documentation,
                metadata: row.metadata,
            });
        }

        Ok(elements)
    }
}

/// Statistics for a repository's code analysis
#[derive(Debug)]
pub struct RepoStats {
    pub total_files: i64,    // Changed from u32 - matches SQLite INTEGER
    pub analyzed_files: i64, // Changed from u32 - matches SQLite INTEGER
    pub total_elements: i64, // Changed from u32 - matches SQLite INTEGER
    pub avg_complexity: f64,
    pub total_quality_issues: i64, // Changed from u32 - matches SQLite INTEGER
    pub critical_issues: i64,      // Changed from u32 - matches SQLite INTEGER
    pub high_issues: i64,          // Changed from u32 - matches SQLite INTEGER
}

/// Semantic stats for a single file
#[derive(Debug, Clone)]
pub struct FileSemanticStats {
    pub file_path: String,
    pub language: Option<String>,
    pub element_count: i64,
    pub complexity_score: Option<f64>,
    pub quality_issue_count: i64,
    pub is_test_file: bool,
    pub is_analyzed: bool,
    pub function_count: i64,
    pub line_count: i64,
}

impl CodeIntelligenceStorage {
    /// Get semantic stats for all files in a project
    pub async fn get_file_semantic_stats(&self, project_id: &str) -> Result<Vec<FileSemanticStats>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                rf.file_path,
                rf.language,
                rf.element_count,
                rf.complexity_score,
                rf.ast_analyzed as "is_analyzed: bool",
                rf.function_count,
                rf.line_count,
                (SELECT COUNT(*) FROM code_quality_issues cqi
                 JOIN code_elements ce ON cqi.element_id = ce.id
                 WHERE ce.file_id = rf.id) as "quality_issue_count: i64",
                CASE
                    WHEN rf.file_path LIKE '%_test%' OR rf.file_path LIKE '%test_%'
                         OR rf.file_path LIKE '%/tests/%' OR rf.file_path LIKE '%_spec%'
                         OR rf.file_path LIKE '%.test.%' OR rf.file_path LIKE '%.spec.%'
                    THEN 1 ELSE 0
                END as "is_test_file: i64"
            FROM repository_files rf
            WHERE rf.project_id = ?
            ORDER BY rf.file_path
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        let stats = rows
            .into_iter()
            .map(|r| FileSemanticStats {
                file_path: r.file_path,
                language: r.language,
                element_count: r.element_count.unwrap_or(0),
                complexity_score: r.complexity_score,
                quality_issue_count: r.quality_issue_count.unwrap_or(0),
                is_test_file: r.is_test_file.unwrap_or(0) == 1,
                is_analyzed: r.is_analyzed.unwrap_or(false),
                function_count: r.function_count.unwrap_or(0),
                line_count: r.line_count.unwrap_or(0),
            })
            .collect();

        Ok(stats)
    }
}
