#!/bin/bash
# install.sh
set -eo pipefail

# Mira installer
# Usage: curl -fsSL https://raw.githubusercontent.com/ConaryLabs/Mira/main/install.sh | bash

if [ -z "${HOME:-}" ]; then
    echo "error: \$HOME is not set" >&2
    exit 1
fi

REPO="ConaryLabs/Mira"
MIRA_HOME="${HOME}/.mira"
# Install directory — always ~/.mira/bin (no sudo needed).
# LOCALAPPDATA on Windows for convention.
case "$(uname -s 2>/dev/null)" in
    MINGW*|MSYS*|CYGWIN*) INSTALL_DIR="${MIRA_INSTALL_DIR:-${LOCALAPPDATA}/Mira/bin}" ;;
    *)                    INSTALL_DIR="${MIRA_INSTALL_DIR:-${MIRA_HOME}/bin}" ;;
esac

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

info() { echo -e "${GREEN}==>${NC} $1"; }
warn() { echo -e "${YELLOW}warning:${NC} $1"; }
error() { echo -e "${RED}error:${NC} $1"; exit 1; }

# --- jq management ---

JQ=""
resolve_jq() {
    if [ -n "$JQ" ]; then return; fi
    local bundled="${MIRA_HOME}/bin/jq"
    if [ -x "$bundled" ]; then
        JQ="$bundled"
    elif command -v jq &> /dev/null; then
        JQ="$(command -v jq)"
    fi
}

install_jq() {
    resolve_jq
    if [ -n "$JQ" ]; then return 0; fi

    local bin_dir="${MIRA_HOME}/bin"
    local jq_bin="${bin_dir}/jq"
    local jq_version="1.7.1"
    local jq_url="" jq_expected_hash=""
    local os_name arch_name

    os_name="$(uname -s)"
    arch_name="$(uname -m)"

    # URLs and SHA256 checksums for jq 1.7.1 (from jqlang/jq releases)
    case "$os_name" in
        Linux*)
            case "$arch_name" in
                x86_64|amd64)
                    jq_url="https://github.com/jqlang/jq/releases/download/jq-${jq_version}/jq-linux-amd64"
                    jq_expected_hash="5942c9b0934e510ee61eb3e30273f1b3fe2590df93933a93d7c58b81d19c8ff5" ;;
                aarch64|arm64)
                    jq_url="https://github.com/jqlang/jq/releases/download/jq-${jq_version}/jq-linux-arm64"
                    jq_expected_hash="4dd2d8a0661df0b22f1bb9a1f9830f06b6f3b8f7d91211a1ef5d7c4f06a8b4a5" ;;
            esac ;;
        Darwin*)
            case "$arch_name" in
                x86_64|amd64)
                    jq_url="https://github.com/jqlang/jq/releases/download/jq-${jq_version}/jq-macos-amd64"
                    jq_expected_hash="4155822bbf5ea90f5c79cf254665975eb4274d426d0709770c21774de5407443" ;;
                aarch64|arm64)
                    jq_url="https://github.com/jqlang/jq/releases/download/jq-${jq_version}/jq-macos-arm64"
                    jq_expected_hash="0bbe619e663e0de2c550be2fe0d240d076799d6f8a652b70fa04aea8a8362e8a" ;;
            esac ;;
        MINGW*|MSYS*|CYGWIN*)
            jq_url="https://github.com/jqlang/jq/releases/download/jq-${jq_version}/jq-windows-amd64.exe"
            jq_expected_hash="7451fbbf37feffb9bf262bd97c54f0da558c63f0748e64152dd87b0a07b6d6ab"
            jq_bin="${bin_dir}/jq.exe" ;;
    esac

    if [ -z "$jq_url" ]; then
        warn "Cannot determine jq download URL for ${os_name}/${arch_name}"
        return 1
    fi

    mkdir -p "$bin_dir"
    info "Downloading jq ${jq_version}..."
    if ! curl -fsSL "$jq_url" -o "$jq_bin" 2>/dev/null; then
        warn "Failed to download jq -- JSON config steps may be skipped"
        return 1
    fi

    # Verify jq checksum
    local jq_actual_hash=""
    if command -v sha256sum &>/dev/null; then
        jq_actual_hash=$(sha256sum "$jq_bin" | cut -d' ' -f1)
    elif command -v shasum &>/dev/null; then
        jq_actual_hash=$(shasum -a 256 "$jq_bin" | cut -d' ' -f1)
    fi
    if [ -n "$jq_actual_hash" ] && [ "$jq_actual_hash" != "$jq_expected_hash" ]; then
        warn "jq checksum verification failed -- removing untrusted binary"
        rm -f "$jq_bin"
        return 1
    fi

    chmod +x "$jq_bin"
    JQ="$jq_bin"
    return 0
}

