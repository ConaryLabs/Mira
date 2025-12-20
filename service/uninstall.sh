#!/bin/bash
# Mira Daemon Uninstallation Script (Linux)

set -e

INSTALL_DIR="/usr/local/bin"
SERVICE_DIR="$HOME/.config/systemd/user"

echo "=== Mira Daemon Uninstallation ==="
echo ""

# Check if running as root
if [ "$EUID" -eq 0 ]; then
    echo "Error: Don't run this script as root."
    exit 1
fi

# Stop service if running
echo "Stopping mira service..."
systemctl --user stop mira.service 2>/dev/null || true

# Disable service
echo "Disabling mira service..."
systemctl --user disable mira.service 2>/dev/null || true

# Remove service file
echo "Removing service file..."
rm -f "$SERVICE_DIR/mira.service"

# Reload systemd
echo "Reloading systemd..."
systemctl --user daemon-reload

# Remove binary (needs sudo)
echo "Removing binary (requires sudo)..."
sudo rm -f "$INSTALL_DIR/mira"

echo ""
echo "=== Uninstallation Complete ==="
echo ""
echo "Note: ~/.mira directory was NOT removed (contains your data)."
echo "To remove data: rm -rf ~/.mira"
