#!/bin/bash
# deploy/setup-ubuntu.sh
# Mira VPS Setup Script for Ubuntu 24.04
# Sets up Mira with HTTPS for Claude Connections
#
# Usage:
#   curl -sSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/deploy/setup-ubuntu.sh | bash -s YOUR_DOMAIN
#
# Or download and run:
#   chmod +x setup-ubuntu.sh
#   ./setup-ubuntu.sh mira.yourdomain.com

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log() { echo -e "${GREEN}[+]${NC} $1"; }
warn() { echo -e "${YELLOW}[!]${NC} $1"; }
error() { echo -e "${RED}[x]${NC} $1"; exit 1; }

# Check for domain argument
DOMAIN="${1:-}"
AUTH_TOKEN="${2:-}"

if [ -z "$DOMAIN" ]; then
    echo "Usage: $0 <domain> [auth-token]"
    echo "Example: $0 mira.yourdomain.com"
    echo "Example: $0 mira.yourdomain.com my-secret-token"
    echo ""
    echo "Make sure your domain's DNS A record points to this server's IP."
    echo "The optional auth-token adds Bearer token authentication to the MCP endpoint."
    exit 1
fi

# Must run as root or with sudo
if [ "$EUID" -ne 0 ]; then
    error "Please run as root or with sudo"
fi

# Get the actual user (not root) for installation
ACTUAL_USER="${SUDO_USER:-$USER}"
ACTUAL_HOME=$(eval echo ~$ACTUAL_USER)

log "Setting up Mira on Ubuntu 24.04"
log "Domain: $DOMAIN"
log "User: $ACTUAL_USER"
log "Home: $ACTUAL_HOME"

# Update system
log "Updating system packages..."
apt-get update
apt-get upgrade -y

# Install build dependencies
log "Installing build dependencies..."
apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    git \
    curl \
    sqlite3

# Install Rust for the actual user
log "Installing Rust..."
if [ ! -d "$ACTUAL_HOME/.cargo" ]; then
    sudo -u "$ACTUAL_USER" bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
fi

# Source cargo for this script
export PATH="$ACTUAL_HOME/.cargo/bin:$PATH"

# Install Caddy (for HTTPS reverse proxy)
log "Installing Caddy..."
apt-get install -y debian-keyring debian-archive-keyring apt-transport-https
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | tee /etc/apt/sources.list.d/caddy-stable.list
apt-get update
apt-get install -y caddy

# Clone Mira repository
log "Cloning Mira repository..."
MIRA_DIR="$ACTUAL_HOME/Mira"
if [ -d "$MIRA_DIR" ]; then
    warn "Mira directory exists, pulling latest..."
    sudo -u "$ACTUAL_USER" git -C "$MIRA_DIR" pull
else
    sudo -u "$ACTUAL_USER" git clone https://github.com/ConaryLabs/Mira.git "$MIRA_DIR"
fi

# Build Mira
log "Building Mira (this may take a few minutes)..."
cd "$MIRA_DIR"
sudo -u "$ACTUAL_USER" bash -c "source $ACTUAL_HOME/.cargo/env && cargo build --release"

# Create .mira directory
log "Creating Mira data directory..."
sudo -u "$ACTUAL_USER" mkdir -p "$ACTUAL_HOME/.mira"

# Create environment file
log "Creating environment file..."
ENV_FILE="$MIRA_DIR/.env"
if [ ! -f "$ENV_FILE" ]; then
    cat > "$ENV_FILE" << 'EOF'
# Mira Environment Configuration
# Add your API keys here

# Required for embeddings (semantic search)
OPENAI_API_KEY=

# Optional: DeepSeek for chat (only needed if using Mira Studio chat)
# DEEPSEEK_API_KEY=
EOF
    chown "$ACTUAL_USER:$ACTUAL_USER" "$ENV_FILE"
    chmod 600 "$ENV_FILE"
    warn "Created $ENV_FILE - add your OPENAI_API_KEY for embeddings"
fi

# Create systemd service
log "Creating systemd service..."
cat > /etc/systemd/system/mira.service << EOF
[Unit]
Description=Mira - Memory and Intelligence Layer
After=network.target

[Service]
Type=simple
User=$ACTUAL_USER
WorkingDirectory=$MIRA_DIR
ExecStart=$MIRA_DIR/target/release/mira web
Restart=on-failure
RestartSec=5

