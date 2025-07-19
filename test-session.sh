#!/bin/bash

# Test session persistence in Mira

echo "=== Testing Mira Session Persistence ==="
echo

# First, let's check if the database exists
if [ -f "mira.db" ]; then
    echo "✓ Database file exists: mira.db"
    echo "  Size: $(ls -lh mira.db | awk '{print $5}')"
    echo
else
    echo "✗ Database file not found!"
    echo
fi

# Make a request and capture the session cookie
echo "1. Making first request..."
RESPONSE=$(curl -s -i -X POST http://localhost:8080/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "Hello Mira! Remember this number: 42"}')

# Extract the session cookie
SESSION_COOKIE=$(echo "$RESPONSE" | grep -i "set-cookie: mira_session=" | sed 's/.*mira_session=\([^;]*\).*/\1/')

if [ -z "$SESSION_COOKIE" ]; then
    echo "✗ No session cookie received!"
    exit 1
fi

echo "✓ Got session cookie: $SESSION_COOKIE"
echo

# Extract the JSON response body (curl -i includes headers)
BODY=$(echo "$RESPONSE" | tail -n 1)
echo "Response 1: $BODY"
echo

# Wait a bit
sleep 1

# Make a second request with the same session
echo "2. Making second request with same session..."
RESPONSE2=$(curl -s -X POST http://localhost:8080/chat \
  -H "Content-Type: application/json" \
  -H "Cookie: mira_session=$SESSION_COOKIE" \
  -d '{"message": "What number did I just tell you?"}')

echo "Response 2: $RESPONSE2"
echo

# Check the database directly
echo "3. Checking database contents..."
echo "Number of messages in session $SESSION_COOKIE:"
sqlite3 mira.db "SELECT COUNT(*) FROM chat_history WHERE session_id = '$SESSION_COOKIE';" 2>/dev/null || echo "Error reading database"

echo
echo "Last 5 messages in this session:"
sqlite3 mira.db "SELECT role, substr(content, 1, 50) || '...' FROM chat_history WHERE session_id = '$SESSION_COOKIE' ORDER BY ts DESC LIMIT 5;" 2>/dev/null || echo "Error reading database"

echo
echo "=== Test Complete ==="
