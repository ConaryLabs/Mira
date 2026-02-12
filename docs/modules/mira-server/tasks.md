<!-- docs/modules/mira-server/tasks.md -->
# tasks

Reader for Claude Code's native task files. Provides filesystem-based access to task lists without any database dependency.

## Overview

Claude Code stores tasks as JSON files in `~/.claude/tasks/{list-id}/`. This module reads and parses those files, providing a view into the current session's task list. It uses multiple strategies to find the active task list: environment variable, captured session hook data, or most recently modified directory.

## Key Types

- `NativeTask` - A task as stored by Claude Code, with id, subject, description, status, blocks, and blockedBy fields

## Key Functions

- `find_current_task_list()` - Locate the active task list directory (env var -> hook capture -> most recent)
- `read_task_list()` - Read and parse all task JSON files from a directory
- `get_pending_tasks()` - Filter to pending/in_progress tasks only
- `count_tasks()` - Count completed vs remaining tasks
- `task_list_id()` - Extract the list ID from a directory path

## Architecture Notes

This is a pure filesystem reader with no database interaction. Tasks are sorted by numeric ID for consistent ordering. Malformed JSON files are logged and skipped rather than causing errors. Used by the `UserPromptSubmit` hook to inject pending tasks into context and by the `Stop` hook to snapshot task state.