require_jq() {
    resolve_jq
    if [ -z "$JQ" ]; then
        warn "jq not available -- skipping JSON configuration"
        return 1
    fi
    return 0
}

# Atomically write content to a file (write to .tmp, then rename).
# Resolves symlinks so we update the target rather than replacing the link.
# Usage: atomic_write "content" "/path/to/file"
atomic_write() {
    local content="$1" file="$2"
    # Resolve symlink to avoid replacing it with a regular file
    if [ -L "$file" ]; then
        local link_target link_dir
        link_target=$(readlink -f "$file" 2>/dev/null) || {
            # macOS/BSD fallback: resolve relative target against symlink's directory
            link_target=$(readlink "$file")
            case "$link_target" in
                /*) ;;  # absolute — use as-is
                *)  link_dir=$(cd "$(dirname "$file")" && pwd)
                    link_target="${link_dir}/${link_target}" ;;
            esac
        }
        file="$link_target"
    fi
    printf '%s\n' "$content" > "${file}.tmp"
    mv -f "${file}.tmp" "$file"
}

# --- Platform detection ---

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
        arm64|aarch64) arch="aarch64" ;;
        *) error "Unsupported architecture: $(uname -m)" ;;
    esac

    echo "${arch}-${os}"
}

# --- Version detection ---

get_latest_version() {
    local response
    response=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest") || {
        error "Failed to fetch latest release from GitHub (check network or try again)"
    }
    # Extract tag_name value; || true prevents pipefail from killing the script
    echo "$response" | grep -o '"tag_name"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*"v\{0,1\}\([^"]*\)".*/\1/' || true
}

# --- Binary installation ---

install_binary() {
    local platform="$1"
    local version="$2"
    local ext="tar.gz"
    local tmp_dir

    if [[ "$platform" == *"windows"* ]]; then
        ext="zip"
    fi

    local url="https://github.com/${REPO}/releases/download/v${version}/mira-${platform}.${ext}"

    info "Downloading mira ${version} for ${platform}..."

    tmp_dir=$(mktemp -d)
    trap 'rm -rf "$tmp_dir"' EXIT

    if ! curl -fsSL "$url" -o "$tmp_dir/mira.${ext}"; then
        error "Download failed: ${url} -- check your network and try again"
    fi

    # Verify checksum before extraction
    local checksum_url="https://github.com/${REPO}/releases/download/v${version}/checksums.sha256"
    local checksum_file="${tmp_dir}/checksums.sha256"
    if curl -fsSL "$checksum_url" -o "$checksum_file" 2>/dev/null; then
        local expected_hash actual_hash
        expected_hash=$(grep "mira-${platform}.${ext}" "$checksum_file" | cut -d' ' -f1)
        if [ -z "$expected_hash" ]; then
            error "Checksum file present but no entry for mira-${platform}.${ext} -- possible corrupted release"
        fi
        actual_hash=""
        if command -v sha256sum &>/dev/null; then
            actual_hash=$(sha256sum "$tmp_dir/mira.${ext}" | cut -d' ' -f1)
        elif command -v shasum &>/dev/null; then
            actual_hash=$(shasum -a 256 "$tmp_dir/mira.${ext}" | cut -d' ' -f1)
        fi
        if [ -z "$actual_hash" ]; then
            warn "No checksum tool found (sha256sum/shasum) -- skipping verification"
        elif [ "$actual_hash" != "$expected_hash" ]; then
            error "Checksum verification failed! Expected: ${expected_hash}, Got: ${actual_hash}"
        else
            info "Checksum verified"
        fi
    fi

    # Extract
    if [[ "$ext" == "zip" ]]; then
        if command -v powershell.exe &>/dev/null; then
            powershell.exe -NoProfile -Command \
                "Expand-Archive -Force -Path '$(cygpath -w "$tmp_dir/mira.zip")' -DestinationPath '$(cygpath -w "$tmp_dir")'" 2>/dev/null
        elif command -v unzip &>/dev/null; then
            unzip -q "$tmp_dir/mira.zip" -d "$tmp_dir"
        else
            error "No zip extraction tool found (need unzip or powershell.exe)"
        fi
    else
        if ! tar -xz -C "$tmp_dir" -f "$tmp_dir/mira.${ext}"; then
            error "Failed to extract archive -- try downloading manually from https://github.com/${REPO}/releases"
        fi
    fi

    local bin_name="mira"
    if [[ "$platform" == *"windows"* ]]; then
        bin_name="mira.exe"
    fi

    mkdir -p "$INSTALL_DIR"
    mv -f "$tmp_dir/$bin_name" "$INSTALL_DIR/$bin_name"
    chmod +x "$INSTALL_DIR/$bin_name" 2>/dev/null || true

    # Strip macOS quarantine attribute
    case "$(uname -s)" in
        Darwin*) xattr -d com.apple.quarantine "$INSTALL_DIR/$bin_name" 2>/dev/null || true ;;
    esac

    # Clean up before any exec calls (EXIT trap doesn't fire on exec in dash)
    rm -rf "$tmp_dir"
    trap - EXIT

    info "Installed mira to $INSTALL_DIR/$bin_name"
}

