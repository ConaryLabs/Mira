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

# Create Mira directory
mkdir -p "$MIRA_DIR/data"
cd "$MIRA_DIR"

echo "Pulling Mira Docker image..."
# For now, we'll build locally. In production, this would be:
# docker pull ghcr.io/conarylabs/mira:latest

# Download source and build (temporary until published to registry)
if [ ! -f "Dockerfile" ]; then
    echo "Downloading Mira source..."
    curl -fsSL "https://github.com/ConaryLabs/Mira/archive/main.tar.gz" | tar -xz --strip-components=1 2>/dev/null || {
        echo "Could not download from GitHub. Please clone manually:"
        echo "  git clone https://github.com/ConaryLabs/Mira.git ~/.mira"
        echo "  cd ~/.mira && ./install.sh"
        exit 1
    }
fi

echo "Building Docker image (this may take a few minutes)..."
docker build -t mira:latest . > /dev/null

# Create wrapper script for Claude Code
cat > "$MIRA_DIR/mira" << 'WRAPPER'
#!/bin/bash
# Mira wrapper script - runs the MCP server in Docker
exec docker run -i --rm \
    -v "$HOME/.mira/data:/app/data" \
    mira:latest
WRAPPER
chmod +x "$MIRA_DIR/mira"

# Initialize database
echo "Initializing database..."
docker run --rm \
    -v "$MIRA_DIR/data:/app/data" \
    --entrypoint sh \
    mira:latest \
    -c "cat migrations/*.sql | sqlite3 /app/data/mira.db && sqlite3 /app/data/mira.db < seed_mira_guidelines.sql" 2>/dev/null

echo "Database initialized"

# Configure Claude Code
CLAUDE_CONFIG="$HOME/.claude/mcp.json"
mkdir -p "$(dirname "$CLAUDE_CONFIG")"

echo "Configuring Claude Code..."

# Read existing config or create new
if [ -f "$CLAUDE_CONFIG" ]; then
    # Check if mira is already configured
    if grep -q '"mira"' "$CLAUDE_CONFIG"; then
        echo "  Mira already in config, updating..."
    else
        # Backup and merge
        cp "$CLAUDE_CONFIG" "$CLAUDE_CONFIG.bak"
        echo "  Backed up existing config to $CLAUDE_CONFIG.bak"
    fi
fi

# Simple config for Mira
# If user has existing servers, they'll need to manually merge
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
echo "Next steps:"
echo ""
echo "1. Restart Claude Code to load Mira"
echo ""
echo "2. Add to your project's CLAUDE.md:"
echo ""
echo "   ## Mira Memory"
echo "   At session start:"
echo "   set_project(project_path=\"/path/to/your/project\")"
echo "   get_guidelines(category=\"mira_usage\")"
echo ""
echo "Installation: $MIRA_DIR"
echo "Database:     $MIRA_DIR/data/mira.db"
echo ""
echo "To uninstall: rm -rf ~/.mira && rm ~/.claude/mcp.json"
echo ""
