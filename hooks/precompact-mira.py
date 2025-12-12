#!/usr/bin/env python3
"""
Mira PreCompact Hook for Claude Code

This hook fires before Claude Code compacts/summarizes the conversation.
It extracts key context from the transcript and saves it to Mira's database,
preserving information that would otherwise be lost during compaction.

Saves:
- Full session summary
- Key decisions made
- Files modified
- Topics discussed
- Tool calls made
"""

import json
import os
import re
import sqlite3
import sys
import hashlib
import urllib.request
import urllib.error
from datetime import datetime
from pathlib import Path

# Database path - same as Mira uses
DB_PATH = Path.home() / "Mira" / "data" / "mira.db"

# Mira HTTP API config
MIRA_URL = os.environ.get("MIRA_URL", "http://localhost:3000/mcp")
MIRA_AUTH_TOKEN = os.environ.get("MIRA_AUTH_TOKEN", "63c7663fe0dbdfcd2bbf6c33a0a9b4b9")


def extract_transcript_context(transcript_path: str) -> dict:
    """Extract key context from the transcript JSONL file."""

    context = {
        "messages": [],
        "files_modified": set(),
        "files_read": set(),
        "decisions": [],
        "topics": set(),
        "tool_calls": [],
        "errors_encountered": [],
        "user_requests": [],
    }

    if not os.path.exists(transcript_path):
        return context

    try:
        with open(transcript_path, 'r') as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    entry = json.loads(line)
                    process_transcript_entry(entry, context)
                except json.JSONDecodeError:
                    continue
    except Exception as e:
        context["errors_encountered"].append(f"Failed to read transcript: {e}")

    # Convert sets to lists for JSON serialization
    context["files_modified"] = list(context["files_modified"])
    context["files_read"] = list(context["files_read"])
    context["topics"] = list(context["topics"])

    return context


def process_transcript_entry(entry: dict, context: dict):
    """Process a single transcript entry."""

    msg_type = entry.get("type", "")

    if msg_type == "user":
        # User messages - capture as requests/topics
        content = entry.get("message", {}).get("content", "")
        if isinstance(content, str) and content.strip():
            context["user_requests"].append(content[:500])  # Truncate long messages
            # Extract potential topics from user messages
            extract_topics(content, context["topics"])

    elif msg_type == "assistant":
        # Assistant messages - look for decisions and explanations
        message = entry.get("message", {})
        content = message.get("content", [])

        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict):
                    # Tool use blocks
                    if block.get("type") == "tool_use":
                        tool_name = block.get("name", "")
                        tool_input = block.get("input", {})

                        context["tool_calls"].append({
                            "tool": tool_name,
                            "input_summary": summarize_tool_input(tool_name, tool_input)
                        })

                        # Track file operations
                        if tool_name in ("Edit", "Write"):
                            file_path = tool_input.get("file_path", "")
                            if file_path:
                                context["files_modified"].add(file_path)
                        elif tool_name == "Read":
                            file_path = tool_input.get("file_path", "")
                            if file_path:
                                context["files_read"].add(file_path)

                    # Text blocks - look for decisions
                    elif block.get("type") == "text":
                        text = block.get("text", "")
                        if text:
                            extract_decisions(text, context["decisions"])
                            extract_topics(text, context["topics"])

    elif msg_type == "tool_result":
        # Check for errors in tool results
        result = entry.get("result", {})
        if isinstance(result, dict):
            is_error = result.get("is_error", False)
            if is_error:
                content = result.get("content", "")
                if content:
                    context["errors_encountered"].append(content[:200])


def summarize_tool_input(tool_name: str, tool_input: dict) -> str:
    """Create a brief summary of tool input."""
    if tool_name == "Bash":
        cmd = tool_input.get("command", "")
        return cmd[:100] if cmd else ""
    elif tool_name in ("Edit", "Write", "Read"):
        return tool_input.get("file_path", "")[:100]
    elif tool_name == "Grep":
        pattern = tool_input.get("pattern", "")
        return f"pattern: {pattern[:50]}"
    elif tool_name == "Glob":
        pattern = tool_input.get("pattern", "")
        return f"glob: {pattern[:50]}"
    else:
        # MCP tools or others
        return str(tool_input)[:100]


