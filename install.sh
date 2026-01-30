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
        info "Adding Mira marketplace..."
        claude plugin marketplace add "$REPO" 2>/dev/null || true

        info "Installing Claude Code plugin..."
        if claude plugin install "mira@mira" 2>/dev/null; then
            info "Plugin installed successfully"
        else
            warn "Plugin install failed - you may need to install it manually:"
            echo "    claude plugin marketplace add $REPO"
            echo "    claude plugin install mira@mira"
        fi
    else
        warn "Claude Code CLI not found. Install the plugin manually with:"
        echo "    claude plugin marketplace add $REPO"
        echo "    claude plugin install mira@mira"
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

# Brave Search (optional - enables web search for experts)
# Get your key: https://brave.com/search/api/
# BRAVE_API_KEY=PASTE_YOUR_BRAVE_KEY_HERE
EOF
    fi
}

# Configure Claude Code hooks for behavior tracking and proactive features
setup_hooks() {
    local settings_dir="$HOME/.claude"
    local settings_file="$settings_dir/settings.json"
    local mira_bin="$INSTALL_DIR/mira"

    # Ensure directory exists
    mkdir -p "$settings_dir"

    # Check if jq is available for JSON manipulation
    if ! command -v jq &> /dev/null; then
        warn "jq not found - skipping hook configuration"
        warn "Install hooks manually by adding to ~/.claude/settings.json:"
        echo '    "hooks": {'
        echo '      "PostToolUse": [{"matcher": "Write|Edit|NotebookEdit", "hooks": [{"type": "command", "command": "mira hook post-tool", "timeout": 5000}]}],'
        echo '      "UserPromptSubmit": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook user-prompt", "timeout": 5000}]}],'
        echo '      "SessionStart": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook session-start", "timeout": 10000}]}],'
        echo '      "PreCompact": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook pre-compact", "timeout": 30000}]}],'
        echo '      "Stop": [{"matcher": "", "hooks": [{"type": "command", "command": "mira hook stop", "timeout": 5000}]}]'
        echo '    }'
        return
    fi

    info "Configuring Claude Code hooks..."

    # Define the hooks we want to add
    local hooks_json
    hooks_json=$(cat << EOF
{
  "PostToolUse": [
    {
      "matcher": "Write|Edit|NotebookEdit",
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook post-tool",
          "timeout": 5000
        }
      ]
    }
  ],
  "UserPromptSubmit": [
    {
      "matcher": "",
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook user-prompt",
          "timeout": 5000
        }
      ]
    }
  ],
  "SessionStart": [
    {
      "matcher": "",
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook session-start",
          "timeout": 10000
        }
      ]
    }
  ],
  "PreCompact": [
    {
      "matcher": "",
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook pre-compact",
          "timeout": 30000
        }
      ]
    }
  ],
  "Stop": [
    {
      "matcher": "",
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook stop",
          "timeout": 5000
        }
      ]
    }
  ]
}
EOF
)

    if [ -f "$settings_file" ]; then
        # File exists - merge hooks
        local existing_hooks
        existing_hooks=$(jq '.hooks // {}' "$settings_file" 2>/dev/null || echo '{}')

        # Merge: new hooks take precedence for Mira-specific hooks
        local merged_hooks
        merged_hooks=$(echo "$existing_hooks" | jq --argjson new "$hooks_json" '. * $new')

        # Update the settings file
        local updated
        updated=$(jq --argjson hooks "$merged_hooks" '.hooks = $hooks' "$settings_file")
        echo "$updated" > "$settings_file"

        info "Updated hooks in $settings_file"
    else
        # Create new settings file with hooks
        echo "{\"hooks\": $hooks_json}" | jq '.' > "$settings_file"
        info "Created $settings_file with hooks"
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
    setup_hooks
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
    echo "    2. Add Mira instructions to your project:"
    echo ""
    echo "       cd /path/to/your/project"
    echo "       mira init"
    echo ""
    echo "       This creates CLAUDE.md, .claude/rules/, and .claude/skills/"
    echo "       with all Mira guidance in a modular structure."
    echo ""
    echo "       Or manually: see docs/CLAUDE_TEMPLATE.md for the file layout."
    echo ""
    echo "    3. Restart Claude Code (if running) to enable hooks"
    echo ""
    echo "  Verify: mira --version"
    echo ""
}

main
