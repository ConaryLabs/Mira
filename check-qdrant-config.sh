#!/bin/bash

# Qdrant Configuration Checker for Mira Backend
# This script helps diagnose why memory search returns empty results

echo "🔍 Checking Qdrant and OpenAI Configuration..."
echo "============================================="

# Check if .env file exists
if [ -f .env ]; then
    echo "✅ .env file found"
    
    # Check OpenAI API key
    if grep -q "OPENAI_API_KEY=" .env && ! grep -q "OPENAI_API_KEY=$" .env && ! grep -q "OPENAI_API_KEY=\"\"" .env; then
        echo "✅ OpenAI API key is configured"
    else
        echo "❌ OpenAI API key is missing or empty in .env"
        echo "   Add: OPENAI_API_KEY=your-key-here"
    fi
    
    # Check Qdrant URL
    if grep -q "QDRANT_URL=" .env; then
        QDRANT_URL=$(grep "QDRANT_URL=" .env | cut -d'=' -f2 | tr -d '"')
        echo "✅ Qdrant URL configured: $QDRANT_URL"
    else
        echo "⚠️  Qdrant URL not found in .env"
        echo "   Add: QDRANT_URL=http://localhost:6333"
    fi
else
    echo "❌ .env file not found!"
    echo "   Create .env with:"
    echo "   OPENAI_API_KEY=your-key-here"
    echo "   QDRANT_URL=http://localhost:6333"
fi

echo ""
echo "🐳 Checking if Qdrant is running..."
echo "------------------------------------"

# Check if Qdrant container is running
if docker ps | grep -q qdrant; then
    echo "✅ Qdrant container is running"
    
    # Get container info
    CONTAINER_ID=$(docker ps | grep qdrant | awk '{print $1}')
    echo "   Container ID: $CONTAINER_ID"
    
    # Check port mapping
    PORT_INFO=$(docker port $CONTAINER_ID 6333/tcp 2>/dev/null)
    if [ ! -z "$PORT_INFO" ]; then
        echo "   Port mapping: $PORT_INFO"
    fi
else
    echo "❌ Qdrant container is not running"
    echo ""
    echo "   Start Qdrant with:"
    echo "   docker run -p 6333:6333 -p 6334:6334 \\"
    echo "     -v ./qdrant_storage:/qdrant/storage:z \\"
    echo "     qdrant/qdrant"
fi

echo ""
echo "🔌 Testing Qdrant API..."
echo "------------------------"

# Test Qdrant API endpoint
QDRANT_URL="${QDRANT_URL:-http://localhost:6333}"
if curl -s -o /dev/null -w "%{http_code}" "$QDRANT_URL/collections" | grep -q "200"; then
    echo "✅ Qdrant API is accessible"
    
    # List collections
    echo ""
    echo "📚 Qdrant Collections:"
    curl -s "$QDRANT_URL/collections" | jq -r '.result.collections[].name' 2>/dev/null || echo "   (unable to parse collections)"
    
    # Check for Mira collections
    echo ""
    echo "🔍 Checking for Mira collections..."
    for collection in "memories_semantic" "memories_code" "memories_summary"; do
        if curl -s "$QDRANT_URL/collections/$collection" | grep -q "\"status\":\"ok\""; then
            echo "✅ Collection exists: $collection"
            
            # Get point count
            COUNT=$(curl -s "$QDRANT_URL/collections/$collection" | jq '.result.points_count' 2>/dev/null)
            echo "   Points count: ${COUNT:-unknown}"
        else
            echo "❌ Collection missing: $collection"
        fi
    done
else
    echo "❌ Cannot connect to Qdrant at $QDRANT_URL"
    echo "   Make sure Qdrant is running and accessible"
fi

echo ""
echo "🧪 Testing OpenAI Embeddings..."
echo "--------------------------------"

# Create a simple Python test script
cat > test_embedding.py << 'EOF'
import os
import sys
from openai import OpenAI

api_key = os.getenv("OPENAI_API_KEY")
if not api_key:
    print("❌ OPENAI_API_KEY not set")
    sys.exit(1)

try:
    client = OpenAI(api_key=api_key)
    response = client.embeddings.create(
        model="text-embedding-3-small",
        input="test"
    )
    print(f"✅ Embedding generated successfully")
    print(f"   Model: {response.model}")
    print(f"   Dimensions: {len(response.data[0].embedding)}")
except Exception as e:
    print(f"❌ Failed to generate embedding: {e}")
EOF

if command -v python3 &> /dev/null; then
    python3 test_embedding.py
else
    echo "⚠️  Python3 not found, skipping embedding test"
fi

rm -f test_embedding.py

echo ""
echo "📝 Summary and Recommendations:"
echo "================================"

# Provide recommendations based on findings
if ! docker ps | grep -q qdrant; then
    echo "1. Start Qdrant container first"
fi

if [ ! -f .env ] || ! grep -q "OPENAI_API_KEY=" .env; then
    echo "2. Configure OpenAI API key in .env"
fi

if ! curl -s -o /dev/null -w "%{http_code}" "$QDRANT_URL/collections" | grep -q "200"; then
    echo "3. Verify Qdrant is accessible at $QDRANT_URL"
fi

echo ""
echo "🚀 Quick Fix Commands:"
echo "----------------------"
echo "# Start Qdrant:"
echo "docker run -d --name qdrant -p 6333:6333 -p 6334:6334 -v ./qdrant_storage:/qdrant/storage:z qdrant/qdrant"
echo ""
echo "# Create collections (run after starting backend once):"
echo "curl -X PUT 'http://localhost:6333/collections/memories_semantic' \\"
echo "  -H 'Content-Type: application/json' \\"
echo "  -d '{\"vectors\": {\"size\": 1536, \"distance\": \"Cosine\"}}'"
echo ""
echo "# Check collection status:"
echo "curl 'http://localhost:6333/collections'"
