#!/bin/bash
# build-studio.sh
# Build Mira Studio - single binary with embedded WASM frontend
#
# Usage:
#   ./build-studio.sh          # Standard build (wasm-pack + cargo)
#   ./build-studio.sh --leptos # Use cargo-leptos (requires: cargo install cargo-leptos)
#   ./build-studio.sh --watch  # Development mode with hot reload (cargo-leptos only)
#
# IMPORTANT: Build order matters for single binary packaging!
# 1. WASM must be built FIRST (into pkg/)
# 2. Server binary embeds assets/ and pkg/ at compile time via rust-embed

set -e

# Check for cargo-leptos mode
USE_LEPTOS=false
WATCH_MODE=false

for arg in "$@"; do
    case $arg in
        --leptos)
            USE_LEPTOS=true
            ;;
        --watch)
            WATCH_MODE=true
            USE_LEPTOS=true
            ;;
    esac
done

echo "Building Mira Studio (single binary)..."

if [ "$USE_LEPTOS" = true ]; then
    # cargo-leptos build
    if ! command -v cargo-leptos &> /dev/null; then
        echo ""
        echo "ERROR: cargo-leptos not found. Install with:"
        echo "  cargo install cargo-leptos"
        echo ""
        echo "Or use standard build:"
        echo "  ./build-studio.sh"
        exit 1
    fi

    if [ "$WATCH_MODE" = true ]; then
        echo ""
        echo ">> Starting development server with hot reload..."
        cargo leptos watch
    else
        echo ""
        echo ">> Building with cargo-leptos..."
        cargo leptos build --release
    fi

    BINARY="./target/release/mira"
else
    # Standard build (wasm-pack + cargo)

    # Step 1: Build the WASM frontend FIRST
    echo ""
    echo ">> Step 1: Building WASM frontend..."
    wasm-pack build --target web crates/mira-app --out-dir ../../pkg

    # Step 2: Build the server (embeds WASM at compile time)
    echo ""
    echo ">> Step 2: Building server with embedded assets..."
    cargo build --release -p mira-server

    BINARY="./target/release/mira"
fi

# Show binary size
if [ -f "$BINARY" ]; then
    SIZE=$(du -h "$BINARY" | cut -f1)
    echo ""
    echo "Build complete!"
    echo "  Binary: $BINARY ($SIZE)"
    echo ""
    echo "The binary is self-contained - no external files needed."
    echo ""
    echo "To run:"
    echo "  $BINARY web --port 3000"
    echo ""
    echo "Then open http://localhost:3000"
fi
