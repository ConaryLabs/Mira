#!/bin/bash
# Mira Power Suit - Easy Setup Script
# Usage: curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash

set -e

MIRA_DIR="${MIRA_DIR:-$HOME/.mira}"
CLAUDE_DIR="$HOME/.claude"

echo "Installing Mira Power Suit for Claude Code"
echo ""

# Check for Docker
if ! command -v docker &> /dev/null; then
    echo "Error: Docker is required but not installed."
    echo "Please install Docker: https://docs.docker.com/get-docker/"
    exit 1
fi

# Check for docker compose
if ! docker compose version &> /dev/null; then
    echo "Error: Docker Compose is required but not installed."
    echo "Please install Docker Compose: https://docs.docker.com/compose/install/"
    exit 1
fi

# Create Mira directory
mkdir -p "$MIRA_DIR"
cd "$MIRA_DIR"

# Download source if not present
if [ ! -f "Dockerfile" ]; then
    echo "Downloading Mira source..."
    curl -fsSL "https://github.com/ConaryLabs/Mira/archive/main.tar.gz" | tar -xz --strip-components=1 2>/dev/null || {
        echo "Could not download from GitHub. Please clone manually:"
        echo "  git clone https://github.com/ConaryLabs/Mira.git ~/.mira"
        echo "  cd ~/.mira && ./install.sh"
        exit 1
    }
fi

echo "Building Mira Docker image (this may take a few minutes)..."
docker compose build mira

# Ask about semantic search
echo ""
echo "Semantic search enables finding memories and code by meaning (not just keywords)."
echo "It requires Qdrant (vector database) and a Google Gemini API key (free tier available)."
echo ""
echo "Enable semantic search? [Y/n]:"
read -r ENABLE_SEMANTIC

ENABLE_SEMANTIC=${ENABLE_SEMANTIC:-Y}
if [[ "$ENABLE_SEMANTIC" =~ ^[Yy] ]]; then
    USE_QDRANT=true

    echo "Starting Qdrant..."
    docker compose up -d qdrant

    # Wait for Qdrant to be ready
    echo "Waiting for Qdrant to start..."
    for i in {1..30}; do
        if curl -s http://localhost:6334/healthz > /dev/null 2>&1; then
            break
        fi
        sleep 1
    done

    echo ""
    echo "Enter your Gemini API key (get one free at https://aistudio.google.com/apikey)"
    echo "Or press Enter to add later:"
    read -r GEMINI_KEY

    if [ -n "$GEMINI_KEY" ]; then
        echo "GEMINI_API_KEY=$GEMINI_KEY" > "$MIRA_DIR/.env"
        chmod 600 "$MIRA_DIR/.env"
        echo "API key saved to ~/.mira/.env"
        SEMANTIC_STATUS="enabled"
    else
        echo "No key provided. Add GEMINI_API_KEY to ~/.mira/.env later."
        SEMANTIC_STATUS="Qdrant running, needs API key"
    fi
else
    USE_QDRANT=false
    SEMANTIC_STATUS="disabled"
    echo "Skipped. Mira will use text-based search only."
fi

# Initialize database
echo ""
echo "Initializing database..."
docker compose run --rm -T mira sh -c "cat migrations/*.sql | sqlite3 /app/data/mira.db && sqlite3 /app/data/mira.db < seed_mira_guidelines.sql" 2>/dev/null

echo "Database initialized"

# Create wrapper script for Claude Code
if [ "$USE_QDRANT" = true ]; then
    # With Qdrant
    cat > "$MIRA_DIR/mira" << 'WRAPPER'
#!/bin/bash
# Mira wrapper script - runs the MCP server in Docker with Qdrant
cd "$HOME/.mira"
if [ -f .env ]; then
    set -a
    source .env
    set +a
fi
exec docker compose run --rm -T \
    -e GEMINI_API_KEY="${GEMINI_API_KEY:-}" \
    mira
