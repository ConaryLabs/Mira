#!/bin/bash
# backend/scripts/db-reset.sh
# Full database reset - wipes both SQLite and Qdrant
# Usage: ./scripts/db-reset.sh [--force]

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Get script directory and backend root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$(dirname "$SCRIPT_DIR")"

# Configuration
QDRANT_URL="${QDRANT_URL:-http://localhost:6333}"
DB_FILE="$BACKEND_DIR/data/mira.db"
COLLECTIONS=("mira_code" "mira_conversation" "mira_git")

# Parse arguments
FORCE=false
if [ "$1" = "--force" ]; then
    FORCE=true
fi

echo -e "${RED}========================================${NC}"
echo -e "${RED}   FULL DATABASE RESET${NC}"
echo -e "${RED}========================================${NC}"
echo ""
echo "This will DELETE:"
echo "  - SQLite database: $DB_FILE"
echo "  - Qdrant collections: ${COLLECTIONS[*]}"
echo ""
echo -e "${YELLOW}All data will be permanently lost!${NC}"
echo ""

if [ "$FORCE" = false ]; then
    read -p "Type 'RESET' to confirm: " confirm
    if [ "$confirm" != "RESET" ]; then
        echo "Aborted."
        exit 0
    fi
fi

# Check if backend service is running
SERVICE_WAS_RUNNING=false
if systemctl is-active --quiet mira-backend 2>/dev/null; then
    SERVICE_WAS_RUNNING=true
    echo -e "${YELLOW}Stopping mira-backend service...${NC}"
    sudo systemctl stop mira-backend
fi

# Reset SQLite
echo ""
echo -e "${YELLOW}Resetting SQLite database...${NC}"
if [ -f "$DB_FILE" ]; then
    rm -f "$DB_FILE" "$DB_FILE-shm" "$DB_FILE-wal"
    echo "  Deleted: $DB_FILE"
else
    echo "  No existing database found"
fi

# Run migrations
echo "  Running migrations..."
cd "$BACKEND_DIR"
if command -v sqlx &> /dev/null; then
    DATABASE_URL="sqlite:./data/mira.db" sqlx migrate run
    echo -e "  ${GREEN}Migrations complete${NC}"
else
    echo -e "  ${YELLOW}sqlx not found - migrations will run on backend start${NC}"
fi

# Reset Qdrant collections
echo ""
echo -e "${YELLOW}Resetting Qdrant collections...${NC}"

for collection in "${COLLECTIONS[@]}"; do
    response=$(curl -s -w "\n%{http_code}" -X DELETE "$QDRANT_URL/collections/$collection" 2>/dev/null)
    http_code=$(echo "$response" | tail -n1)

    if [ "$http_code" = "200" ]; then
        echo -e "  ${GREEN}Deleted:${NC} $collection"
    elif [ "$http_code" = "404" ]; then
        echo "  Not found: $collection (already deleted)"
    else
        echo -e "  ${RED}Failed:${NC} $collection (HTTP $http_code)"
    fi
done

echo ""
echo "  Collections will be recreated on backend start"

# Restart service if it was running
if [ "$SERVICE_WAS_RUNNING" = true ]; then
    echo ""
    echo -e "${YELLOW}Restarting mira-backend service...${NC}"
    sudo systemctl start mira-backend
    sleep 2
    if systemctl is-active --quiet mira-backend; then
        echo -e "${GREEN}Service restarted successfully${NC}"
    else
        echo -e "${RED}Service failed to start - check logs with: journalctl -u mira-backend${NC}"
    fi
fi

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   RESET COMPLETE${NC}"
echo -e "${GREEN}========================================${NC}"
