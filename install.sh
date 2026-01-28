#!/bin/bash
set -e

# Mira installer
# Usage: curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash

REPO="ConaryLabs/Mira"
INSTALL_DIR="${MIRA_INSTALL_DIR:-/usr/local/bin}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

info() { echo -e "${GREEN}==>${NC} $1"; }
warn() { echo -e "${YELLOW}warning:${NC} $1"; }
error() { echo -e "${RED}error:${NC} $1"; exit 1; }

# Detect OS and architecture
detect_platform() {
    local os arch

    case "$(uname -s)" in
        Linux*)  os="unknown-linux-gnu" ;;
        Darwin*) os="apple-darwin" ;;
        MINGW*|MSYS*|CYGWIN*) os="pc-windows-msvc" ;;
        *) error "Unsupported operating system: $(uname -s)" ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64) arch="x86_64" ;;
        arm64|aarch64)
            if [ "$os" = "apple-darwin" ]; then
                arch="aarch64"
            else
                error "ARM64 Linux is not yet supported"
            fi
            ;;
        *) error "Unsupported architecture: $(uname -m)" ;;
    esac

    echo "${arch}-${os}"
}

# Get latest release version
get_latest_version() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"([^"]+)".*/\1/'
}

# Download and install binary
install_binary() {
    local platform="$1"
    local version="$2"
    local ext="tar.gz"
    local tmp_dir

    if [[ "$platform" == *"windows"* ]]; then
        ext="zip"
    fi

    local url="https://github.com/${REPO}/releases/download/${version}/mira-${platform}.${ext}"

    info "Downloading mira ${version} for ${platform}..."

    tmp_dir=$(mktemp -d)
    trap "rm -rf $tmp_dir" EXIT

    if [[ "$ext" == "zip" ]]; then
        curl -fsSL "$url" -o "$tmp_dir/mira.zip"
        unzip -q "$tmp_dir/mira.zip" -d "$tmp_dir"
    else
        curl -fsSL "$url" | tar -xz -C "$tmp_dir"
    fi

    # Check if we need sudo
    if [ -w "$INSTALL_DIR" ]; then
        mv "$tmp_dir/mira" "$INSTALL_DIR/mira"
        chmod +x "$INSTALL_DIR/mira"
    else
        info "Installing to $INSTALL_DIR (requires sudo)..."
        sudo mv "$tmp_dir/mira" "$INSTALL_DIR/mira"
        sudo chmod +x "$INSTALL_DIR/mira"
    fi

    info "Installed mira to $INSTALL_DIR/mira"
}

# Install Claude Code plugin
install_plugin() {
    if command -v claude &> /dev/null; then
        info "Installing Claude Code plugin..."
        claude plugin install "$REPO" || warn "Plugin install failed - you may need to install it manually"
    else
        warn "Claude Code CLI not found. Install the plugin manually with:"
        echo "  claude plugin install $REPO"
    fi
}

# Create config directory
setup_config() {
    local config_dir="$HOME/.mira"

    if [ ! -d "$config_dir" ]; then
        info "Creating config directory at $config_dir"
        mkdir -p "$config_dir"
    fi

    if [ ! -f "$config_dir/.env" ]; then
        info "Creating .env template at $config_dir/.env"
        cat > "$config_dir/.env" << 'EOF'
# ============================================
# MIRA API KEYS - Replace the values below
# ============================================

# DeepSeek (for expert consultations)
# Get your key: https://platform.deepseek.com/api_keys
DEEPSEEK_API_KEY=PASTE_YOUR_DEEPSEEK_KEY_HERE

# Google Gemini (for embeddings/semantic search)
# Get your key: https://aistudio.google.com/app/apikey
GEMINI_API_KEY=PASTE_YOUR_GEMINI_KEY_HERE
EOF
    fi
}

main() {
    echo ""
    echo "  ╔╦╗╦╦═╗╔═╗"
    echo "  ║║║║╠╦╝╠═╣"
    echo "  ╩ ╩╩╩╚═╩ ╩"
    echo "  Installer"
    echo ""

    local platform version

    platform=$(detect_platform)
    info "Detected platform: $platform"

    version=$(get_latest_version)
    info "Latest version: $version"

    install_binary "$platform" "$version"
    setup_config
    install_plugin

    echo ""
    info "Installation complete!"
    echo ""
    echo "  Next steps:"
    echo ""
    echo "    1. Add your API keys:"
    echo ""
    echo "       Open ~/.mira/.env in your editor and replace:"
    echo "         PASTE_YOUR_DEEPSEEK_KEY_HERE  ->  your actual DeepSeek key"
    echo "         PASTE_YOUR_GEMINI_KEY_HERE    ->  your actual Gemini key"
    echo ""
    echo "       Get keys from:"
    echo "         DeepSeek: https://platform.deepseek.com/api_keys"
    echo "         Gemini:   https://aistudio.google.com/app/apikey"
    echo ""
    echo "    2. Add Mira instructions to your project's CLAUDE.md:"
    echo ""
    echo "       curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/docs/CLAUDE_TEMPLATE.md >> CLAUDE.md"
    echo ""
    echo "    3. Start Claude Code in your project directory"
    echo ""
    echo "  Verify: mira --version"
    echo ""
}

main
