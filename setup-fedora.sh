#!/usr/bin/env bash
#
# Mira + Claude Code Setup Script for Fedora 43
#
# This script sets up a fresh Fedora 43 installation with:
# - Rust toolchain (1.92+) with rust-analyzer
# - wasm-pack for WASM builds
# - Mira (memory + code intelligence MCP server)
# - Claude Code CLI with LSP plugin
# - Full Claude Code configuration matching Peter's setup
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/setup-fedora.sh | bash
#   # OR
#   ./setup-fedora.sh
#
# Environment variables (optional):
#   GEMINI_API_KEY  - For semantic search embeddings (get from https://aistudio.google.com/apikey)
#   MIRA_REPO       - Git URL (default: https://github.com/ConaryLabs/Mira.git)
#   MIRA_DIR        - Install directory (default: ~/Mira)
#

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() { echo -e "${BLUE}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# Configuration
MIRA_REPO="${MIRA_REPO:-https://github.com/ConaryLabs/Mira.git}"
MIRA_DIR="${MIRA_DIR:-$HOME/Mira}"
RUST_VERSION="1.92.0"

# ============================================================================
# Phase 1: System Dependencies
# ============================================================================
install_system_deps() {
    log "Installing system dependencies..."

    sudo dnf install -y \
        gcc \
        gcc-c++ \
        make \
        cmake \
        git \
        curl \
        pkg-config \
        openssl-devel \
        sqlite-devel \
        zlib-devel \
        perl \
        jq \
        || error "Failed to install system dependencies"

    success "System dependencies installed"
}

# ============================================================================
# Phase 2: Rust Toolchain + rust-analyzer + wasm-pack
# ============================================================================
install_rust() {
    if command -v rustc &> /dev/null; then
        local current_version
        current_version=$(rustc --version | awk '{print $2}')
        log "Rust $current_version already installed"

        # Check if version is sufficient (1.92+)
        if [[ "$(printf '%s\n' "$RUST_VERSION" "$current_version" | sort -V | head -n1)" == "$RUST_VERSION" ]]; then
            success "Rust version is sufficient"
        else
            warn "Rust version $current_version is too old, updating..."
            rustup update stable
        fi
    else
        log "Installing Rust via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
        source "$HOME/.cargo/env"
    fi

    # Ensure cargo is in PATH
    if [[ -f "$HOME/.cargo/env" ]]; then
        source "$HOME/.cargo/env"
    fi

    # Install rust-analyzer component
    log "Installing rust-analyzer..."
    rustup component add rust-analyzer 2>/dev/null || {
        # Fallback: install from GitHub releases
        warn "rustup component not available, installing from GitHub..."
        curl -L https://github.com/rust-lang/rust-analyzer/releases/latest/download/rust-analyzer-x86_64-unknown-linux-gnu.gz | gunzip -c - > ~/.cargo/bin/rust-analyzer
        chmod +x ~/.cargo/bin/rust-analyzer
    }

    # Install wasm-pack for WASM builds
    log "Installing wasm-pack..."
    if ! command -v wasm-pack &> /dev/null; then
        curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh
    fi

    success "Rust toolchain installed: $(rustc --version)"
    success "rust-analyzer: $(rust-analyzer --version 2>/dev/null || echo 'installed')"
    success "wasm-pack: $(wasm-pack --version 2>/dev/null || echo 'installed')"
}

# ============================================================================
# Phase 3: Clone and Build Mira
# ============================================================================
build_mira() {
    # Ensure cargo is in PATH
    if [[ -f "$HOME/.cargo/env" ]]; then
        source "$HOME/.cargo/env"
    fi

    if [[ -d "$MIRA_DIR" ]]; then
        log "Mira directory exists, updating..."
        cd "$MIRA_DIR"
        git pull --ff-only || warn "Could not pull updates (maybe local changes?)"
    else
        log "Cloning Mira repository..."
        git clone "$MIRA_REPO" "$MIRA_DIR"
        cd "$MIRA_DIR"
    fi

    log "Building Mira (release mode)..."
    cargo build --release || error "Failed to build Mira"

    # Verify binary exists
    if [[ -x "$MIRA_DIR/target/release/mira" ]]; then
        success "Mira built successfully: $MIRA_DIR/target/release/mira"
    else
        error "Mira binary not found after build"
    fi

    # Build WASM frontend (optional, for Studio)
    if command -v wasm-pack &> /dev/null; then
        log "Building WASM frontend..."
        if [[ -f "$MIRA_DIR/build-studio.sh" ]]; then
            bash "$MIRA_DIR/build-studio.sh" || warn "WASM build failed (optional)"
        fi
    fi
}

# ============================================================================
# Phase 4: Install Claude Code
# ============================================================================
install_claude_code() {
    if command -v claude &> /dev/null; then
        local version
        version=$(claude --version 2>/dev/null | head -1)
        success "Claude Code already installed: $version"
        return 0
    fi

    log "Installing Claude Code..."

    # Claude Code native installer
    curl -fsSL https://claude.ai/install.sh | sh

    # Add to PATH if needed
    if [[ -d "$HOME/.local/bin" ]] && [[ ":$PATH:" != *":$HOME/.local/bin:"* ]]; then
        export PATH="$HOME/.local/bin:$PATH"
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
    fi

    if command -v claude &> /dev/null; then
        success "Claude Code installed: $(claude --version 2>/dev/null | head -1)"
    else
        error "Claude Code installation failed"
    fi
}

# ============================================================================
# Phase 5: Configure Claude Code with Mira MCP + LSP Plugin
# ============================================================================
configure_claude_code() {
    log "Configuring Claude Code..."

    local claude_dir="$HOME/.claude"
    local plugins_dir="$claude_dir/plugins"
    mkdir -p "$claude_dir" "$plugins_dir"

    # -------------------------------------------------------------------------
    # Project-level MCP config (.mcp.json in Mira directory)
    # -------------------------------------------------------------------------
    cat > "$MIRA_DIR/.mcp.json" << EOF
{
  "mcpServers": {
    "mira": {
      "command": "$MIRA_DIR/target/release/mira",
      "args": ["serve"]
    }
  }
}
EOF
    success "Created $MIRA_DIR/.mcp.json"

    # -------------------------------------------------------------------------
    # LSP configuration (cclsp.json in Mira directory)
    # -------------------------------------------------------------------------
    cat > "$MIRA_DIR/cclsp.json" << 'EOF'
{
  "servers": [
    {
      "extensions": ["rs"],
      "command": ["rust-analyzer"],
      "rootDir": "."
    }
  ]
}
EOF
    success "Created $MIRA_DIR/cclsp.json (LSP config)"

    # -------------------------------------------------------------------------
    # Claude Code settings.json (global settings)
    # -------------------------------------------------------------------------
    cat > "$claude_dir/settings.json" << EOF
{
  "enabledMcpjsonServers": ["mira"],
  "hooks": {
    "PermissionRequest": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "$MIRA_DIR/target/release/mira hook permission",
            "timeout": 3000
          }
        ]
      }
    ]
  },
  "alwaysThinkingEnabled": true,
  "enabledPlugins": {
    "rust-analyzer-lsp@claude-plugins-official": true
  }
}
EOF
    success "Created $claude_dir/settings.json"

    # -------------------------------------------------------------------------
    # Claude Code settings.local.json (local overrides)
    # -------------------------------------------------------------------------
    cat > "$claude_dir/settings.local.json" << 'EOF'
{
  "enableAllProjectMcpServers": true,
  "enabledMcpjsonServers": ["mira"]
}
EOF
    success "Created $claude_dir/settings.local.json"

    # -------------------------------------------------------------------------
    # Install rust-analyzer-lsp plugin from official marketplace
    # -------------------------------------------------------------------------
    log "Installing rust-analyzer-lsp plugin..."

    # Create known_marketplaces.json
    cat > "$plugins_dir/known_marketplaces.json" << 'EOF'
{
  "claude-plugins-official": {
    "source": {
      "source": "github",
      "repo": "anthropics/claude-plugins-official"
    },
    "installLocation": "",
    "lastUpdated": ""
  }
}
EOF

    # Clone the official plugins marketplace
    local marketplace_dir="$plugins_dir/marketplaces/claude-plugins-official"
    if [[ -d "$marketplace_dir" ]]; then
        log "Updating official plugins marketplace..."
        cd "$marketplace_dir"
        git pull --ff-only || warn "Could not update marketplace"
    else
        log "Cloning official plugins marketplace..."
        mkdir -p "$plugins_dir/marketplaces"
        git clone https://github.com/anthropics/claude-plugins-official.git "$marketplace_dir"
    fi

    # Update known_marketplaces.json with correct path
    cat > "$plugins_dir/known_marketplaces.json" << EOF
{
  "claude-plugins-official": {
    "source": {
      "source": "github",
      "repo": "anthropics/claude-plugins-official"
    },
    "installLocation": "$marketplace_dir",
    "lastUpdated": "$(date -Iseconds)"
  }
}
EOF

    # Copy the plugin to cache
    local plugin_cache="$plugins_dir/cache/claude-plugins-official/rust-analyzer-lsp/1.0.0"
    mkdir -p "$plugin_cache"
    if [[ -d "$marketplace_dir/rust-analyzer-lsp" ]]; then
        cp -r "$marketplace_dir/rust-analyzer-lsp/"* "$plugin_cache/"
    fi

    # Create installed_plugins.json
    cat > "$plugins_dir/installed_plugins.json" << EOF
{
  "version": 2,
  "plugins": {
    "rust-analyzer-lsp@claude-plugins-official": [
      {
        "scope": "user",
        "installPath": "$plugin_cache",
        "version": "1.0.0",
        "installedAt": "$(date -Iseconds)",
        "lastUpdated": "$(date -Iseconds)",
        "isLocal": true
      }
    ]
  }
}
EOF
    success "Installed rust-analyzer-lsp plugin"
}

