// backend/src/memory/features/code_intelligence/call_graph.rs
// Call graph management: caller-callee relationships, traversal, and impact analysis

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet, VecDeque};
use tracing::{debug, info};

// ============================================================================
// Data Structures
// ============================================================================

/// A call relationship between two code elements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallEdge {
    pub id: Option<i64>,
    pub caller_id: i64,
    pub callee_id: i64,
    pub call_line: i64,
}

/// A code element with basic info for call graph traversal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallGraphElement {
    pub id: i64,
    pub name: String,
    pub element_type: String,
    pub file_path: String,
    pub start_line: i64,
}

/// Impact analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactAnalysis {
    /// The element being analyzed
    pub source_element: CallGraphElement,
    /// Elements that directly call this element
    pub direct_callers: Vec<CallGraphElement>,
    /// Elements that indirectly call this element (transitive)
    pub indirect_callers: Vec<CallGraphElement>,
    /// Elements that this element directly calls
    pub direct_callees: Vec<CallGraphElement>,
    /// Elements that this element indirectly calls (transitive)
    pub indirect_callees: Vec<CallGraphElement>,
    /// Total impact score (based on caller count and depth)
    pub impact_score: f64,
}

/// Path between two elements in the call graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallPath {
    pub elements: Vec<CallGraphElement>,
    pub call_lines: Vec<i64>,
    pub total_depth: usize,
}

// ============================================================================
// Call Graph Service
// ============================================================================

/// Service for managing and traversing the call graph
pub struct CallGraphService {
    pool: SqlitePool,
}

impl CallGraphService {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // ========================================================================
    // Basic CRUD Operations
    // ========================================================================

