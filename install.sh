#!/bin/bash
# install.sh
# Mira Installation Script for Ubuntu 24.04
# Usage: sudo ./install.sh

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
QDRANT_VERSION="v1.12.1"
NODE_VERSION="20"
MIRA_PATH="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Helper functions
print_step() {
    echo -e "\n${BLUE}==>${NC} ${GREEN}$1${NC}"
}

print_warning() {
    echo -e "${YELLOW}Warning:${NC} $1"
}

print_error() {
    echo -e "${RED}Error:${NC} $1"
}

print_success() {
    echo -e "${GREEN}$1${NC}"
}

check_command() {
    if ! command -v "$1" &> /dev/null; then
        return 1
    fi
    return 0
}

# ============================================================================
# Step 1: Pre-flight Checks
# ============================================================================
print_step "Step 1/12: Pre-flight checks"

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    print_error "This script must be run as root (use sudo ./install.sh)"
    exit 1
fi

# Get the actual user (not root)
if [ -n "$SUDO_USER" ]; then
    MIRA_USER="$SUDO_USER"
else
    MIRA_USER="$(whoami)"
fi

echo "Installation path: $MIRA_PATH"
echo "Running as user: $MIRA_USER"

# Check Ubuntu version
if [ -f /etc/os-release ]; then
    . /etc/os-release
    if [ "$ID" = "ubuntu" ]; then
        echo "Detected: Ubuntu $VERSION_ID"
        if [[ "$VERSION_ID" != "24.04"* ]]; then
            print_warning "This script is designed for Ubuntu 24.04. Your version: $VERSION_ID"
            read -p "Continue anyway? [y/N] " -n 1 -r
            echo
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                exit 1
            fi
        fi
    else
        print_warning "This script is designed for Ubuntu. Detected: $ID"
        read -p "Continue anyway? [y/N] " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    fi
fi

print_success "Pre-flight checks passed"

# ============================================================================
# Step 2: System Dependencies
# ============================================================================
print_step "Step 2/12: Installing system dependencies"

apt-get update
apt-get install -y \
    build-essential \
    curl \
    git \
    libgit2-dev \
    sqlite3 \
    libsqlite3-dev \
    libssl-dev \
    pkg-config \
    nginx

print_success "System dependencies installed"

# ============================================================================
# Step 3: Rust Installation
# ============================================================================
print_step "Step 3/12: Installing Rust"

if check_command rustc; then
    RUST_VERSION=$(rustc --version | cut -d' ' -f2)
    echo "Rust already installed: $RUST_VERSION"
else
    echo "Installing Rust via rustup..."
    sudo -u "$MIRA_USER" bash -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'
fi

# Source cargo environment for this script
export PATH="$PATH:/home/$MIRA_USER/.cargo/bin"
if [ -f "/home/$MIRA_USER/.cargo/env" ]; then
    source "/home/$MIRA_USER/.cargo/env"
fi

# Verify Rust installation
if ! check_command rustc; then
    print_error "Rust installation failed. Please install manually: https://rustup.rs"
    exit 1
fi

RUST_VERSION=$(rustc --version | cut -d' ' -f2)
echo "Rust version: $RUST_VERSION"
print_success "Rust installed"

# ============================================================================
# Step 4: Node.js Installation
# ============================================================================
print_step "Step 4/12: Installing Node.js"

if check_command node; then
    NODE_CURRENT=$(node --version | cut -d'v' -f2 | cut -d'.' -f1)
    if [ "$NODE_CURRENT" -ge 18 ]; then
        echo "Node.js already installed: $(node --version)"
    else
        print_warning "Node.js version too old. Installing Node.js $NODE_VERSION..."
        curl -fsSL https://deb.nodesource.com/setup_${NODE_VERSION}.x | bash -
        apt-get install -y nodejs
    fi
else
    echo "Installing Node.js $NODE_VERSION..."
    curl -fsSL https://deb.nodesource.com/setup_${NODE_VERSION}.x | bash -
    apt-get install -y nodejs
fi

echo "Node.js version: $(node --version)"
echo "npm version: $(npm --version)"
print_success "Node.js installed"

# ============================================================================
# Step 5: Qdrant Download
# ============================================================================
print_step "Step 5/12: Downloading Qdrant"

QDRANT_BIN="$MIRA_PATH/backend/bin/qdrant"
mkdir -p "$MIRA_PATH/backend/bin"

if [ -f "$QDRANT_BIN" ]; then
    echo "Qdrant binary already exists at $QDRANT_BIN"
