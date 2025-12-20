#!/bin/bash
# Mira Daemon Installation Script (Linux)
# Installs mira as a systemd user service

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
INSTALL_DIR="/usr/local/bin"
SERVICE_DIR="$HOME/.config/systemd/user"

echo "=== Mira Daemon Installation ==="
echo ""

# Check if running as root (we don't want that for user service)
if [ "$EUID" -eq 0 ]; then
    echo "Error: Don't run this script as root."
    echo "The service will be installed as a user service under your account."
    exit 1
fi

# Build release binary
echo "Building release binary..."
cd "$PROJECT_DIR"
cargo build --release

# Install binary (needs sudo)
echo ""
echo "Installing binary to $INSTALL_DIR (requires sudo)..."
sudo cp "$PROJECT_DIR/target/release/mira" "$INSTALL_DIR/mira"
sudo chmod +x "$INSTALL_DIR/mira"

# Create mira data directory
echo "Creating ~/.mira directory..."
mkdir -p "$HOME/.mira"

# Install systemd service
echo "Installing systemd user service..."
mkdir -p "$SERVICE_DIR"
cp "$SCRIPT_DIR/mira.service" "$SERVICE_DIR/mira.service"

# Reload systemd
echo "Reloading systemd..."
systemctl --user daemon-reload

# Enable service (start on login)
echo "Enabling mira service..."
systemctl --user enable mira.service

# Enable lingering (keeps user services running after logout)
echo "Enabling lingering (keeps service running after logout)..."
sudo loginctl enable-linger "$USER"

# Start service
echo "Starting mira service..."
systemctl --user start mira.service

# Wait a moment for startup
sleep 2

# Check status
echo ""
echo "=== Installation Complete ==="
echo ""
systemctl --user status mira.service --no-pager || true
echo ""
echo "Mira daemon is now running on port 3199"
echo ""
echo "Useful commands:"
echo "  mira status                    # Check daemon status"
echo "  systemctl --user status mira   # View service status"
echo "  systemctl --user restart mira  # Restart daemon"
echo "  systemctl --user stop mira     # Stop daemon"
echo "  journalctl --user -u mira -f   # View logs"
echo ""
echo "Claude Code config (~/.claude/settings.local.json):"
echo '  {'
echo '    "mcpServers": {'
echo '      "mira": {'
echo '        "command": "/usr/local/bin/mira",'
echo '        "args": ["connect"]'
echo '      }'
echo '    }'
echo '  }'
