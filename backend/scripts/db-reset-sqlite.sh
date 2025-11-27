#!/bin/bash
# backend/scripts/db-reset-sqlite.sh
# Reset SQLite database only (preserves Qdrant embeddings)
# Usage: ./scripts/db-reset-sqlite.sh [--force]

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
DB_FILE="$BACKEND_DIR/data/mira.db"

# Parse arguments
FORCE=false
if [ "$1" = "--force" ]; then
    FORCE=true
fi

echo -e "${YELLOW}========================================${NC}"
echo -e "${YELLOW}   SQLITE DATABASE RESET${NC}"
echo -e "${YELLOW}========================================${NC}"
echo ""
echo "This will DELETE: $DB_FILE"
echo "Qdrant embeddings will be preserved."
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

# Delete database files
echo ""
echo -e "${YELLOW}Deleting SQLite database...${NC}"
if [ -f "$DB_FILE" ]; then
    rm -f "$DB_FILE" "$DB_FILE-shm" "$DB_FILE-wal"
    echo "  Deleted: $DB_FILE"
else
    echo "  No existing database found"
fi

# Ensure data directory exists
mkdir -p "$BACKEND_DIR/data"

# Run migrations
echo ""
echo -e "${YELLOW}Running migrations...${NC}"
cd "$BACKEND_DIR"
if command -v sqlx &> /dev/null; then
    DATABASE_URL="sqlite:./data/mira.db" sqlx migrate run
    echo -e "${GREEN}Migrations complete${NC}"
else
    echo -e "${YELLOW}sqlx not found - migrations will run on backend start${NC}"
fi

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
echo -e "${GREEN}   SQLITE RESET COMPLETE${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo -e "${YELLOW}Note:${NC} Qdrant embeddings are still intact."
echo "They may reference deleted SQLite records."
echo "Run db-reset-qdrant.sh if you want to clear embeddings too."