else
    # Detect architecture
    ARCH=$(uname -m)
    case $ARCH in
        x86_64)
            QDRANT_ARCH="x86_64-unknown-linux-gnu"
            ;;
        aarch64)
            QDRANT_ARCH="aarch64-unknown-linux-gnu"
            ;;
        *)
            print_error "Unsupported architecture: $ARCH"
            exit 1
            ;;
    esac

    QDRANT_URL="https://github.com/qdrant/qdrant/releases/download/${QDRANT_VERSION}/qdrant-${QDRANT_ARCH}.tar.gz"
    echo "Downloading Qdrant ${QDRANT_VERSION} for ${ARCH}..."
    echo "URL: $QDRANT_URL"

    TEMP_DIR=$(mktemp -d)
    curl -L "$QDRANT_URL" -o "$TEMP_DIR/qdrant.tar.gz"
    tar -xzf "$TEMP_DIR/qdrant.tar.gz" -C "$TEMP_DIR"
    mv "$TEMP_DIR/qdrant" "$QDRANT_BIN"
    rm -rf "$TEMP_DIR"
fi

chmod +x "$QDRANT_BIN"
chown "$MIRA_USER:$MIRA_USER" "$QDRANT_BIN"

echo "Qdrant binary: $QDRANT_BIN"
print_success "Qdrant downloaded"

# ============================================================================
# Step 6: Environment Setup
# ============================================================================
print_step "Step 6/12: Setting up environment"

ENV_FILE="$MIRA_PATH/backend/.env"
ENV_EXAMPLE="$MIRA_PATH/backend/.env.example"

if [ -f "$ENV_FILE" ]; then
    echo "Environment file already exists: $ENV_FILE"
    read -p "Overwrite? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        echo "Keeping existing .env file"
    else
        cp "$ENV_EXAMPLE" "$ENV_FILE"
    fi
else
    cp "$ENV_EXAMPLE" "$ENV_FILE"
fi

# Prompt for OpenAI API key
echo ""
echo "Mira requires an OpenAI API key for GPT 5.1 and embeddings."
echo "Get your API key from: https://platform.openai.com/api-keys"
echo ""

while true; do
    read -p "Enter your OpenAI API key (sk-...): " OPENAI_KEY
    if [[ "$OPENAI_KEY" == sk-* ]]; then
        break
    else
        print_warning "Invalid API key format. It should start with 'sk-'"
    fi
done

# Update .env file with the API key
sed -i "s|OPENAI_API_KEY=.*|OPENAI_API_KEY=$OPENAI_KEY|" "$ENV_FILE"

# Set correct ownership
chown "$MIRA_USER:$MIRA_USER" "$ENV_FILE"
chmod 600 "$ENV_FILE"

print_success "Environment configured"

# ============================================================================
# Step 7: Database Setup
# ============================================================================
print_step "Step 7/12: Setting up database"

mkdir -p "$MIRA_PATH/backend/data"
chown -R "$MIRA_USER:$MIRA_USER" "$MIRA_PATH/backend/data"

print_success "Database directory created"

# ============================================================================
# Step 8: Build Phase
# ============================================================================
print_step "Step 8/12: Building application"

echo "Building backend (this may take a few minutes)..."
cd "$MIRA_PATH/backend"
sudo -u "$MIRA_USER" bash -c "source /home/$MIRA_USER/.cargo/env && cargo build --release"

echo "Building frontend..."
cd "$MIRA_PATH/frontend"
sudo -u "$MIRA_USER" npm install
sudo -u "$MIRA_USER" npm run build

print_success "Application built"

# ============================================================================
# Step 9: Nginx Configuration
# ============================================================================
print_step "Step 9/12: Configuring Nginx"

# Remove default site
rm -f /etc/nginx/sites-enabled/default

# Copy and configure mira nginx config
NGINX_CONF="/etc/nginx/sites-available/mira"
cp "$MIRA_PATH/scripts/mira-nginx.conf" "$NGINX_CONF"

# Replace placeholder with actual path
sed -i "s|MIRA_PATH|$MIRA_PATH|g" "$NGINX_CONF"

# Enable the site
ln -sf "$NGINX_CONF" /etc/nginx/sites-enabled/mira

# Test nginx configuration
nginx -t

print_success "Nginx configured"

# ============================================================================
# Step 10: Systemd Services
# ============================================================================
print_step "Step 10/12: Installing systemd services"

# Copy and configure qdrant service
QDRANT_SERVICE="/etc/systemd/system/mira-qdrant.service"
cp "$MIRA_PATH/scripts/mira-qdrant.service" "$QDRANT_SERVICE"
sed -i "s|MIRA_PATH|$MIRA_PATH|g" "$QDRANT_SERVICE"
sed -i "s|MIRA_USER|$MIRA_USER|g" "$QDRANT_SERVICE"