def extract_decisions(text: str, decisions: list):
    """Extract decision-like statements from text."""
    # Look for common decision patterns
    decision_patterns = [
        r"(?:I(?:'ll| will)|Let's|We should|Going to|I'm going to|I decided to|The approach is to)\s+([^.!?\n]{10,100})",
        r"(?:Using|Switching to|Implementing|Creating|Adding)\s+([^.!?\n]{10,80})",
    ]

    for pattern in decision_patterns:
        matches = re.findall(pattern, text, re.IGNORECASE)
        for match in matches[:3]:  # Limit to 3 per pattern
            if match.strip() and match not in decisions:
                decisions.append(match.strip()[:150])

    # Limit total decisions
    while len(decisions) > 20:
        decisions.pop(0)


def extract_topics(text: str, topics: set):
    """Extract topic keywords from text."""
    # Common technical topics to look for
    topic_keywords = [
        "api", "database", "authentication", "auth", "testing", "test",
        "deployment", "docker", "kubernetes", "git", "ci/cd", "pipeline",
        "frontend", "backend", "server", "client", "ui", "ux",
        "bug", "fix", "feature", "refactor", "optimization", "performance",
        "security", "encryption", "migration", "config", "configuration",
        "rust", "python", "typescript", "javascript", "sql", "json",
        "mcp", "embeddings", "qdrant", "semantic", "indexer", "daemon",
    ]

    text_lower = text.lower()
    for topic in topic_keywords:
        if topic in text_lower:
            topics.add(topic)

    # Limit topics
    while len(topics) > 30:
        topics.pop()


def generate_summary(context: dict, trigger: str) -> str:
    """Generate a summary of the session."""

    parts = []

    # Trigger info
    parts.append(f"Compaction triggered: {trigger}")

    # Files summary
    if context["files_modified"]:
        parts.append(f"\nFiles modified ({len(context['files_modified'])}):")
        for f in context["files_modified"][:10]:
            # Show relative paths where possible
            display = f.split("/Mira/")[-1] if "/Mira/" in f else f.split("/")[-1]
            parts.append(f"  - {display}")
        if len(context["files_modified"]) > 10:
            parts.append(f"  ... and {len(context['files_modified']) - 10} more")

    # User requests
    if context["user_requests"]:
        parts.append(f"\nUser requests ({len(context['user_requests'])}):")
        for req in context["user_requests"][:5]:
            # First line only, truncated
            first_line = req.split('\n')[0][:100]
            parts.append(f"  - {first_line}")

    # Key decisions
    if context["decisions"]:
        parts.append(f"\nKey decisions/actions:")
        for dec in context["decisions"][:10]:
            parts.append(f"  - {dec}")

    # Topics
    if context["topics"]:
        parts.append(f"\nTopics: {', '.join(sorted(context['topics']))}")

    # Tool usage stats
    if context["tool_calls"]:
        tool_counts = {}
        for tc in context["tool_calls"]:
            tool = tc["tool"]
            tool_counts[tool] = tool_counts.get(tool, 0) + 1
        parts.append(f"\nTools used: {', '.join(f'{k}({v})' for k, v in sorted(tool_counts.items()))}")

    # Errors
    if context["errors_encountered"]:
        parts.append(f"\nErrors encountered: {len(context['errors_encountered'])}")

    return "\n".join(parts)


