#!/usr/bin/env python3
"""
Mira Permission Hook for Claude Code

This hook queries Mira's permission_rules table directly (via SQLite)
to auto-approve known tool operations across sessions.

When a permission is found: returns {"behavior": "allow"}
When no permission: does nothing (passes through to user prompt)
"""

import json
import os
import re
import sqlite3
import sys
import fnmatch
from pathlib import Path

# Database path - same as Mira uses
DB_PATH = Path.home() / "Mira" / "data" / "mira.db"


def glob_match(value: str, pattern: str) -> bool:
    """Simple glob matching using fnmatch."""
    return fnmatch.fnmatch(value, pattern)


def check_permission(tool_name: str, tool_input: dict, project_path: str | None) -> dict:
    """Check if a permission rule exists that allows this operation."""

    if not DB_PATH.exists():
        return {"decision": "ask_user", "reason": "database_not_found"}

    # Determine the input field and value based on tool type
    input_field = None
    input_value = None

    if tool_name == "Bash":
        input_field = "command"
        input_value = tool_input.get("command", "")
    elif tool_name in ("Edit", "Write", "Read"):
        input_field = "file_path"
        input_value = tool_input.get("file_path", "")
    elif tool_name == "Glob":
        input_field = "pattern"
        input_value = tool_input.get("pattern", "")
    elif tool_name == "Grep":
        input_field = "pattern"
        input_value = tool_input.get("pattern", "")

    try:
        conn = sqlite3.connect(str(DB_PATH), timeout=2.0)
        conn.row_factory = sqlite3.Row
        cursor = conn.cursor()

        # Get project_id if we have a project path
        project_id = None
        if project_path:
            cursor.execute(
                "SELECT id FROM projects WHERE path = ?",
                (project_path,)
            )
            row = cursor.fetchone()
            if row:
                project_id = row["id"]

        # Query for matching rules
        # Global rules (project_id IS NULL) always apply
        # Project rules only apply if project_id matches
        cursor.execute("""
            SELECT id, scope, project_id, input_field, input_pattern, match_type
            FROM permission_rules
            WHERE tool_name = ?
              AND (
                (scope = 'global' AND project_id IS NULL) OR
                (scope = 'project' AND project_id = ?)
              )
            ORDER BY
                CASE WHEN project_id IS NOT NULL THEN 0 ELSE 1 END,
                CASE match_type
                    WHEN 'exact' THEN 0
                    WHEN 'prefix' THEN 1
                    WHEN 'glob' THEN 2
                    ELSE 3
                END
        """, (tool_name, project_id))

        rules = cursor.fetchall()

        for rule in rules:
            rule_field = rule["input_field"]
            pattern = rule["input_pattern"]
            match_type = rule["match_type"]

            # If rule has no pattern, it matches all operations for this tool
            if not pattern:
                update_usage(conn, rule["id"])
                conn.close()
                return {
                    "decision": "allow",
                    "rule_id": rule["id"],
                    "match_type": "tool_only"
                }

            # Check if the input matches the pattern
            if rule_field == input_field and input_value:
                matches = False

                if match_type == "exact":
                    matches = (input_value == pattern)
                elif match_type == "prefix":
                    matches = input_value.startswith(pattern)
                elif match_type == "glob":
                    matches = glob_match(input_value, pattern)

                if matches:
                    update_usage(conn, rule["id"])
                    conn.close()
                    return {
                        "decision": "allow",
                        "rule_id": rule["id"],
                        "match_type": match_type,
                        "pattern": pattern
                    }

        conn.close()
        return {"decision": "ask_user", "reason": "no_matching_rule"}

    except sqlite3.Error as e:
        return {"decision": "ask_user", "reason": f"db_error: {e}"}
    except Exception as e:
        return {"decision": "ask_user", "reason": f"error: {e}"}


def update_usage(conn: sqlite3.Connection, rule_id: str):
    """Update usage stats for a rule."""
    try:
        import time
        now = int(time.time())
        cursor = conn.cursor()
        cursor.execute(
            "UPDATE permission_rules SET times_used = times_used + 1, last_used_at = ? WHERE id = ?",
            (now, rule_id)
        )
        conn.commit()
    except Exception:
        pass  # Non-critical, don't fail on stats update


def main():
    # Read hook input from stdin
    try:
        raw_input = sys.stdin.read()
        hook_input = json.loads(raw_input)
    except json.JSONDecodeError as e:
        # Invalid JSON - let Claude Code handle it normally
        sys.exit(0)

    # Only process PermissionRequest events
    hook_event = hook_input.get("hook_event_name", "")
    if hook_event != "PermissionRequest":
        sys.exit(0)

    tool_name = hook_input.get("tool_name", "")
    tool_input = hook_input.get("tool_input", {})
    cwd = hook_input.get("cwd", "")

    # Check for matching permission rule
    result = check_permission(tool_name, tool_input, cwd)
    decision = result.get("decision", "ask_user")

    # If we found a matching allow rule, auto-approve
    if decision == "allow":
        response = {
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": {
                    "behavior": "allow"
                }
            }
        }
        print(json.dumps(response))
        sys.exit(0)

    # No matching rule - exit silently to let Claude Code prompt the user
    # (Don't output anything - this lets the normal permission flow continue)
    sys.exit(0)


if __name__ == "__main__":
    main()
