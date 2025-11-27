#!/bin/bash
# backend/scripts/db-reset-qdrant.sh
# Reset Qdrant collections only (preserves SQLite data)
# Usage: ./scripts/db-reset-qdrant.sh [--force]

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Configuration
QDRANT_URL="${QDRANT_URL:-http://localhost:6333}"
COLLECTIONS=("mira_code" "mira_conversation" "mira_git")

# Parse arguments
FORCE=false
if [ "$1" = "--force" ]; then
    FORCE=true
fi

echo -e "${YELLOW}========================================${NC}"
echo -e "${YELLOW}   QDRANT COLLECTIONS RESET${NC}"
echo -e "${YELLOW}========================================${NC}"
echo ""
echo "This will DELETE collections: ${COLLECTIONS[*]}"
echo "SQLite data will be preserved."
echo ""
echo "Qdrant URL: $QDRANT_URL"
echo ""

if [ "$FORCE" = false ]; then
    read -p "Type 'RESET' to confirm: " confirm
    if [ "$confirm" != "RESET" ]; then
        echo "Aborted."
        exit 0
    fi
fi

# Check if Qdrant is reachable
echo ""
echo -e "${YELLOW}Checking Qdrant connection...${NC}"
if ! curl -s "$QDRANT_URL/collections" > /dev/null 2>&1; then
    echo -e "${RED}Error: Cannot connect to Qdrant at $QDRANT_URL${NC}"
    echo "Make sure Qdrant is running."
    exit 1
fi
echo -e "${GREEN}Connected to Qdrant${NC}"

# Delete collections
echo ""
echo -e "${YELLOW}Deleting collections...${NC}"

deleted=0
for collection in "${COLLECTIONS[@]}"; do
    response=$(curl -s -w "\n%{http_code}" -X DELETE "$QDRANT_URL/collections/$collection" 2>/dev/null)
    http_code=$(echo "$response" | tail -n1)

    if [ "$http_code" = "200" ]; then
        echo -e "  ${GREEN}Deleted:${NC} $collection"
        ((deleted++))
    elif [ "$http_code" = "404" ]; then
        echo "  Not found: $collection (already deleted)"
    else
        echo -e "  ${RED}Failed:${NC} $collection (HTTP $http_code)"
    fi
done

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   QDRANT RESET COMPLETE${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Deleted $deleted collection(s)."
echo "Collections will be recreated automatically on backend start."
echo ""
echo -e "${YELLOW}Note:${NC} SQLite data is still intact."
echo "Embedding references in SQLite will be orphaned."
