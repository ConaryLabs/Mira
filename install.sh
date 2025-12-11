#!/bin/bash
# Mira Power Suit - Easy Setup Script
# Usage: curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash

set -e

MIRA_DIR="${MIRA_DIR:-$HOME/.mira}"

echo "Installing Mira Power Suit for Claude Code"
echo ""

# Check for Docker
if ! command -v docker &> /dev/null; then
    echo "Error: Docker is required but not installed."
    echo "Please install Docker: https://docs.docker.com/get-docker/"
    exit 1
fi

# Check for docker compose
if ! docker compose version &> /dev/null; then
    echo "Error: Docker Compose is required but not installed."
    echo "Please install Docker Compose: https://docs.docker.com/compose/install/"
    exit 1
fi

# Create Mira directory
mkdir -p "$MIRA_DIR"
cd "$MIRA_DIR"

# Download source if not present
if [ ! -f "Dockerfile" ]; then
    echo "Downloading Mira source..."
    curl -fsSL "https://github.com/ConaryLabs/Mira/archive/main.tar.gz" | tar -xz --strip-components=1 2>/dev/null || {
        echo "Could not download from GitHub. Please clone manually:"
        echo "  git clone https://github.com/ConaryLabs/Mira.git ~/.mira"
        echo "  cd ~/.mira && ./install.sh"
        exit 1
    }
fi

echo "Building Docker images (this may take a few minutes)..."
docker compose build

echo "Starting Qdrant (vector database for semantic search)..."
docker compose up -d qdrant

# Wait for Qdrant to be ready
echo "Waiting for Qdrant to start..."
for i in {1..30}; do
    if curl -s http://localhost:6334/healthz > /dev/null 2>&1; then
        break
    fi
    sleep 1
done

# Initialize database
echo "Initializing database..."
docker compose run --rm -T mira sh -c "cat migrations/*.sql | sqlite3 /app/data/mira.db && sqlite3 /app/data/mira.db < seed_mira_guidelines.sql" 2>/dev/null

echo "Database initialized"

# Create wrapper script for Claude Code
# This runs mira connected to the Qdrant container
cat > "$MIRA_DIR/mira" << 'WRAPPER'
#!/bin/bash
# Mira wrapper script - runs the MCP server in Docker with Qdrant
cd "$HOME/.mira"
exec docker compose run --rm -T \
    -e OPENAI_API_KEY="${OPENAI_API_KEY:-}" \
    mira
WRAPPER
chmod +x "$MIRA_DIR/mira"

# Configure Claude Code
CLAUDE_CONFIG="$HOME/.claude/mcp.json"
mkdir -p "$(dirname "$CLAUDE_CONFIG")"

echo "Configuring Claude Code..."

if [ -f "$CLAUDE_CONFIG" ]; then
    if grep -q '"mira"' "$CLAUDE_CONFIG"; then
        echo "  Mira already in config, updating..."
    else
        cp "$CLAUDE_CONFIG" "$CLAUDE_CONFIG.bak"
        echo "  Backed up existing config to $CLAUDE_CONFIG.bak"
    fi
fi

cat > "$CLAUDE_CONFIG" << EOF
{
  "mcpServers": {
    "mira": {
      "command": "$MIRA_DIR/mira"
    }
  }
}
EOF

echo "Claude Code configured"

# Done
echo ""
echo "============================================"
echo "Mira Power Suit installed"
echo "============================================"
echo ""
echo "Components:"
echo "  - Mira MCP server (Docker)"
echo "  - Qdrant vector database (Docker, port 6334)"
echo "  - SQLite database (~/.mira/data/mira.db)"
echo ""
echo "Next steps:"
echo ""
echo "1. (Optional) Enable semantic search by setting OPENAI_API_KEY:"
echo "   export OPENAI_API_KEY=sk-..."
echo ""
echo "2. Restart Claude Code to load Mira"
echo ""
echo "3. Add to your project's CLAUDE.md:"
echo ""
echo "   ## Mira Memory"
echo "   At session start:"
echo "   set_project(project_path=\"/path/to/your/project\")"
echo "   get_guidelines(category=\"mira_usage\")"
echo ""
echo "Installation: $MIRA_DIR"
echo ""
echo "Commands:"
echo "  Start Qdrant:  cd ~/.mira && docker compose up -d qdrant"
echo "  Stop Qdrant:   cd ~/.mira && docker compose down"
echo "  Uninstall:     rm -rf ~/.mira && rm ~/.claude/mcp.json"
echo ""