WRAPPER
else
    # Without Qdrant - just run Mira standalone
    cat > "$MIRA_DIR/mira" << 'WRAPPER'
#!/bin/bash
# Mira wrapper script - runs the MCP server in Docker (no semantic search)
cd "$HOME/.mira"
exec docker run -i --rm \
    -v "$HOME/.mira/data:/app/data" \
    mira-mira
WRAPPER
fi
chmod +x "$MIRA_DIR/mira"

# Install Claude Code hooks
echo ""
echo "Installing Claude Code hooks..."
mkdir -p "$CLAUDE_DIR/hooks"

# PreCompact hook - saves context before compaction
cat > "$CLAUDE_DIR/hooks/precompact-mira.py" << 'HOOK'
#!/usr/bin/env python3
"""
Mira PreCompact Hook - saves context before Claude Code compacts conversation.
"""

import json
import os
import re
import sqlite3
import sys
import hashlib
from datetime import datetime
from pathlib import Path

DB_PATH = Path.home() / ".mira" / "data" / "mira.db"

def extract_transcript_context(transcript_path):
    context = {
        "messages": [], "files_modified": set(), "files_read": set(),
        "decisions": [], "topics": set(), "tool_calls": [],
        "errors_encountered": [], "user_requests": [],
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
                    process_entry(entry, context)
                except json.JSONDecodeError:
                    continue
    except Exception:
        pass
    context["files_modified"] = list(context["files_modified"])
    context["files_read"] = list(context["files_read"])
    context["topics"] = list(context["topics"])
    return context

def process_entry(entry, context):
    msg_type = entry.get("type", "")
    if msg_type == "user":
        content = entry.get("message", {}).get("content", "")
        if isinstance(content, str) and content.strip():
            context["user_requests"].append(content[:500])
    elif msg_type == "assistant":
        message = entry.get("message", {})
        content = message.get("content", [])
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict):
                    if block.get("type") == "tool_use":
                        tool_name = block.get("name", "")
                        tool_input = block.get("input", {})
                        context["tool_calls"].append({"tool": tool_name})
                        if tool_name in ("Edit", "Write"):
                            fp = tool_input.get("file_path", "")
                            if fp:
                                context["files_modified"].add(fp)
                        elif tool_name == "Read":
                            fp = tool_input.get("file_path", "")
                            if fp:
                                context["files_read"].add(fp)
                    elif block.get("type") == "text":
                        text = block.get("text", "")
                        if text:
                            for m in re.findall(r"(?:I'll|Let's|Going to|Using|Creating)\s+([^.!?\n]{10,80})", text, re.I):
                                if m not in context["decisions"]:
                                    context["decisions"].append(m[:150])

def generate_summary(context, trigger):
    parts = [f"Compaction: {trigger}"]
    if context["files_modified"]:
        parts.append(f"Files modified: {', '.join(f.split('/')[-1] for f in list(context['files_modified'])[:10])}")
    if context["user_requests"]:
        parts.append(f"Requests: {len(context['user_requests'])}")
    if context["decisions"]:
        parts.append(f"Decisions: {len(context['decisions'])}")
    return " | ".join(parts)