    /// Record a call relationship between two elements
    pub async fn add_call(
        &self,
        caller_id: i64,
        callee_id: i64,
        call_line: i64,
    ) -> Result<CallEdge> {
        let result = sqlx::query!(
            r#"
            INSERT INTO call_graph (caller_id, callee_id, call_line)
            VALUES (?, ?, ?)
            ON CONFLICT(caller_id, callee_id, call_line) DO UPDATE SET
                caller_id = excluded.caller_id
            RETURNING id
            "#,
            caller_id,
            callee_id,
            call_line
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(CallEdge {
            id: Some(result.id),
            caller_id,
            callee_id,
            call_line,
        })
    }

    /// Add multiple call relationships in a batch
    pub async fn add_calls_batch(&self, calls: &[(i64, i64, i64)]) -> Result<usize> {
        let mut tx = self.pool.begin().await?;
        let mut count = 0;

        for (caller_id, callee_id, call_line) in calls {
            sqlx::query!(
                r#"
                INSERT INTO call_graph (caller_id, callee_id, call_line)
                VALUES (?, ?, ?)
                ON CONFLICT(caller_id, callee_id, call_line) DO NOTHING
                "#,
                caller_id,
                callee_id,
                call_line
            )
            .execute(&mut *tx)
            .await?;
            count += 1;
        }

        tx.commit().await?;
        Ok(count)
    }

    /// Remove a call relationship
    pub async fn remove_call(&self, caller_id: i64, callee_id: i64) -> Result<u64> {
        let result = sqlx::query!(
            "DELETE FROM call_graph WHERE caller_id = ? AND callee_id = ?",
            caller_id,
            callee_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Remove all calls from a specific caller
    pub async fn remove_calls_from(&self, caller_id: i64) -> Result<u64> {
        let result = sqlx::query!("DELETE FROM call_graph WHERE caller_id = ?", caller_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Remove all calls involving elements from a file
    pub async fn remove_calls_for_file(&self, file_id: i64) -> Result<u64> {
        let result = sqlx::query!(
            r#"
            DELETE FROM call_graph
            WHERE caller_id IN (SELECT id FROM code_elements WHERE file_id = ?)
               OR callee_id IN (SELECT id FROM code_elements WHERE file_id = ?)
            "#,
            file_id,
            file_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    // ========================================================================
    // Query Operations
    // ========================================================================

    /// Get all direct callers of an element
    pub async fn get_callers(&self, callee_id: i64) -> Result<Vec<CallGraphElement>> {
        let rows = sqlx::query!(
            r#"
            SELECT ce.id as "id!", ce.name, ce.element_type, ce.file_path, ce.start_line
            FROM code_elements ce
            JOIN call_graph cg ON ce.id = cg.caller_id
            WHERE cg.callee_id = ?
            "#,
            callee_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CallGraphElement {
                id: r.id,
                name: r.name,
                element_type: r.element_type,
                file_path: r.file_path.unwrap_or_default(),
                start_line: r.start_line,
            })
            .collect())
    }

    /// Get all direct callees of an element
    pub async fn get_callees(&self, caller_id: i64) -> Result<Vec<CallGraphElement>> {
        let rows = sqlx::query!(
            r#"
            SELECT ce.id as "id!", ce.name, ce.element_type, ce.file_path, ce.start_line
            FROM code_elements ce
            JOIN call_graph cg ON ce.id = cg.callee_id
            WHERE cg.caller_id = ?
            "#,
            caller_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CallGraphElement {
                id: r.id,
                name: r.name,
                element_type: r.element_type,
                file_path: r.file_path.unwrap_or_default(),
                start_line: r.start_line,
            })
            .collect())
    }

    /// Get all call edges for an element (both caller and callee)
    pub async fn get_all_edges(&self, element_id: i64) -> Result<Vec<CallEdge>> {
        let rows = sqlx::query!(
            r#"
            SELECT id, caller_id, callee_id, call_line
            FROM call_graph
            WHERE caller_id = ? OR callee_id = ?
            "#,
            element_id,
            element_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CallEdge {
                id: Some(r.id),
                caller_id: r.caller_id,
                callee_id: r.callee_id,
                call_line: r.call_line,
            })
            .collect())
    }

    /// Get element by ID
    async fn get_element(&self, element_id: i64) -> Result<Option<CallGraphElement>> {
        let row = sqlx::query!(
            r#"
            SELECT id as "id!", name, element_type, file_path, start_line
            FROM code_elements
            WHERE id = ?
            "#,
            element_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| CallGraphElement {
            id: r.id,
            name: r.name,
            element_type: r.element_type,
            file_path: r.file_path.unwrap_or_default(),
            start_line: r.start_line,
        }))
    }

    // ========================================================================
    // Traversal Operations
    // ========================================================================

    /// Get all transitive callers (elements that eventually call this element)
    pub async fn get_transitive_callers(
        &self,
        element_id: i64,
        max_depth: usize,
    ) -> Result<Vec<(CallGraphElement, usize)>> {
        let mut visited: HashSet<i64> = HashSet::new();
        let mut result: Vec<(CallGraphElement, usize)> = Vec::new();
        let mut queue: VecDeque<(i64, usize)> = VecDeque::new();

        visited.insert(element_id);
        queue.push_back((element_id, 0));

        while let Some((current_id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            let callers = self.get_callers(current_id).await?;
            for caller in callers {
                if !visited.contains(&caller.id) {
                    visited.insert(caller.id);
                    result.push((caller.clone(), depth + 1));
                    queue.push_back((caller.id, depth + 1));
                }
            }
        }

        Ok(result)
    }

    /// Get all transitive callees (elements that this element eventually calls)
    pub async fn get_transitive_callees(
        &self,
        element_id: i64,
        max_depth: usize,
    ) -> Result<Vec<(CallGraphElement, usize)>> {
        let mut visited: HashSet<i64> = HashSet::new();
        let mut result: Vec<(CallGraphElement, usize)> = Vec::new();
        let mut queue: VecDeque<(i64, usize)> = VecDeque::new();

        visited.insert(element_id);
        queue.push_back((element_id, 0));

        while let Some((current_id, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }

            let callees = self.get_callees(current_id).await?;
            for callee in callees {
                if !visited.contains(&callee.id) {
                    visited.insert(callee.id);
                    result.push((callee.clone(), depth + 1));
                    queue.push_back((callee.id, depth + 1));
                }
            }
        }

        Ok(result)
    }

    /// Find shortest path between two elements
    pub async fn find_path(
        &self,
        from_id: i64,
        to_id: i64,
        max_depth: usize,
    ) -> Result<Option<CallPath>> {
        let mut visited: HashSet<i64> = HashSet::new();
        let mut parent_map: HashMap<i64, (i64, i64)> = HashMap::new(); // child -> (parent, call_line)
        let mut queue: VecDeque<(i64, usize)> = VecDeque::new();

        visited.insert(from_id);
        queue.push_back((from_id, 0));

        let mut found = false;

        while let Some((current_id, depth)) = queue.pop_front() {
            if current_id == to_id {
                found = true;
                break;
            }

            if depth >= max_depth {
                continue;
            }

            // Get callees with call lines
            let rows = sqlx::query!(
                r#"
                SELECT callee_id, call_line
                FROM call_graph
                WHERE caller_id = ?
                "#,
                current_id
            )
            .fetch_all(&self.pool)
            .await?;

            for row in rows {
                if !visited.contains(&row.callee_id) {
                    visited.insert(row.callee_id);
                    parent_map.insert(row.callee_id, (current_id, row.call_line));
                    queue.push_back((row.callee_id, depth + 1));
                }
            }
        }

        if !found {
            return Ok(None);
        }

        // Reconstruct path
        let mut path_ids: Vec<i64> = Vec::new();
        let mut call_lines: Vec<i64> = Vec::new();
        let mut current = to_id;

        while current != from_id {
            path_ids.push(current);
            if let Some(&(parent, line)) = parent_map.get(&current) {
                call_lines.push(line);
                current = parent;
            } else {
                break;
            }
        }
        path_ids.push(from_id);

        path_ids.reverse();
        call_lines.reverse();

        // Fetch element details
        let mut elements: Vec<CallGraphElement> = Vec::new();
        for id in &path_ids {
            if let Some(element) = self.get_element(*id).await? {
                elements.push(element);
            }
        }

        Ok(Some(CallPath {
            total_depth: elements.len() - 1,
            elements,
            call_lines,
        }))
    }

    // ========================================================================
    // Impact Analysis
    // ========================================================================

    /// Perform impact analysis for an element
    pub async fn analyze_impact(&self, element_id: i64, max_depth: usize) -> Result<ImpactAnalysis> {
        info!("Analyzing impact for element {}", element_id);

        let source_element = self
            .get_element(element_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Element not found: {}", element_id))?;

        // Get direct relationships
        let direct_callers = self.get_callers(element_id).await?;
        let direct_callees = self.get_callees(element_id).await?;

        // Get transitive relationships
        let transitive_callers = self.get_transitive_callers(element_id, max_depth).await?;
        let transitive_callees = self.get_transitive_callees(element_id, max_depth).await?;

        // Filter to indirect only (not in direct)
        let direct_caller_ids: HashSet<i64> = direct_callers.iter().map(|c| c.id).collect();
        let direct_callee_ids: HashSet<i64> = direct_callees.iter().map(|c| c.id).collect();

        let indirect_callers: Vec<CallGraphElement> = transitive_callers
            .into_iter()
            .filter(|(e, _)| !direct_caller_ids.contains(&e.id))
            .map(|(e, _)| e)
            .collect();

        let indirect_callees: Vec<CallGraphElement> = transitive_callees
            .into_iter()
            .filter(|(e, _)| !direct_callee_ids.contains(&e.id))
            .map(|(e, _)| e)
            .collect();

        // Calculate impact score
        // Higher score = more elements affected by changes
        let total_callers = direct_callers.len() + indirect_callers.len();
        let impact_score = if total_callers == 0 {
            0.0
        } else {
            // Score considers both count and having indirect callers
            let base_score = (total_callers as f64).ln() / 10.0;
            let indirect_bonus = if !indirect_callers.is_empty() {
                0.2
            } else {
                0.0
            };
            (base_score + indirect_bonus).min(1.0)
        };

        debug!(
            "Impact analysis for {}: {} direct callers, {} indirect callers, score: {:.2}",
            source_element.name,
            direct_callers.len(),
            indirect_callers.len(),
            impact_score
        );

        Ok(ImpactAnalysis {
            source_element,
            direct_callers,
            indirect_callers,
            direct_callees,
            indirect_callees,
            impact_score,
        })
    }

    /// Find all entry points (functions with no callers) in a project
    pub async fn find_entry_points(&self, project_id: &str) -> Result<Vec<CallGraphElement>> {
        let rows = sqlx::query!(
            r#"
            SELECT ce.id as "id!", ce.name, ce.element_type, ce.file_path, ce.start_line
            FROM code_elements ce
            WHERE ce.project_id = ?
              AND ce.element_type = 'function'
              AND ce.id NOT IN (SELECT callee_id FROM call_graph)
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CallGraphElement {
                id: r.id,
                name: r.name,
                element_type: r.element_type,
                file_path: r.file_path.unwrap_or_default(),
                start_line: r.start_line,
            })
            .collect())
    }

    /// Find all leaf functions (functions that don't call anything)
    pub async fn find_leaf_functions(&self, project_id: &str) -> Result<Vec<CallGraphElement>> {
        let rows = sqlx::query!(
            r#"
            SELECT ce.id as "id!", ce.name, ce.element_type, ce.file_path, ce.start_line
            FROM code_elements ce
            WHERE ce.project_id = ?
              AND ce.element_type = 'function'
              AND ce.id NOT IN (SELECT caller_id FROM call_graph)
            "#,
            project_id
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CallGraphElement {
                id: r.id,
                name: r.name,
                element_type: r.element_type,
                file_path: r.file_path.unwrap_or_default(),
                start_line: r.start_line,
            })
            .collect())
    }

    /// Get call graph statistics for a project
    pub async fn get_statistics(&self, project_id: &str) -> Result<CallGraphStats> {
        // Count total elements
        let element_count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) as count FROM code_elements WHERE project_id = ? AND element_type = 'function'",
            project_id
        )
        .fetch_one(&self.pool)
        .await? as i64;

        // Count total edges
        let edge_count: i64 = sqlx::query_scalar!(
            r#"
            SELECT COUNT(*) as count FROM call_graph cg
            JOIN code_elements ce ON cg.caller_id = ce.id
            WHERE ce.project_id = ?
            "#,
            project_id
        )
        .fetch_one(&self.pool)
        .await? as i64;

        // Entry points
        let entry_points = self.find_entry_points(project_id).await?.len();

        // Leaf functions
        let leaf_functions = self.find_leaf_functions(project_id).await?.len();

        Ok(CallGraphStats {
            total_functions: element_count as usize,
            total_edges: edge_count as usize,
            entry_points,
            leaf_functions,
            avg_out_degree: if element_count > 0 {
                edge_count as f64 / element_count as f64
            } else {
                0.0
            },
        })
    }
}

/// Statistics about the call graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallGraphStats {
    pub total_functions: usize,
    pub total_edges: usize,
    pub entry_points: usize,
    pub leaf_functions: usize,
    pub avg_out_degree: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call_edge_creation() {
        let edge = CallEdge {
            id: Some(1),
            caller_id: 10,
            callee_id: 20,
            call_line: 42,
        };

        assert_eq!(edge.caller_id, 10);
        assert_eq!(edge.callee_id, 20);
        assert_eq!(edge.call_line, 42);
    }

    #[test]
    fn test_call_graph_element() {
        let element = CallGraphElement {
            id: 1,
            name: "process_data".to_string(),
            element_type: "function".to_string(),
            file_path: "src/main.rs".to_string(),
            start_line: 10,
        };

        assert_eq!(element.name, "process_data");
        assert_eq!(element.element_type, "function");
    }

    #[test]
    fn test_impact_analysis_structure() {
        let element = CallGraphElement {
            id: 1,
            name: "validate".to_string(),
            element_type: "function".to_string(),
            file_path: "src/auth.rs".to_string(),
            start_line: 50,
        };

        let analysis = ImpactAnalysis {
            source_element: element,
            direct_callers: vec![],
            indirect_callers: vec![],
            direct_callees: vec![],
            indirect_callees: vec![],
            impact_score: 0.5,
        };

        assert_eq!(analysis.impact_score, 0.5);
        assert!(analysis.direct_callers.is_empty());
    }
}
