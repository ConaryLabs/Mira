#!/bin/bash
# Build Mira Studio - single binary with embedded WASM frontend
#
# IMPORTANT: Build order matters for single binary packaging!
# 1. WASM must be built FIRST (into pkg/)
# 2. Server binary embeds assets/ and pkg/ at compile time via rust-embed

set -e

echo "Building Mira Studio (single binary)..."

# Step 1: Build the WASM frontend FIRST
echo ""
echo ">> Step 1: Building WASM frontend..."
wasm-pack build --target web crates/mira-app --out-dir ../../pkg

# Step 2: Build the server (embeds WASM at compile time)
echo ""
echo ">> Step 2: Building server with embedded assets..."
cargo build --release -p mira-server

# Show binary size
BINARY="./target/release/mira"
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