def call_mira_tool(session_id: str, tool_name: str, arguments: dict) -> dict:
    """Call a Mira MCP tool via HTTP API."""
    try:
        # Initialize session first
        init_payload = json.dumps({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "precompact-hook", "version": "1.0"}
            }
        }).encode()

        req = urllib.request.Request(
            MIRA_URL,
            data=init_payload,
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json, text/event-stream",
                "Authorization": f"Bearer {MIRA_AUTH_TOKEN}"
            }
        )

        with urllib.request.urlopen(req, timeout=5) as resp:
            # Get session ID from header
            mcp_session = resp.getheader("mcp-session-id", session_id)
            resp.read()  # Consume response

        # Send initialized notification
        notif_payload = json.dumps({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }).encode()

        req = urllib.request.Request(
            MIRA_URL,
            data=notif_payload,
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json, text/event-stream",
                "Authorization": f"Bearer {MIRA_AUTH_TOKEN}",
                "Mcp-Session-Id": mcp_session
            }
        )
        urllib.request.urlopen(req, timeout=5).read()

        # Now call the actual tool
        tool_payload = json.dumps({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        }).encode()

        req = urllib.request.Request(
            MIRA_URL,
            data=tool_payload,
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json, text/event-stream",
                "Authorization": f"Bearer {MIRA_AUTH_TOKEN}",
                "Mcp-Session-Id": mcp_session
            }
        )

        with urllib.request.urlopen(req, timeout=10) as resp:
            data = resp.read().decode()
            # Parse SSE response (data: {...})
            for line in data.split('\n'):
                if line.startswith('data: '):
                    return json.loads(line[6:])

        return {"error": "no_response"}

    except urllib.error.URLError as e:
        return {"error": f"url_error: {e}"}
    except Exception as e:
        return {"error": f"error: {e}"}