# ============================================================================
# Phase 6: Environment Setup (API Keys)
# ============================================================================
setup_environment() {
    log "Setting up environment..."

    local env_file="$HOME/.config/environment.d/mira.conf"
    mkdir -p "$(dirname "$env_file")"

    if [[ -n "${GEMINI_API_KEY:-}" ]]; then
        echo "GEMINI_API_KEY=$GEMINI_API_KEY" > "$env_file"
        success "Saved GEMINI_API_KEY to $env_file"

        # Also add to bashrc for immediate use
        if ! grep -q "GEMINI_API_KEY" "$HOME/.bashrc" 2>/dev/null; then
            echo "export GEMINI_API_KEY=\"$GEMINI_API_KEY\"" >> "$HOME/.bashrc"
        fi
    else
        warn "GEMINI_API_KEY not set. Semantic search will be disabled."
        warn "Get a key from: https://aistudio.google.com/apikey"
        warn "Then add to ~/.bashrc: export GEMINI_API_KEY=your_key_here"
    fi
}

# ============================================================================
# Phase 7: Verify Installation
# ============================================================================
verify_installation() {
    log "Verifying installation..."

    local errors=0

    # Ensure cargo is in PATH
    if [[ -f "$HOME/.cargo/env" ]]; then
        source "$HOME/.cargo/env"
    fi

    # Check Rust
    if command -v rustc &> /dev/null; then
        success "Rust: $(rustc --version)"
    else
        warn "Rust not found"
        ((errors++))
    fi

    # Check rust-analyzer
    if command -v rust-analyzer &> /dev/null; then
        success "rust-analyzer: $(rust-analyzer --version 2>/dev/null || echo 'available')"
    else
        warn "rust-analyzer not found"
        ((errors++))
    fi

    # Check wasm-pack
    if command -v wasm-pack &> /dev/null; then
        success "wasm-pack: $(wasm-pack --version)"
    else
        warn "wasm-pack not found"
        ((errors++))
    fi

    # Check Mira binary
    if [[ -x "$MIRA_DIR/target/release/mira" ]]; then
        success "Mira: $($MIRA_DIR/target/release/mira --version 2>/dev/null || echo 'built')"
    else
        warn "Mira binary not found"
        ((errors++))
    fi

    # Check Claude Code
    if command -v claude &> /dev/null; then
        success "Claude Code: $(claude --version 2>/dev/null | head -1)"
    else
        warn "Claude Code not found in PATH"
        ((errors++))
    fi

    # Check MCP config
    if [[ -f "$MIRA_DIR/.mcp.json" ]]; then
        success "MCP config: $MIRA_DIR/.mcp.json"
    else
        warn "MCP config missing"
        ((errors++))
    fi

    # Check LSP config
    if [[ -f "$MIRA_DIR/cclsp.json" ]]; then
        success "LSP config: $MIRA_DIR/cclsp.json"
    else
        warn "LSP config missing"
        ((errors++))
    fi

    # Check Claude settings
    if [[ -f "$HOME/.claude/settings.json" ]]; then
        success "Claude settings: ~/.claude/settings.json"
    else
        warn "Claude settings missing"
        ((errors++))
    fi

    # Check plugin
    if [[ -f "$HOME/.claude/plugins/installed_plugins.json" ]]; then
        success "LSP plugin: rust-analyzer-lsp installed"
    else
        warn "LSP plugin not installed"
        ((errors++))
    fi

    if [[ $errors -gt 0 ]]; then
        warn "Installation completed with $errors warnings"
    else
        success "All components installed successfully!"
    fi
}

