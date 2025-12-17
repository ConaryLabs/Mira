#!/bin/bash
# Mira Rebuild Script
# Rebuilds all components and restarts services

set -e

MIRA_DIR="/home/peter/Mira"
cd "$MIRA_DIR"

echo "============================================"
echo "Mira Rebuild Script"
echo "============================================"

# Parse arguments
BUILD_BACKEND=true
BUILD_CHAT=true
BUILD_FRONTEND=true
RESTART_SERVICES=true

while [[ $# -gt 0 ]]; do
    case $1 in
        --backend-only)
            BUILD_CHAT=false
            BUILD_FRONTEND=false
            ;;
        --chat-only)
            BUILD_BACKEND=false
            BUILD_FRONTEND=false
            ;;
        --frontend-only)
            BUILD_BACKEND=false
            BUILD_CHAT=false
            ;;
        --no-restart)
            RESTART_SERVICES=false
            ;;
        --help)
            echo "Usage: $0 [options]"
            echo "Options:"
            echo "  --backend-only   Only rebuild Mira MCP backend"
            echo "  --chat-only      Only rebuild mira-chat backend"
            echo "  --frontend-only  Only rebuild Studio frontend"
            echo "  --no-restart     Don't restart services after build"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
    shift
done

# Build Mira MCP backend
if $BUILD_BACKEND; then
    echo ""
    echo "[1/3] Building Mira MCP backend..."
    cd "$MIRA_DIR"
    SQLX_OFFLINE=true cargo build --release 2>&1 | tail -5
    echo "✓ Mira backend built"
fi

# Build mira-chat backend (part of workspace, builds to main target dir)
if $BUILD_CHAT; then
    echo ""
    echo "[2/3] Building mira-chat backend..."
    cd "$MIRA_DIR"
    SQLX_OFFLINE=true cargo build --release -p mira-chat 2>&1 | tail -5
    echo "✓ mira-chat built"
fi

# Build Studio frontend
if $BUILD_FRONTEND; then
    echo ""
    echo "[3/3] Building Studio frontend..."
    cd "$MIRA_DIR/studio"
    npm run build 2>&1 | tail -5
    echo "✓ Studio frontend built"
fi

# Restart services
if $RESTART_SERVICES; then
    echo ""
    echo "Restarting services..."

    if $BUILD_BACKEND; then
        sudo systemctl restart mira-http
        echo "✓ mira-http restarted"
    fi

    if $BUILD_CHAT; then
        sudo systemctl restart mira-chat 2>/dev/null || echo "  (mira-chat service not installed yet - run: sudo cp mira-chat.service /etc/systemd/system/ && sudo systemctl daemon-reload && sudo systemctl enable mira-chat)"
    fi

    # Nginx reload for frontend changes
    if $BUILD_FRONTEND; then
        sudo systemctl reload nginx
        echo "✓ nginx reloaded"
    fi
fi

echo ""
echo "============================================"
echo "Build complete!"
echo ""
echo "Services:"
echo "  mira-http: $(sudo systemctl is-active mira-http 2>/dev/null || echo 'not running')"
echo "  mira-chat: $(sudo systemctl is-active mira-chat 2>/dev/null || echo 'not installed')"
echo "  nginx:     $(sudo systemctl is-active nginx)"
echo ""
echo "URLs:"
echo "  MCP:    https://mira.conarylabs.com/mcp"
echo "  Studio: https://mira.conarylabs.com/"
echo "  API:    https://mira.conarylabs.com/api/status"
echo "============================================"