# --- Plugin installation ---

install_plugin() {
    if command -v claude &> /dev/null; then
        info "Adding Mira marketplace..."
        claude plugin marketplace add "$REPO" 2>/dev/null || true

        info "Installing Claude Code plugin..."
        if claude plugin install "mira@mira" 2>/dev/null; then
            info "Plugin installed successfully (hooks + skills auto-configured)"
            return 0
        else
            warn "Plugin install failed - falling back to manual setup"
            return 1
        fi
    else
        warn "Claude Code CLI not found - falling back to manual setup"
        return 1
    fi
}

# --- Config directory ---

setup_config() {
    if [ ! -d "$MIRA_HOME" ]; then
        info "Creating config directory at $MIRA_HOME"
        mkdir -p "$MIRA_HOME"
        chmod 700 "$MIRA_HOME"
    fi
}

# --- MCP server fallback ---
# When plugin install fails, register mira as a global MCP server so Claude Code
# can still call mira's tools (code, session, goal, etc.).

setup_mcp() {
    require_jq || return

    local mcp_file="${HOME}/.claude/mcp.json"
    local mira_exe="mira"
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) mira_exe="mira.exe" ;;
    esac
    local mira_bin="$INSTALL_DIR/$mira_exe"

    mkdir -p "${HOME}/.claude"

    if [ -f "$mcp_file" ]; then
        # Check if mira server already configured
        local has_mira
        has_mira=$("$JQ" '.mcpServers | has("mira")' "$mcp_file" 2>/dev/null || echo "false")
        if [ "$has_mira" = "true" ]; then
            return
        fi
        local updated
        updated=$("$JQ" --arg cmd "$mira_bin" \
            '.mcpServers.mira = {"command": $cmd, "args": ["serve"]}' "$mcp_file") || {
            warn "Failed to update $mcp_file"
            return
        }
        atomic_write "$updated" "$mcp_file"
    else
        local content
        content=$("$JQ" -n --arg cmd "$mira_bin" \
            '{mcpServers: {mira: {command: $cmd, args: ["serve"]}}}') || {
            warn "Failed to generate $mcp_file"
            return
        }
        atomic_write "$content" "$mcp_file"
    fi

    info "MCP server configured in $mcp_file"
}

# --- Hooks setup ---

