# Changelog

All notable changes to Mira will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-01-27

### Added
- Task-type-aware embeddings for better semantic search quality
- Background worker split into fast/slow lanes for better responsiveness
- Proactive interventions from pondering insights
- Goal and milestone tracking across sessions
- Evidence-based memory system (candidate → confirmed promotion)
- Cross-project intelligence sharing (opt-in)
- Expert consultation with codebase access (architect, code_reviewer, security, etc.)
- Automatic documentation gap detection
- LLM usage and cost analytics

### Changed
- Default embedding dimensions changed from 768 to 1536
- Consolidated MCP tools from 40+ to ~22 action-based tools
- Improved semantic code search with CODE_RETRIEVAL_QUERY task type

### Removed
- GLM/ZhipuAI provider (simplified to DeepSeek + Gemini only)
- Legacy Database struct (fully migrated to DatabasePool)

### Fixed
- vec_code table no longer dropped on startup
- Memory embedding backfill now works correctly

## [0.1.0] - 2025-12-31

### Added
- Initial release
- Persistent memory system with semantic search
- Code intelligence (symbols, call graphs, semantic search)
- Background indexing and embedding generation
- Expert consultation via DeepSeek Reasoner
- Session history and context tracking
- MCP server integration with Claude Code
