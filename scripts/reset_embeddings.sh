#!/bin/bash
# scripts/reset_embeddings.sh
# Nuclear option: Reset all embeddings in Qdrant and SQLite tracking

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Load .env if it exists
if [ -f .env ]; then
    set -a
    source .env
    set +a
fi

QDRANT_URL=${QDRANT_URL:-http://localhost:6333}
DATABASE_URL=${DATABASE_URL:-sqlite:./mira.db}

echo -e "${RED}╔════════════════════════════════════════════════════════╗${NC}"
echo -e "${RED}║  WARNING: EMBEDDING RESET                              ║${NC}"
echo -e "${RED}║  This will delete ALL embeddings from Qdrant          ║${NC}"
echo -e "${RED}║  and clear tracking tables in SQLite                  ║${NC}"
echo -e "${RED}╚════════════════════════════════════════════════════════╝${NC}\n"

echo -e "${YELLOW}Collections to be deleted:${NC}"
echo "  - semantic (conversation embeddings)"
echo "  - code (code element embeddings)"
echo "  - summary (rolling summaries)"
echo "  - documents (uploaded docs)"
echo "  - relationship (relationship facts)"
echo ""

echo -e "${YELLOW}SQLite tables to be cleared:${NC}"
echo "  - message_embeddings (embedding tracking)"
echo ""

read -p "Are you sure you want to continue? (type 'yes' to confirm): " -r
echo
if [[ ! $REPLY == "yes" ]]; then
    echo -e "${GREEN}Aborted. No changes made.${NC}"
    exit 0
fi

echo ""
echo -e "${BLUE}Starting reset...${NC}\n"

# Check Qdrant connectivity
if ! curl -s ${QDRANT_URL}/health > /dev/null 2>&1; then
    echo -e "${RED}✗ Cannot connect to Qdrant at ${QDRANT_URL}${NC}"
    exit 1
fi

# Delete Qdrant collections
COLLECTIONS=("mira_semantic" "mira_code" "mira_summary" "mira_documents" "mira_relationship")

for collection in "${COLLECTIONS[@]}"; do
    echo -e "${BLUE}Deleting collection: ${collection}${NC}"
    
    response=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE "${QDRANT_URL}/collections/${collection}")
    
    if [ "$response" == "200" ] || [ "$response" == "404" ]; then
        echo -e "${GREEN}✓ Deleted ${collection}${NC}"
    else
        echo -e "${YELLOW}⚠ Failed to delete ${collection} (HTTP ${response})${NC}"
    fi
done

echo ""

# Clear SQLite tracking tables
echo -e "${BLUE}Clearing SQLite tracking tables...${NC}"

# Extract database path from DATABASE_URL
DB_PATH=$(echo $DATABASE_URL | sed 's/sqlite://')

if [ ! -f "$DB_PATH" ]; then
    echo -e "${YELLOW}⚠ Database not found at ${DB_PATH}${NC}"
else
    sqlite3 "$DB_PATH" <<EOF
DELETE FROM message_embeddings;
EOF
    
    count=$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM message_embeddings;")
    
    if [ "$count" == "0" ]; then
        echo -e "${GREEN}✓ Cleared message_embeddings table${NC}"
    else
        echo -e "${RED}✗ Failed to clear message_embeddings table${NC}"
        exit 1
    fi
fi

echo ""
echo -e "${GREEN}╔════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║  RESET COMPLETE                                        ║${NC}"
echo -e "${GREEN}║  All embeddings have been deleted                     ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════════════════════╝${NC}\n"

echo -e "${BLUE}Next steps:${NC}"
echo "  1. Embeddings will be regenerated automatically as:"
echo "     - New messages are sent"
echo "     - Code files are analyzed"
echo "     - Documents are uploaded"
echo "  2. Or run a backfill task to re-embed existing content"
echo ""

exit 0
