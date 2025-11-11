#!/bin/bash
# scripts/reset_embeddings.sh
#
# Nuclear option: Completely resets all embeddings by deleting Qdrant collections
# and clearing embedding columns in SQLite. Use when things are truly fucked.

set -e

# Colors for output
RED='\033[0;31m'
YELLOW='\033[1;33m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

echo -e "${RED}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${RED}║           NUCLEAR EMBEDDING RESET                          ║${NC}"
echo -e "${RED}║                                                            ║${NC}"
echo -e "${RED}║  This will DELETE ALL EMBEDDINGS. This includes:          ║${NC}"
echo -e "${RED}║  • All Qdrant collections                                  ║${NC}"
echo -e "${RED}║  • All embedding data in SQLite                            ║${NC}"
echo -e "${RED}║  • All vector tracking metadata                            ║${NC}"
echo -e "${RED}║                                                            ║${NC}"
echo -e "${RED}║  Your message content will be preserved.                   ║${NC}"
echo -e "${RED}║  Embeddings will be regenerated on next use.               ║${NC}"
echo -e "${RED}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${YELLOW}This action CANNOT be undone.${NC}"
echo ""
read -p "Type 'NUKE' to confirm: " confirm

if [ "$confirm" != "NUKE" ]; then
    echo "Aborted."
    exit 0
fi

echo ""
echo -e "${YELLOW}Starting nuclear reset...${NC}"

# Get configuration from environment or use defaults
QDRANT_URL="${QDRANT_URL:-http://localhost:6333}"
SQLITE_DB="${SQLITE_DB:-mira.db}"
COLLECTION_PREFIX="${QDRANT_COLLECTION_PREFIX:-mira}"

echo ""
echo "Configuration:"
echo "  Qdrant URL: $QDRANT_URL"
echo "  SQLite DB: $SQLITE_DB"
echo "  Collection Prefix: $COLLECTION_PREFIX"
echo ""

# Step 1: Delete Qdrant collections
echo -e "${YELLOW}Step 1/3: Deleting Qdrant collections...${NC}"

COLLECTIONS=(
    "${COLLECTION_PREFIX}-semantic"
    "${COLLECTION_PREFIX}-code"
    "${COLLECTION_PREFIX}-summary"
    "${COLLECTION_PREFIX}-documents"
    "${COLLECTION_PREFIX}-relationship"
)

deleted=0
not_found=0

for collection in "${COLLECTIONS[@]}"; do
    response=$(curl -s -w "\n%{http_code}" -X DELETE "$QDRANT_URL/collections/$collection" 2>/dev/null)
    http_code=$(echo "$response" | tail -n1)
    
    if [ "$http_code" = "200" ]; then
        echo "  ✓ Deleted: $collection"
        ((deleted++))
    elif [ "$http_code" = "404" ]; then
        echo "  - Not found: $collection (already deleted or never existed)"
        ((not_found++))
    else
        echo "  ✗ Failed to delete: $collection (HTTP $http_code)"
    fi
done

echo ""
echo "  Collections deleted: $deleted"
echo "  Collections not found: $not_found"

# Step 2: Clear SQLite embedding columns
echo ""
echo -e "${YELLOW}Step 2/3: Clearing SQLite embedding data...${NC}"

if [ ! -f "$SQLITE_DB" ]; then
    echo -e "${RED}  ✗ Database not found: $SQLITE_DB${NC}"
    echo "  Skipping SQLite cleanup."
else
    sqlite3 "$SQLITE_DB" <<EOF
-- Clear embedding vector data
UPDATE memory_entries SET embedding = NULL WHERE embedding IS NOT NULL;

-- Clear Qdrant point tracking
UPDATE memory_entries SET qdrant_point_id = NULL WHERE qdrant_point_id IS NOT NULL;

-- Clear embedding metadata
UPDATE memory_entries 
SET metadata = json_remove(metadata, '$.embedding_model', '$.embedding_head')
WHERE metadata IS NOT NULL;

-- Report changes
SELECT 
    'Cleared ' || COUNT(*) || ' embedding vectors' as result 
FROM memory_entries;
EOF

    echo "  ✓ SQLite cleanup complete"
fi

# Step 3: Verify cleanup
echo ""
echo -e "${YELLOW}Step 3/3: Verifying cleanup...${NC}"

# Check Qdrant collections
response=$(curl -s "$QDRANT_URL/collections" 2>/dev/null || echo "{}")
collection_count=$(echo "$response" | grep -o "\"$COLLECTION_PREFIX-" | wc -l || echo "0")

echo "  Remaining Qdrant collections with prefix '$COLLECTION_PREFIX': $collection_count"

# Check SQLite
if [ -f "$SQLITE_DB" ]; then
    remaining=$(sqlite3 "$SQLITE_DB" "SELECT COUNT(*) FROM memory_entries WHERE embedding IS NOT NULL;" 2>/dev/null || echo "?")
    echo "  Remaining SQLite embeddings: $remaining"
fi

echo ""
echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║                  RESET COMPLETE                            ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "Next steps:"
echo "  1. Restart your Mira backend"
echo "  2. Embeddings will be regenerated automatically as messages are processed"
echo "  3. Use the backfill task if you want to regenerate all embeddings immediately"
echo ""
