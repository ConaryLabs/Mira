#!/bin/bash

echo "=== Mira Database Check ==="
echo

# Check if database exists
if [ ! -f "mira.db" ]; then
    echo "❌ Database file not found!"
    exit 1
fi

echo "✓ Database found: mira.db"
echo "  Size: $(ls -lh mira.db | awk '{print $5}')"
echo

# Check if sqlite3 is installed
if ! command -v sqlite3 &> /dev/null; then
    echo "❌ sqlite3 command not found. Please install sqlite3."
    echo "  Ubuntu/Debian: sudo apt-get install sqlite3"
    echo "  Fedora: sudo dnf install sqlite"
    echo "  Arch: sudo pacman -S sqlite"
    exit 1
fi

echo "✓ sqlite3 installed"
echo

# Basic database queries
echo "📊 Database Statistics:"
echo "----------------------"

echo -n "Total messages: "
sqlite3 mira.db "SELECT COUNT(*) FROM chat_history;" 2>/dev/null || echo "Error"

echo -n "Unique sessions: "
sqlite3 mira.db "SELECT COUNT(DISTINCT session_id) FROM chat_history;" 2>/dev/null || echo "Error"

echo
echo "📝 Recent Activity (last 5 messages):"
echo "------------------------------------"
sqlite3 -column -header mira.db "
SELECT 
    datetime(ts, 'unixepoch', 'localtime') as time,
    substr(session_id, 1, 12) || '...' as session,
    role,
    substr(content, 1, 40) || '...' as message
FROM chat_history 
ORDER BY ts DESC 
LIMIT 5;" 2>/dev/null || echo "Error reading messages"

echo
echo "💬 Sessions Summary:"
echo "-------------------"
sqlite3 -column -header mira.db "
SELECT 
    substr(session_id, 1, 12) || '...' as session,
    COUNT(*) as messages,
    datetime(MAX(ts), 'unixepoch', 'localtime') as last_active
FROM chat_history 
GROUP BY session_id
ORDER BY MAX(ts) DESC
LIMIT 5;" 2>/dev/null || echo "Error reading sessions"