def save_to_mira(session_id, trigger, context, summary):
    if not DB_PATH.exists():
        return
    try:
        conn = sqlite3.connect(str(DB_PATH), timeout=5.0)
        cursor = conn.cursor()
        timestamp = int(datetime.now().timestamp())
        cursor.execute("SELECT id FROM projects ORDER BY id LIMIT 1")
        row = cursor.fetchone()
        project_id = row[0] if row else None
        snapshot_id = hashlib.md5(f"{session_id}-{timestamp}".encode()).hexdigest()[:16]
        full_summary = f"[Pre-Compaction - {trigger}]\n{summary}"
        cursor.execute("""
            INSERT INTO memory_entries (id, session_id, role, content, created_at, project_id)
            VALUES (?, ?, 'session_summary', ?, ?, ?)
        """, (snapshot_id, session_id, full_summary, timestamp, project_id))
        if context["files_modified"]:
            files_key = f"compaction-files-{snapshot_id}"
            cursor.execute("""
                INSERT OR REPLACE INTO memory_facts (id, fact_type, key, value, category, source, created_at, updated_at, project_id)
                VALUES (?, 'context', ?, ?, 'compaction', 'precompact_hook', ?, ?, ?)
            """, (hashlib.md5(files_key.encode()).hexdigest()[:16], files_key,
                  f"Files modified: {', '.join(context['files_modified'][:20])}", timestamp, timestamp, project_id))
        if context["decisions"]:
            dec_key = f"compaction-decisions-{snapshot_id}"
            cursor.execute("""
                INSERT OR REPLACE INTO memory_facts (id, fact_type, key, value, category, source, created_at, updated_at, project_id)
                VALUES (?, 'decision', ?, ?, 'compaction', 'precompact_hook', ?, ?, ?)
            """, (hashlib.md5(dec_key.encode()).hexdigest()[:16], dec_key,
                  "Decisions:\n" + "\n".join(f"- {d}" for d in context["decisions"][:15]), timestamp, timestamp, project_id))
        conn.commit()
        conn.close()
        return snapshot_id
    except Exception:
        return None

def main():
    try:
        hook_input = json.loads(sys.stdin.read())
    except json.JSONDecodeError:
        sys.exit(0)
    if hook_input.get("hook_event_name") != "PreCompact":
        sys.exit(0)
    session_id = hook_input.get("session_id", "unknown")
    transcript_path = hook_input.get("transcript_path", "")
    trigger = hook_input.get("trigger", "unknown")
    context = extract_transcript_context(transcript_path)
    summary = generate_summary(context, trigger)
    snapshot_id = save_to_mira(session_id, trigger, context, summary)
    if snapshot_id:
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreCompact", "status": f"Saved to Mira ({snapshot_id})"}}))
    sys.exit(0)

if __name__ == "__main__":
    main()
HOOK
chmod +x "$CLAUDE_DIR/hooks/precompact-mira.py"

# Permission auto-approval hook
cat > "$CLAUDE_DIR/hooks/mira_permission_hook.py" << 'HOOK'
#!/usr/bin/env python3
"""
Mira Permission Hook - auto-approves saved permissions from Mira's database.
"""

import json
import sqlite3
import sys
import fnmatch
from pathlib import Path

DB_PATH = Path.home() / ".mira" / "data" / "mira.db"

def check_permission(tool_name, tool_input, project_path):
    if not DB_PATH.exists():
        return {"decision": "ask_user"}
    input_field, input_value = None, None
    if tool_name == "Bash":
        input_field, input_value = "command", tool_input.get("command", "")
    elif tool_name in ("Edit", "Write", "Read"):
        input_field, input_value = "file_path", tool_input.get("file_path", "")
    try:
        conn = sqlite3.connect(str(DB_PATH), timeout=2.0)
        conn.row_factory = sqlite3.Row
        cursor = conn.cursor()
        project_id = None
        if project_path:
            cursor.execute("SELECT id FROM projects WHERE path = ?", (project_path,))
            row = cursor.fetchone()
            if row:
                project_id = row["id"]
        cursor.execute("""
            SELECT id, input_field, input_pattern, match_type FROM permission_rules
            WHERE tool_name = ? AND ((scope = 'global' AND project_id IS NULL) OR (scope = 'project' AND project_id = ?))
        """, (tool_name, project_id))
        for rule in cursor.fetchall():
            pattern = rule["input_pattern"]
            if not pattern:
                conn.close()
                return {"decision": "allow"}
            if rule["input_field"] == input_field and input_value:
                match_type = rule["match_type"]
                if match_type == "exact" and input_value == pattern:
                    conn.close()
                    return {"decision": "allow"}
                elif match_type == "prefix" and input_value.startswith(pattern):
                    conn.close()
                    return {"decision": "allow"}
                elif match_type == "glob" and fnmatch.fnmatch(input_value, pattern):
                    conn.close()
                    return {"decision": "allow"}
        conn.close()
    except Exception:
        pass
    return {"decision": "ask_user"}

