#!/bin/bash
# backend/scripts/db-reset-test.sh
# Clean up test Qdrant collections
# Usage: ./scripts/db-reset-test.sh [--force]

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Configuration
QDRANT_URL="${QDRANT_URL:-http://localhost:6333}"

# Parse arguments
FORCE=false
if [ "$1" = "--force" ]; then
    FORCE=true
fi

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}   TEST COLLECTIONS CLEANUP${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""
echo "Qdrant URL: $QDRANT_URL"
echo ""

# Check if Qdrant is reachable
echo -e "${YELLOW}Checking Qdrant connection...${NC}"
if ! curl -s "$QDRANT_URL/collections" > /dev/null 2>&1; then
    echo -e "${RED}Error: Cannot connect to Qdrant at $QDRANT_URL${NC}"
    echo "Make sure Qdrant is running."
    exit 1
fi
echo -e "${GREEN}Connected to Qdrant${NC}"

# Get all collections
echo ""
echo -e "${YELLOW}Finding test collections...${NC}"
collections_json=$(curl -s "$QDRANT_URL/collections")
all_collections=$(echo "$collections_json" | grep -o '"name":"[^"]*"' | cut -d'"' -f4)

# Find test collections (prefixes: test_, e2e_test_)
test_collections=()
while IFS= read -r collection; do
    if [[ "$collection" == test_* ]] || [[ "$collection" == e2e_test_* ]]; then
        test_collections+=("$collection")
    fi
done <<< "$all_collections"

if [ ${#test_collections[@]} -eq 0 ]; then
    echo ""
    echo -e "${GREEN}No test collections found. Nothing to clean up.${NC}"
    exit 0
fi

echo ""
echo "Found ${#test_collections[@]} test collection(s):"
for collection in "${test_collections[@]}"; do
    echo "  - $collection"
done
echo ""

if [ "$FORCE" = false ]; then
    read -p "Delete these collections? [y/N] " confirm
    if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
        echo "Aborted."
        exit 0
    fi
fi

# Delete test collections
echo ""
echo -e "${YELLOW}Deleting test collections...${NC}"

deleted=0
for collection in "${test_collections[@]}"; do
    response=$(curl -s -w "\n%{http_code}" -X DELETE "$QDRANT_URL/collections/$collection" 2>/dev/null)
    http_code=$(echo "$response" | tail -n1)

    if [ "$http_code" = "200" ]; then
        echo -e "  ${GREEN}Deleted:${NC} $collection"
        ((deleted++))
    else
        echo -e "  ${RED}Failed:${NC} $collection (HTTP $http_code)"
    fi
done

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   CLEANUP COMPLETE${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Deleted $deleted test collection(s)."
