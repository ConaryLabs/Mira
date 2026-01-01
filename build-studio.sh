#!/bin/bash
# Build Mira Studio - server + WASM frontend

set -e

echo "Building Mira Studio..."

# Build the WASM frontend
echo ">> Building WASM frontend..."
wasm-pack build --target web crates/mira-app --out-dir ../../pkg

# Build the server
echo ">> Building server..."
cargo build --release -p mira-server

echo ""
echo "Build complete!"
echo ""
echo "To run the web server:"
echo "  ./target/release/mira web --port 3000"
echo ""
echo "Then open http://localhost:3000 in your browser"
