-- Seed Mira usage guidelines
-- Run after schema migration: sqlite3 data/mira.db < migrations/seed_mira_guidelines.sql

-- Clear existing mira guidelines (idempotent)
DELETE FROM coding_guidelines WHERE category = 'mira_usage';

-- Project Setup (HIGHEST PRIORITY - do first)
INSERT INTO coding_guidelines (content, category, project_path, priority, created_at, updated_at) VALUES
('PROJECT_SETUP: At session start, FIRST call set_project(project_path="/absolute/path/to/project") to enable project-scoped data', 'mira_usage', NULL, 200, strftime('%s', 'now'), strftime('%s', 'now')),
('PROJECT_SETUP: After set_project(), call get_guidelines(category="mira_usage") to load these instructions', 'mira_usage', NULL, 199, strftime('%s', 'now'), strftime('%s', 'now')),
('PROJECT_SETUP: Use get_project() to check which project is currently active', 'mira_usage', NULL, 198, strftime('%s', 'now'), strftime('%s', 'now'));

-- Memory Tools
INSERT INTO coding_guidelines (content, category, project_path, priority, created_at, updated_at) VALUES
('REMEMBER: When user states a preference ("I prefer...", "always use...", "we do X here"), use remember() to store it with fact_type="preference"', 'mira_usage', NULL, 100, strftime('%s', 'now'), strftime('%s', 'now')),
('REMEMBER: When user makes a decision ("let''s go with...", "we decided..."), use remember() with fact_type="decision"', 'mira_usage', NULL, 100, strftime('%s', 'now'), strftime('%s', 'now')),
('REMEMBER: When user corrects you or says "don''t do X", use remember() to store the correction', 'mira_usage', NULL, 100, strftime('%s', 'now'), strftime('%s', 'now')),
('RECALL: When user asks about past work ("what did we...", "how did I..."), use recall() to find relevant memories', 'mira_usage', NULL, 100, strftime('%s', 'now'), strftime('%s', 'now')),
('RECALL: At start of session, use recall() to get context about past work on this project', 'mira_usage', NULL, 100, strftime('%s', 'now'), strftime('%s', 'now')),
('RECALL: Before asking user a question, use recall() to check if they told you before', 'mira_usage', NULL, 99, strftime('%s', 'now'), strftime('%s', 'now'));

-- Code Intelligence
INSERT INTO coding_guidelines (content, category, project_path, priority, created_at, updated_at) VALUES
('CODE_INTEL: Before modifying a file, use get_related_files() to find co-change patterns', 'mira_usage', NULL, 90, strftime('%s', 'now'), strftime('%s', 'now')),
('CODE_INTEL: To understand a function, use get_call_graph() to see callers and callees', 'mira_usage', NULL, 90, strftime('%s', 'now'), strftime('%s', 'now')),
('CODE_INTEL: To find code by description, use semantic_code_search() for natural language search', 'mira_usage', NULL, 90, strftime('%s', 'now'), strftime('%s', 'now')),
('CODE_INTEL: To understand file structure, use get_symbols() to list functions/classes', 'mira_usage', NULL, 90, strftime('%s', 'now'), strftime('%s', 'now'));

-- Git Intelligence
INSERT INTO coding_guidelines (content, category, project_path, priority, created_at, updated_at) VALUES
('GIT_INTEL: Before changing a file, use find_cochange_patterns() to see what else usually changes with it', 'mira_usage', NULL, 85, strftime('%s', 'now'), strftime('%s', 'now')),
('GIT_INTEL: When encountering an error, use find_similar_fixes() to see if it has been fixed before', 'mira_usage', NULL, 85, strftime('%s', 'now'), strftime('%s', 'now')),
('GIT_INTEL: After fixing a tricky bug, use record_error_fix() so future sessions can learn from it', 'mira_usage', NULL, 85, strftime('%s', 'now'), strftime('%s', 'now'));

-- Session Management
INSERT INTO coding_guidelines (content, category, project_path, priority, created_at, updated_at) VALUES
('SESSION: At end of significant sessions, use store_session() with a summary and topics', 'mira_usage', NULL, 80, strftime('%s', 'now'), strftime('%s', 'now')),
('SESSION: When user asks about past sessions, use search_sessions() to find relevant work', 'mira_usage', NULL, 80, strftime('%s', 'now'), strftime('%s', 'now')),
('SESSION: For architectural decisions, use store_decision() with key, decision, and context', 'mira_usage', NULL, 80, strftime('%s', 'now'), strftime('%s', 'now'));

-- Project Guidelines
INSERT INTO coding_guidelines (content, category, project_path, priority, created_at, updated_at) VALUES
('GUIDELINES: When user mentions a coding convention, use add_guideline() to record it', 'mira_usage', NULL, 75, strftime('%s', 'now'), strftime('%s', 'now')),
('GUIDELINES: Before writing code, use get_guidelines() to check project conventions', 'mira_usage', NULL, 75, strftime('%s', 'now'), strftime('%s', 'now'));

-- Best Practices
INSERT INTO coding_guidelines (content, category, project_path, priority, created_at, updated_at) VALUES
('BEST_PRACTICE: Be specific in memories - "Use snake_case for variables" is better than "naming convention discussed"', 'mira_usage', NULL, 70, strftime('%s', 'now'), strftime('%s', 'now')),
('BEST_PRACTICE: Use categories when storing memories - helps with filtering (architecture, style, preference, decision)', 'mira_usage', NULL, 70, strftime('%s', 'now'), strftime('%s', 'now')),
('BEST_PRACTICE: Record decisions with context - include WHY not just WHAT', 'mira_usage', NULL, 70, strftime('%s', 'now'), strftime('%s', 'now')),
('BEST_PRACTICE: Mira tools can be called in parallel - fire multiple independent calls at once for efficiency', 'mira_usage', NULL, 70, strftime('%s', 'now'), strftime('%s', 'now'));