# Copy and configure backend service
BACKEND_SERVICE="/etc/systemd/system/mira-backend.service"
cp "$MIRA_PATH/scripts/mira-backend.service" "$BACKEND_SERVICE"
sed -i "s|MIRA_PATH|$MIRA_PATH|g" "$BACKEND_SERVICE"
sed -i "s|MIRA_USER|$MIRA_USER|g" "$BACKEND_SERVICE"

# Reload systemd
systemctl daemon-reload

print_success "Systemd services installed"

# ============================================================================
# Step 11: Default User Creation
# ============================================================================
print_step "Step 11/12: Creating default user account"

echo ""
echo "Create a Mira user account for web login:"
echo ""

read -p "Username: " MIRA_USERNAME
while true; do
    read -s -p "Password: " MIRA_PASSWORD
    echo
    read -s -p "Confirm password: " MIRA_PASSWORD_CONFIRM
    echo
    if [ "$MIRA_PASSWORD" = "$MIRA_PASSWORD_CONFIRM" ]; then
        break
    else
        print_warning "Passwords don't match. Try again."
    fi
done

# Generate bcrypt hash using Python (available on Ubuntu)
MIRA_PASSWORD_HASH=$(python3 -c "import bcrypt; print(bcrypt.hashpw('$MIRA_PASSWORD'.encode(), bcrypt.gensalt()).decode())" 2>/dev/null || echo "")

if [ -z "$MIRA_PASSWORD_HASH" ]; then
    # Install bcrypt if not available
    pip3 install bcrypt -q
    MIRA_PASSWORD_HASH=$(python3 -c "import bcrypt; print(bcrypt.hashpw('$MIRA_PASSWORD'.encode(), bcrypt.gensalt()).decode())")
fi

# Start Qdrant temporarily to run migrations
echo "Starting Qdrant temporarily for database setup..."
sudo -u "$MIRA_USER" "$QDRANT_BIN" --config-path "$MIRA_PATH/backend/config/config.yaml" &
QDRANT_PID=$!
sleep 3

# Run backend briefly to create database and run migrations
echo "Running database migrations..."
cd "$MIRA_PATH/backend"
sudo -u "$MIRA_USER" bash -c "source /home/$MIRA_USER/.cargo/env && timeout 10 ./target/release/mira-backend || true"

# Insert user into database
DB_FILE="$MIRA_PATH/backend/data/mira.db"
if [ -f "$DB_FILE" ]; then
    sqlite3 "$DB_FILE" "INSERT OR REPLACE INTO users (id, username, password_hash, created_at, updated_at) VALUES (1, '$MIRA_USERNAME', '$MIRA_PASSWORD_HASH', datetime('now'), datetime('now'));"
    echo "User '$MIRA_USERNAME' created successfully"
else
    print_warning "Database file not found. User will need to be created manually."
fi

# Stop temporary Qdrant
kill $QDRANT_PID 2>/dev/null || true

print_success "Default user created"

# ============================================================================
# Step 12: Start Services
# ============================================================================
print_step "Step 12/12: Starting services"

# Enable services
systemctl enable mira-qdrant
systemctl enable mira-backend
systemctl enable nginx

# Start services
systemctl start mira-qdrant
sleep 3
systemctl start mira-backend
systemctl restart nginx

# Verify services are running
echo ""
echo "Service status:"
systemctl is-active --quiet mira-qdrant && echo "  mira-qdrant: running" || echo "  mira-qdrant: NOT RUNNING"
systemctl is-active --quiet mira-backend && echo "  mira-backend: running" || echo "  mira-backend: NOT RUNNING"
systemctl is-active --quiet nginx && echo "  nginx: running" || echo "  nginx: NOT RUNNING"

# ============================================================================
# Success Message
# ============================================================================
echo ""
echo -e "${GREEN}============================================================================${NC}"
echo -e "${GREEN}                    Mira Installation Complete!${NC}"
echo -e "${GREEN}============================================================================${NC}"
echo ""
echo "Access Mira at: http://localhost"
echo "               or http://$(hostname -I | awk '{print $1}')"
echo ""
echo "Login with:"
echo "  Username: $MIRA_USERNAME"
echo "  Password: (the password you entered)"
echo ""
echo "Useful commands:"
echo "  sudo systemctl status mira-backend   # Check backend status"
echo "  sudo systemctl status mira-qdrant    # Check Qdrant status"
echo "  sudo journalctl -u mira-backend -f   # View backend logs"
echo "  sudo journalctl -u mira-qdrant -f    # View Qdrant logs"
echo ""
echo "Configuration files:"
echo "  Backend: $MIRA_PATH/backend/.env"
echo "  Nginx:   /etc/nginx/sites-available/mira"
echo ""
print_success "Enjoy using Mira!"