# ============================================================================
# Main
# ============================================================================
main() {
    echo ""
    echo "╔════════════════════════════════════════════════════════════════╗"
    echo "║           Mira + Claude Code Setup for Fedora 43               ║"
    echo "╚════════════════════════════════════════════════════════════════╝"
    echo ""

    install_system_deps
    install_rust
    build_mira
    install_claude_code
    configure_claude_code
    setup_environment
    verify_installation

    echo ""
    echo "════════════════════════════════════════════════════════════════════"
    echo ""
    success "Setup complete!"
    echo ""
    echo "What's installed:"
    echo "  • Rust $(rustc --version 2>/dev/null | awk '{print $2}' || echo '1.92+')"
    echo "  • rust-analyzer (LSP for Rust)"
    echo "  • wasm-pack (for WASM builds)"
    echo "  • Mira MCP server"
    echo "  • Claude Code with:"
    echo "    - Mira MCP integration"
    echo "    - rust-analyzer-lsp plugin"
    echo "    - Permission hooks"
    echo "    - Always-thinking enabled"
    echo ""
    echo "Next steps:"
    echo "  1. Open a new terminal (or run: source ~/.bashrc)"
    echo "  2. cd $MIRA_DIR"
    echo "  3. claude"
    echo ""
    if [[ -z "${GEMINI_API_KEY:-}" ]]; then
        echo "Optional: Set GEMINI_API_KEY for semantic search:"
        echo "  export GEMINI_API_KEY=your_key_here"
        echo "  (Get a key from https://aistudio.google.com/apikey)"
        echo ""
    fi
}

main "$@"
