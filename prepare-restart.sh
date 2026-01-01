#!/bin/bash
# Prepare for Claude Code restart after Mira refactor
set -e

echo "=== Mira Refactor: Pre-restart Setup ==="
echo

# 1. Stop old systemd service if running
echo "1. Stopping old mira systemd service..."
if systemctl --user is-active mira >/dev/null 2>&1; then
    systemctl --user stop mira
    echo "   Stopped mira service"
else
    echo "   Service not running (ok)"
fi

# Disable to prevent auto-start
if systemctl --user is-enabled mira >/dev/null 2>&1; then
    systemctl --user disable mira
    echo "   Disabled mira service"
fi

# 2. Backup old database
echo
echo "2. Backing up old database..."
if [ -f ~/.mira/mira.db ]; then
    BACKUP=~/.mira/mira.db.backup.$(date +%Y%m%d_%H%M%S)
    cp ~/.mira/mira.db "$BACKUP"
    echo "   Backed up to $BACKUP"
else
    echo "   No existing database (ok)"
fi

# 3. Create fresh database directory
echo
echo "3. Ensuring ~/.mira directory exists..."
mkdir -p ~/.mira
echo "   Done"

# 4. Verify binary is built
echo
echo "4. Checking mira binary..."
if [ -x /home/peter/Mira/target/release/mira ]; then
    echo "   Binary ready: /home/peter/Mira/target/release/mira"
    /home/peter/Mira/target/release/mira --version
else
    echo "   ERROR: Binary not found! Run: cargo build --release"
    exit 1
fi

# 5. Check .mcp.json
echo
echo "5. Checking .mcp.json..."
cat /home/peter/Mira/.mcp.json
echo

# 6. Check GEMINI_API_KEY
echo
echo "6. Checking GEMINI_API_KEY..."
if [ -n "$GEMINI_API_KEY" ]; then
    echo "   GEMINI_API_KEY is set (semantic search enabled)"
elif [ -f ~/.mira/.env ]; then
    if grep -q GEMINI_API_KEY ~/.mira/.env; then
        echo "   Found in ~/.mira/.env - source it before starting Claude Code"
        echo "   Run: source ~/.mira/.env"
    fi
else
    echo "   Not set - semantic search will be disabled"
    echo "   Set it in your shell: export GEMINI_API_KEY=your_key"
fi

echo
echo "=== Ready! ==="
echo
echo "Next steps:"
echo "1. If needed: source ~/.mira/.env   (for GEMINI_API_KEY)"
echo "2. Restart Claude Code"
echo "3. The new Mira MCP will start automatically via .mcp.json"
echo