# Environment
EnvironmentFile=$MIRA_DIR/.env

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier=mira

[Install]
WantedBy=multi-user.target
EOF

# Configure Caddy for HTTPS
log "Configuring Caddy reverse proxy..."

if [ -n "$AUTH_TOKEN" ]; then
    log "Configuring with Bearer token authentication..."
    cat > /etc/caddy/Caddyfile << EOF
$DOMAIN {
    # MCP endpoint for Claude Connections (with auth)
    @mcp path /mcp*
    handle @mcp {
        @no_auth not header Authorization "Bearer $AUTH_TOKEN"
        respond @no_auth "Unauthorized" 401
        reverse_proxy localhost:3000
    }

    # Health check (no auth required)
    handle /health {
        reverse_proxy localhost:3000
    }

    # API routes (with auth)
    @api path /api/*
    handle @api {
        @no_auth_api not header Authorization "Bearer $AUTH_TOKEN"
        respond @no_auth_api "Unauthorized" 401
        reverse_proxy localhost:3000
    }

    # WebSocket support (with auth)
    @ws path /ws
    handle @ws {
        @no_auth_ws not header Authorization "Bearer $AUTH_TOKEN"
        respond @no_auth_ws "Unauthorized" 401
        reverse_proxy localhost:3000
    }

    # Root
    handle {
        respond "Mira MCP Server" 200
    }
}
EOF
else
    log "Configuring without authentication (public access)..."
    cat > /etc/caddy/Caddyfile << EOF
$DOMAIN {
    # MCP endpoint for Claude Connections
    handle /mcp/* {
        reverse_proxy localhost:3000
    }

    # Health check
    handle /health {
        reverse_proxy localhost:3000
    }

    # API routes (optional, for full Mira Studio access)
    handle /api/* {
        reverse_proxy localhost:3000
    }

    # WebSocket support
    handle /ws {
        reverse_proxy localhost:3000
    }

    # Root
    handle {
        respond "Mira MCP Server" 200
    }
}
EOF
fi

# Open firewall ports
log "Configuring firewall..."
if command -v ufw &> /dev/null; then
    ufw allow 22/tcp   # SSH
    ufw allow 80/tcp   # HTTP (for ACME challenge)
    ufw allow 443/tcp  # HTTPS
    ufw --force enable
fi

# Enable and start services
log "Starting services..."
systemctl daemon-reload
systemctl enable mira
systemctl start mira
systemctl restart caddy

# Wait for services to start
sleep 3

# Check status
log "Checking service status..."
if systemctl is-active --quiet mira; then
    log "Mira service is running"
else
    error "Mira service failed to start. Check: journalctl -u mira -n 50"
fi

if systemctl is-active --quiet caddy; then
    log "Caddy is running"
else
    error "Caddy failed to start. Check: journalctl -u caddy -n 50"
fi

# Print success message
echo ""
echo "======================================"
echo -e "${GREEN}Mira Setup Complete!${NC}"
echo "======================================"
echo ""
echo "MCP Endpoint: https://$DOMAIN/mcp"
echo "Health Check: https://$DOMAIN/health"
echo ""
if [ -n "$AUTH_TOKEN" ]; then
    echo "Authentication: Bearer token required"
    echo "Token: $AUTH_TOKEN"
    echo ""
    echo "To connect from Claude.ai:"
    echo "  1. Go to Claude.ai Settings > Connections"
    echo "  2. Add a new MCP connection"
    echo "  3. URL: https://$DOMAIN/mcp"
    echo "  4. Add header: Authorization: Bearer $AUTH_TOKEN"
else
    echo "Authentication: None (public access)"
    echo ""
    echo "To connect from Claude.ai:"
    echo "  1. Go to Claude.ai Settings > Connections"
    echo "  2. Add a new MCP connection"
    echo "  3. URL: https://$DOMAIN/mcp"
    echo ""
    warn "Consider re-running with an auth token for security:"
    echo "  sudo ./setup-ubuntu.sh $DOMAIN your-secret-token"
fi
echo ""
echo "Important next steps:"
echo "  1. Add your OPENAI_API_KEY to $MIRA_DIR/.env"
echo "  2. Restart mira: sudo systemctl restart mira"
echo ""
echo "Useful commands:"
echo "  View logs:     journalctl -u mira -f"
echo "  Restart:       sudo systemctl restart mira"
echo "  Check status:  sudo systemctl status mira"
echo ""