setup_hooks() {
    local settings_dir="$HOME/.claude"
    local settings_file="$settings_dir/settings.json"
    local mira_exe="mira"
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) mira_exe="mira.exe" ;;
    esac
    local mira_bin="$INSTALL_DIR/$mira_exe"

    mkdir -p "$settings_dir"

    if ! require_jq; then
        warn "Install hooks manually by adding to ~/.claude/settings.json:"
        cat << MANUAL
    "hooks": {
      "SessionStart": [{"hooks": [{"type": "command", "command": "${mira_bin} hook session-start", "timeout": 10}]}],
      "UserPromptSubmit": [{"hooks": [{"type": "command", "command": "${mira_bin} hook user-prompt", "timeout": 8}]}]
    }
    (see https://github.com/ConaryLabs/Mira for full hook list)
MANUAL
        return
    fi

    info "Configuring Claude Code hooks..."

    # Build hooks JSON using jq --arg for safe path escaping
    local hooks_json
    hooks_json=$("$JQ" -n --arg bin "$mira_bin" '{
  SessionStart: [{hooks: [{type: "command", command: ($bin + " hook session-start"), timeout: 10, statusMessage: "Mira: Loading session context..."}]}],
  UserPromptSubmit: [{hooks: [{type: "command", command: ($bin + " hook user-prompt"), timeout: 8, statusMessage: "Mira: Loading context..."}]}],
  PreToolUse: [{matcher: "Grep|Glob|Read", hooks: [{type: "command", command: ($bin + " hook pre-tool"), timeout: 3, statusMessage: "Mira: Checking relevant context..."}]}],
  PostToolUse: [{matcher: "Write|Edit|NotebookEdit|Bash", hooks: [{type: "command", command: ($bin + " hook post-tool"), timeout: 5, statusMessage: "Mira: Tracking changes..."}]}],
  PostToolUseFailure: [{hooks: [{type: "command", command: ($bin + " hook post-tool-failure"), timeout: 5, async: true, statusMessage: "Mira: Analyzing failure..."}]}],
  PreCompact: [{hooks: [{type: "command", command: ($bin + " hook pre-compact"), timeout: 30, async: true, statusMessage: "Mira: Preserving context..."}]}],
  Stop: [{hooks: [{type: "command", command: ($bin + " hook stop"), timeout: 8, statusMessage: "Mira: Saving session..."}]}],
  SessionEnd: [{hooks: [{type: "command", command: ($bin + " hook session-end"), timeout: 15, statusMessage: "Mira: Closing session..."}]}],
  SubagentStart: [{hooks: [{type: "command", command: ($bin + " hook subagent-start"), timeout: 3, statusMessage: "Mira: Injecting agent context..."}]}],
  SubagentStop: [{hooks: [{type: "command", command: ($bin + " hook subagent-stop"), timeout: 3, async: true, statusMessage: "Mira: Capturing discoveries..."}]}],
  TaskCompleted: [{hooks: [{type: "command", command: ($bin + " hook task-completed"), timeout: 5, statusMessage: "Mira: Processing task completion..."}]}],
  TeammateIdle: [{hooks: [{type: "command", command: ($bin + " hook teammate-idle"), timeout: 5, statusMessage: "Mira: Checking teammate status..."}]}]
}')

    if [ -f "$settings_file" ]; then
        # Merge hooks, stripping old Mira entries (both direct mira and mira-wrapper paths).
        local updated
        updated=$("$JQ" --argjson new "$hooks_json" '
            .hooks = (reduce ($new | keys[]) as $event (
                (.hooks // {});
                if .[$event] then
                    .[$event] = ([.[$event][] |
                        .hooks = [(.hooks // [])[] | select(
                            .command | tostring | test("mira(-wrapper)?(\\.exe)?\"? hook ") | not
                        )] |
                        select((.hooks | length) > 0)
                    ] + $new[$event])
                else
                    .[$event] = $new[$event]
                end
            ))
        ' "$settings_file") || {
            warn "Failed to parse $settings_file -- is it valid JSON?"
            return
        }

        atomic_write "$updated" "$settings_file"
        info "Updated hooks in $settings_file (existing hooks preserved)"
    else
        local content
        content=$(printf '{"hooks": %s}' "$hooks_json" | "$JQ" '.')
        atomic_write "$content" "$settings_file"
        info "Created $settings_file with hooks"
    fi
}

# --- Status line ---

setup_statusline() {
    require_jq || return

    local settings_dir="$HOME/.claude"
    local settings_file="$settings_dir/settings.json"
    local mira_exe="mira"
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) mira_exe="mira.exe" ;;
    esac
    local mira_bin="$INSTALL_DIR/$mira_exe"
    local statusline_cmd="${mira_bin} statusline"

    mkdir -p "$settings_dir"

    # Check if existing status line is a working Mira statusline
    if [ -f "$settings_file" ]; then
        local current_cmd
        current_cmd=$("$JQ" -r '.statusLine.command // empty' "$settings_file" 2>/dev/null)
        if [ -n "$current_cmd" ]; then
            # Only yield if it's a mira statusline AND the binary works
            case "$current_cmd" in
                *mira*statusline*)
                    local current_bin="${current_cmd%% *}"
                    if [ -x "$current_bin" ]; then
                        return
                    fi
                    ;;
            esac
            # Non-mira statusline or broken mira path -- overwrite
        fi
    fi

    if [ -f "$settings_file" ]; then
        local updated
        updated=$("$JQ" --arg cmd "$statusline_cmd" \
            '.statusLine = {"type": "command", "command": $cmd}' "$settings_file") || {
            warn "Failed to update statusline in $settings_file"
            return
        }
        atomic_write "$updated" "$settings_file"
    else
        local content
        content=$("$JQ" -n --arg cmd "$statusline_cmd" \
            '{statusLine: {type: "command", command: $cmd}}')
        atomic_write "$content" "$settings_file"
    fi

    info "Status line configured (shows goal/index stats)"
}