def save_to_mira(session_id: str, trigger: str, context: dict, summary: str):
    """Save the compaction context to Mira via HTTP API (with embeddings)."""

    timestamp = int(datetime.now().timestamp())
    snapshot_id = hashlib.md5(f"{session_id}-{timestamp}".encode()).hexdigest()[:16]
    results = []

    # Try HTTP API first for semantic search support
    http_available = False
    try:
        req = urllib.request.Request(
            MIRA_URL.replace("/mcp", "/health") if "/mcp" in MIRA_URL else f"{MIRA_URL}/health",
            headers={"Authorization": f"Bearer {MIRA_AUTH_TOKEN}"}
        )
        # Just check if server responds (even 401 means it's up)
        urllib.request.urlopen(MIRA_URL, timeout=2)
        http_available = True
    except:
        pass

    if http_available:
        # Use HTTP API - this generates embeddings for semantic search

        # Store session summary
        full_summary = f"[Pre-Compaction Save - {trigger}]\n{summary}"
        result = call_mira_tool(session_id, "store_session", {
            "summary": full_summary,
            "session_id": session_id,
            "topics": list(context.get("topics", []))[:10]
        })
        results.append(("store_session", result))

        # Store files modified as a memory
        if context["files_modified"]:
            files_content = f"Files modified before compaction ({trigger}): {', '.join(context['files_modified'][:20])}"
            result = call_mira_tool(session_id, "remember", {
                "content": files_content,
                "fact_type": "context",
                "category": "compaction",
                "key": f"compaction-files-{snapshot_id}"
            })
            results.append(("remember_files", result))

        # Store decisions
        if context["decisions"]:
            decisions_content = f"Decisions made before compaction ({trigger}):\n" + "\n".join(f"- {d}" for d in context["decisions"][:15])
            result = call_mira_tool(session_id, "remember", {
                "content": decisions_content,
                "fact_type": "decision",
                "category": "compaction",
                "key": f"compaction-decisions-{snapshot_id}"
            })
            results.append(("remember_decisions", result))

        # Store user requests
        if context["user_requests"]:
            requests_content = f"User requests before compaction ({trigger}):\n" + "\n".join(f"- {r[:150]}" for r in context["user_requests"][:10])
            result = call_mira_tool(session_id, "remember", {
                "content": requests_content,
                "fact_type": "context",
                "category": "compaction",
                "key": f"compaction-requests-{snapshot_id}"
            })
            results.append(("remember_requests", result))

        return {"success": True, "snapshot_id": snapshot_id, "method": "http_api", "results": results}

    # Fallback to direct SQL (no embeddings, but still saves data)
    if not DB_PATH.exists():
        return {"error": "database_not_found"}

    try:
        conn = sqlite3.connect(str(DB_PATH), timeout=5.0)
        cursor = conn.cursor()

        # Get project ID (default to Mira project)
        cursor.execute(
            "SELECT id FROM projects WHERE path = ?",
            ("/home/peter/Mira",)
        )
        row = cursor.fetchone()
        project_id = row[0] if row else None

        # Store the session summary in memory_entries
        full_summary = f"[Pre-Compaction Save - {trigger}]\n{summary}"
        cursor.execute("""
            INSERT INTO memory_entries (id, session_id, role, content, created_at, project_id)
            VALUES (?, ?, 'session_summary', ?, ?, ?)
        """, (snapshot_id, session_id, full_summary, timestamp, project_id))

        # Store key facts in memory_facts (without embeddings)
        if context["files_modified"]:
            files_content = f"Files modified before compaction ({trigger}): {', '.join(context['files_modified'][:20])}"
            files_key = f"compaction-files-{snapshot_id}"
            cursor.execute("""
                INSERT OR REPLACE INTO memory_facts (id, fact_type, key, value, category, source, created_at, updated_at, project_id)
                VALUES (?, 'context', ?, ?, 'compaction', 'precompact_hook', ?, ?, ?)
            """, (
                hashlib.md5(files_key.encode()).hexdigest()[:16],
                files_key,
                files_content,
                timestamp,
                timestamp,
                project_id
            ))

        if context["decisions"]:
            decisions_content = "Decisions made before compaction:\n" + "\n".join(f"- {d}" for d in context["decisions"][:15])
            decisions_key = f"compaction-decisions-{snapshot_id}"
            cursor.execute("""
                INSERT OR REPLACE INTO memory_facts (id, fact_type, key, value, category, source, created_at, updated_at, project_id)
                VALUES (?, 'decision', ?, ?, 'compaction', 'precompact_hook', ?, ?, ?)
            """, (
                hashlib.md5(decisions_key.encode()).hexdigest()[:16],
                decisions_key,
                decisions_content,
                timestamp,
                timestamp,
                project_id
            ))

        if context["user_requests"]:
            requests_content = "User requests before compaction:\n" + "\n".join(f"- {r[:150]}" for r in context["user_requests"][:10])
            requests_key = f"compaction-requests-{snapshot_id}"
            cursor.execute("""
                INSERT OR REPLACE INTO memory_facts (id, fact_type, key, value, category, source, created_at, updated_at, project_id)
                VALUES (?, 'context', ?, ?, 'compaction', 'precompact_hook', ?, ?, ?)
            """, (
                hashlib.md5(requests_key.encode()).hexdigest()[:16],
                requests_key,
                requests_content,
                timestamp,
                timestamp,
                project_id
            ))

        conn.commit()
        conn.close()

        return {"success": True, "snapshot_id": snapshot_id, "method": "direct_sql"}

    except sqlite3.Error as e:
        return {"error": f"db_error: {e}"}
    except Exception as e:
        return {"error": f"error: {e}"}


def main():
    # Read hook input from stdin
    try:
        raw_input = sys.stdin.read()
        hook_input = json.loads(raw_input)
    except json.JSONDecodeError as e:
        # Invalid JSON - exit silently
        sys.exit(0)

    # Only process PreCompact events
    hook_event = hook_input.get("hook_event_name", "")
    if hook_event != "PreCompact":
        sys.exit(0)

    session_id = hook_input.get("session_id", "unknown")
    transcript_path = hook_input.get("transcript_path", "")
    trigger = hook_input.get("trigger", "unknown")  # "auto" or "manual"

    # Extract context from transcript
    context = extract_transcript_context(transcript_path)

    # Generate summary
    summary = generate_summary(context, trigger)

    # Save to Mira
    result = save_to_mira(session_id, trigger, context, summary)

    # Log result (will appear in Claude Code logs)
    if result.get("success"):
        # Output status message
        response = {
            "hookSpecificOutput": {
                "hookEventName": "PreCompact",
                "status": f"Saved pre-compaction context to Mira (snapshot: {result.get('snapshot_id', 'unknown')})"
            }
        }
        print(json.dumps(response))

    sys.exit(0)


if __name__ == "__main__":
    main()
