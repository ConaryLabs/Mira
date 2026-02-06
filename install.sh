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

# Install Claude Code plugin (returns 0 on success, 1 on failure)
install_plugin() {
    if command -v claude &> /dev/null; then
        info "Adding Mira marketplace..."
        claude plugin marketplace add "$REPO" 2>/dev/null || true

        info "Installing Claude Code plugin..."
        if claude plugin install "mira@mira" 2>/dev/null; then
            info "Plugin installed successfully (hooks + skills auto-configured)"
            return 0
        else
            warn "Plugin install failed - falling back to manual hook setup"
            return 1
        fi
    else
        warn "Claude Code CLI not found - falling back to manual hook setup"
        return 1
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
# ═══════════════════════════════════════════════════════════════════════════════
# Mira Environment Configuration
# ═══════════════════════════════════════════════════════════════════════════════
# Configuration is loaded once at startup from:
#   1. ~/.mira/.env (global)
#   2. .env (project-level, overrides global)

# ═══════════════════════════════════════════════════════════════════════════════
# API Keys
# ═══════════════════════════════════════════════════════════════════════════════

# DeepSeek - Primary LLM provider for experts and reasoning
# Get from: https://platform.deepseek.com/api_keys
DEEPSEEK_API_KEY=sk-your-deepseek-key-here

# OpenAI - Required for semantic search (embeddings via text-embedding-3-small)
# Get from: https://platform.openai.com/api-keys
# OPENAI_API_KEY=sk-your-openai-key-here

# Brave Search - Enables web search for expert consultations
# Get from: https://brave.com/search/api/
# BRAVE_API_KEY=your-brave-search-key-here

# ═══════════════════════════════════════════════════════════════════════════════
# LLM Provider Configuration
# ═══════════════════════════════════════════════════════════════════════════════

# Override default LLM provider (options: deepseek)
# DEFAULT_LLM_PROVIDER=deepseek

# ═══════════════════════════════════════════════════════════════════════════════
# User Identity
# ═══════════════════════════════════════════════════════════════════════════════

# Override user identity for multi-user memory scoping
# By default, Mira uses git config user.email or system username
# MIRA_USER_ID=your-unique-id

# ═══════════════════════════════════════════════════════════════════════════════
# Advanced Options
# ═══════════════════════════════════════════════════════════════════════════════

# Disable all LLM features (use heuristic fallbacks only)
# MIRA_DISABLE_LLM=1

# Override embedding dimensions (default: 1536)
# MIRA_EMBEDDING_DIMENSIONS=1536

# Override embedding task type (SEMANTIC_SIMILARITY, RETRIEVAL_DOCUMENT, etc.)
# MIRA_EMBEDDING_TASK_TYPE=RETRIEVAL_DOCUMENT

# Enable fuzzy fallback when embeddings are unavailable (default: true)
# MIRA_FUZZY_FALLBACK=true

# Override project path detection
# MIRA_PROJECT_PATH=/path/to/project
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
        warn "jq not found - skipping automatic hook configuration"
        warn "Install hooks manually by adding to ~/.claude/settings.json:"
        cat << MANUAL
    "hooks": {
      "SessionStart": [{"hooks": [{"type": "command", "command": "mira hook session-start", "timeout": 10}]}],
      "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "mira hook user-prompt", "timeout": 5}]}],
      "PermissionRequest": [{"hooks": [{"type": "command", "command": "mira hook permission", "timeout": 3}]}],
      "PreToolUse": [{"matcher": "Grep|Glob|Read", "hooks": [{"type": "command", "command": "mira hook pre-tool", "timeout": 2}]}],
      "PostToolUse": [{"matcher": "Write|Edit|NotebookEdit", "hooks": [{"type": "command", "command": "mira hook post-tool", "timeout": 5, "async": true}]}],
      "PreCompact": [{"matcher": "*", "hooks": [{"type": "command", "command": "mira hook pre-compact", "timeout": 30, "async": true}]}],
      "Stop": [{"hooks": [{"type": "command", "command": "mira hook stop", "timeout": 5}]}],
      "SessionEnd": [{"hooks": [{"type": "command", "command": "mira hook session-end", "timeout": 5}]}],
      "SubagentStart": [{"hooks": [{"type": "command", "command": "mira hook subagent-start", "timeout": 3}]}],
      "SubagentStop": [{"hooks": [{"type": "command", "command": "mira hook subagent-stop", "timeout": 3, "async": true}]}]
    }
MANUAL
        return
    fi

    info "Configuring Claude Code hooks..."

    # Define all 10 hooks matching plugin/hooks/hooks.json
    local hooks_json
    hooks_json=$(cat << EOF
{
  "SessionStart": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook session-start",
          "timeout": 10
        }
      ]
    }
  ],
  "UserPromptSubmit": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook user-prompt",
          "timeout": 5
        }
      ]
    }
  ],
  "PermissionRequest": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook permission",
          "timeout": 3
        }
      ]
    }
  ],
  "PreToolUse": [
    {
      "matcher": "Grep|Glob|Read",
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook pre-tool",
          "timeout": 2
        }
      ]
    }
  ],
  "PostToolUse": [
    {
      "matcher": "Write|Edit|NotebookEdit",
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook post-tool",
          "timeout": 5,
          "async": true
        }
      ]
    }
  ],
  "PreCompact": [
    {
      "matcher": "*",
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook pre-compact",
          "timeout": 30,
          "async": true
        }
      ]
    }
  ],
  "Stop": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook stop",
          "timeout": 5
        }
      ]
    }
  ],
  "SessionEnd": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook session-end",
          "timeout": 5
        }
      ]
    }
  ],
  "SubagentStart": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook subagent-start",
          "timeout": 3
        }
      ]
    }
  ],
  "SubagentStop": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "${mira_bin} hook subagent-stop",
          "timeout": 3,
          "async": true
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

    local platform version plugin_ok

    platform=$(detect_platform)
    info "Detected platform: $platform"

    version=$(get_latest_version)
    info "Latest version: $version"

    install_binary "$platform" "$version"
    setup_config

    # Try plugin install first — it configures hooks and skills automatically.
    # Only fall back to manual hook setup if plugin install fails.
    plugin_ok=0
    install_plugin || plugin_ok=1

    if [ "$plugin_ok" -eq 1 ]; then
        setup_hooks
    fi

    echo ""
    info "Installation complete!"
    echo ""
    echo "  Next steps:"
    echo ""
    echo "    1. Add your API keys:"
    echo ""
    echo "       Open ~/.mira/.env in your editor and replace the placeholder values."
    echo ""
    echo "       Get keys from:"
    echo "         DeepSeek: https://platform.deepseek.com/api_keys"
    echo "         OpenAI:   https://platform.openai.com/api-keys"
    echo ""

    if [ "$plugin_ok" -eq 0 ]; then
        echo "    2. Hooks and skills were auto-configured by the plugin."
        echo ""
    else
        echo "    2. Hooks were configured in ~/.claude/settings.json."
        echo "       To install the plugin later (includes skills):"
        echo "         claude plugin marketplace add $REPO"
        echo "         claude plugin install mira@mira"
        echo ""
    fi

    echo "    3. Add Mira instructions to your project:"
    echo "       See docs/CLAUDE_TEMPLATE.md for the recommended CLAUDE.md layout."
    echo ""
    echo "    4. Restart Claude Code (if running) to enable hooks"
    echo ""
    echo "  Verify: mira --version"
    echo ""
}

main