# --- Main ---

main() {
    echo ""
    echo "  ╔╦╗╦╦═╗╔═╗"
    echo "  ║║║║╠╦╝╠═╣"
    echo "  ╩ ╩╩╩╚═╩ ╩"
    echo "  Installer"
    echo ""

    local platform version plugin_ok

    # Check for curl
    if ! command -v curl &> /dev/null; then
        error "curl is required but not found. Install curl and try again."
    fi

    platform=$(detect_platform)
    info "Detected platform: $platform"

    version=$(get_latest_version)
    if [ -z "$version" ]; then
        error "Failed to determine latest version (GitHub API may be rate-limited)"
    fi
    info "Latest version: $version"

    setup_config
    install_binary "$platform" "$version"

    # Ensure jq is available for JSON config steps (hooks, statusline, MCP)
    install_jq || true

    # Try plugin install first -- it configures hooks, skills, and MCP automatically.
    # Fall back to manual setup if plugin install fails.
    plugin_ok=0
    install_plugin || plugin_ok=1

    if [ "$plugin_ok" -eq 1 ]; then
        setup_hooks || warn "Hook setup incomplete -- add hooks manually (see README)"
        setup_mcp || warn "MCP setup incomplete -- add mira server to ~/.claude/mcp.json manually"
    fi

    setup_statusline || true

    # Check if INSTALL_DIR is in PATH
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            info "Add ~/.mira/bin to your PATH for convenience:"
            echo ""
            echo "    echo 'export PATH=\"\$HOME/.mira/bin:\$PATH\"' >> ~/.bashrc"
            echo "    source ~/.bashrc"
            echo ""
            ;;
    esac

    info "Installation complete!"
    echo ""
    echo "  Next steps:"
    echo ""
    echo "    1. Configure providers (optional):"
    echo "       ${INSTALL_DIR}/mira setup"
    echo ""
    echo "    2. Or just start using Claude Code -- Mira works without API keys."
    echo "       Code intelligence, session persistence, and goal tracking are ready."
    echo ""
    if [ "$plugin_ok" -eq 0 ]; then
        echo "    Plugin installed -- hooks, skills, and MCP auto-configured."
        echo ""
        echo "  Try it now:"
        echo "    /mira:status          -- See what Mira knows about your project"
        echo '    /mira:search "..."    -- Semantic code search'
        echo "    /mira:goals           -- Track work across sessions"
        echo "    /mira:insights        -- Background analysis results"
    else
        echo "    Hooks and MCP server configured in ~/.claude/"
        echo "    Skills require the plugin. Install later with:"
        echo "      claude plugin marketplace add $REPO"
        echo "      claude plugin install mira@mira"
    fi
    echo ""
    echo "  Verify: ${INSTALL_DIR}/mira --version"
    echo ""
}

main
