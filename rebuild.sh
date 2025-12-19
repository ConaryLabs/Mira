#!/bin/bash
# Mira Rebuild Script
# Rebuilds backend and frontend, restarts service

set -e

MIRA_DIR="/home/peter/Mira"
cd "$MIRA_DIR"

echo "============================================"
echo "Mira Rebuild Script"
echo "============================================"

# Parse arguments
BUILD_BACKEND=true
BUILD_FRONTEND=true
RESTART_SERVICES=true

while [[ $# -gt 0 ]]; do
    case $1 in
        --backend-only)
            BUILD_FRONTEND=false
            ;;
        --frontend-only)
            BUILD_BACKEND=false
            ;;
        --no-restart)
            RESTART_SERVICES=false
            ;;
        --help)
            echo "Usage: $0 [options]"
            echo "Options:"
            echo "  --backend-only   Only rebuild Mira backend"
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

# Build Mira backend (MCP + Chat + Indexer)
if $BUILD_BACKEND; then
    echo ""
    echo "[1/2] Building Mira backend..."
    cd "$MIRA_DIR"
    SQLX_OFFLINE=true cargo build --release 2>&1 | tail -5
    echo "✓ Mira backend built"
fi

# Build Studio frontend
if $BUILD_FRONTEND; then
    echo ""
    echo "[2/2] Building Studio frontend..."
    cd "$MIRA_DIR/studio"
    npm run build 2>&1 | tail -5
    echo "✓ Studio frontend built"
fi

# Restart services
if $RESTART_SERVICES; then
    echo ""
    echo "Restarting services..."

    if $BUILD_BACKEND; then
        sudo systemctl restart mira
        echo "✓ mira restarted"
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
echo "  mira:  $(sudo systemctl is-active mira 2>/dev/null || echo 'not running')"
echo "  nginx: $(sudo systemctl is-active nginx)"
echo ""
echo "URLs:"
echo "  MCP:    https://mira.conarylabs.com/mcp"
echo "  Studio: https://mira.conarylabs.com/"
echo "  API:    https://mira.conarylabs.com/api/status"
echo "============================================"