def main():
    try:
        hook_input = json.loads(sys.stdin.read())
    except json.JSONDecodeError:
        sys.exit(0)
    if hook_input.get("hook_event_name") != "PermissionRequest":
        sys.exit(0)
    result = check_permission(
        hook_input.get("tool_name", ""),
        hook_input.get("tool_input", {}),
        hook_input.get("cwd", "")
    )
    if result.get("decision") == "allow":
        print(json.dumps({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}}))
    sys.exit(0)

if __name__ == "__main__":
    main()
HOOK
chmod +x "$CLAUDE_DIR/hooks/mira_permission_hook.py"

echo "Hooks installed"

# Configure Claude Code MCP
CLAUDE_MCP_CONFIG="$CLAUDE_DIR/mcp.json"
mkdir -p "$(dirname "$CLAUDE_MCP_CONFIG")"

echo "Configuring Claude Code MCP..."

if [ -f "$CLAUDE_MCP_CONFIG" ]; then
    if grep -q '"mira"' "$CLAUDE_MCP_CONFIG"; then
        echo "  Mira already in MCP config, updating..."
    else
        cp "$CLAUDE_MCP_CONFIG" "$CLAUDE_MCP_CONFIG.bak"
        echo "  Backed up existing config to $CLAUDE_MCP_CONFIG.bak"
    fi
fi

cat > "$CLAUDE_MCP_CONFIG" << EOF
{
  "mcpServers": {
    "mira": {
      "command": "$MIRA_DIR/mira"
    }
  }
}
EOF

# Configure Claude Code hooks
CLAUDE_SETTINGS="$CLAUDE_DIR/settings.json"

echo "Configuring Claude Code hooks..."

if [ -f "$CLAUDE_SETTINGS" ]; then
    cp "$CLAUDE_SETTINGS" "$CLAUDE_SETTINGS.bak"
    echo "  Backed up existing settings to $CLAUDE_SETTINGS.bak"
fi

cat > "$CLAUDE_SETTINGS" << EOF
{
  "hooks": {
    "PermissionRequest": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "python3 $CLAUDE_DIR/hooks/mira_permission_hook.py",
            "timeout": 3000
          }
        ]
      }
    ],
    "PreCompact": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "python3 $CLAUDE_DIR/hooks/precompact-mira.py",
            "timeout": 15000
          }
        ]
      }
    ]
  }
}
EOF

echo "Claude Code configured"

# Done
echo ""
echo "============================================"
echo "Mira Power Suit installed"
echo "============================================"
echo ""
echo "Components:"
echo "  - Mira MCP server (Docker)"
echo "  - Qdrant vector database (Docker, port 6334)"
echo "  - SQLite database (~/.mira/data/mira.db)"
echo "  - Semantic search: $SEMANTIC_STATUS"
echo "  - Hooks: PreCompact (auto-save), PermissionRequest (auto-approve)"
echo ""
echo "Next steps:"
echo ""
echo "1. Restart Claude Code to load Mira"
echo ""
echo "2. Add to your project's CLAUDE.md:"
echo ""
echo "   # CLAUDE.md"
echo "   This project uses Mira for persistent memory."
echo ""
echo "   ## Session Start"
echo "   get_guidelines(category=\"mira_usage\")"
echo "   get_session_context()"
echo ""
echo "Installation: $MIRA_DIR"
echo ""
echo "Commands:"
echo "  Start Qdrant:  cd ~/.mira && docker compose up -d qdrant"
echo "  Stop Qdrant:   cd ~/.mira && docker compose down"
echo "  Uninstall:     rm -rf ~/.mira ~/.claude/hooks/precompact-mira.py ~/.claude/hooks/mira_permission_hook.py"
echo ""
