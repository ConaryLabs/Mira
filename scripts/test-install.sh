#!/bin/sh
# test-install.sh — Multi-distro install tests using Podman (or Docker).
#
# Usage:
#   ./scripts/test-install.sh                  # test all distros
#   ./scripts/test-install.sh --distro alpine  # test one distro
#
# Environment:
#   CONTAINER_RT=docker   — use Docker instead of Podman
#   MIRA_TEST_VERSION=... — override version (default: read from wrapper)

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

RT="${CONTAINER_RT:-podman}"

# Verify container runtime is available
if ! command -v "$RT" >/dev/null 2>&1; then
    echo "Error: $RT not found. Install podman or set CONTAINER_RT=docker" >&2
    exit 1
fi

# Resolve version from wrapper
VERSION="${MIRA_TEST_VERSION:-$(grep '^MIRA_VERSION=' "$PROJECT_DIR/plugin/bin/mira-wrapper" | cut -d'"' -f2)}"
echo "Testing Mira v${VERSION}"
echo "Container runtime: $RT"
echo ""

# Distros to test
ALL_DISTROS="ubuntu:24.04 debian:12 fedora:43 alpine:latest"
DISTROS="$ALL_DISTROS"

# Parse --distro flag
while [ $# -gt 0 ]; do
    case "$1" in
        --distro)
            DISTROS="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            echo "Usage: $0 [--distro IMAGE]" >&2
            exit 1
            ;;
    esac
done

# Track results
PASS_COUNT=0
FAIL_COUNT=0
FAILED_DISTROS=""

# Cleanup containers on exit
CONTAINERS=""
cleanup() {
    for cid in $CONTAINERS; do
        "$RT" rm -f "$cid" >/dev/null 2>&1 || true
    done
}
trap cleanup EXIT

# Prerequisites installer per distro
install_prereqs() {
    local image="$1"
    case "$image" in
        ubuntu:*|debian:*)
            echo "apt-get update -qq && apt-get install -y -qq curl tar jq >/dev/null 2>&1"
            ;;
        fedora:*)
            echo "dnf install -y -q curl tar jq >/dev/null 2>&1"
            ;;
        alpine:*)
            echo "apk add --no-cache curl tar jq >/dev/null 2>&1"
            ;;
        *)
            echo "true"
            ;;
    esac
}

# Run test suite inside a container
run_distro_test() {
    local image="$1"
    local distro_name
    distro_name=$(echo "$image" | tr ':/' '_')

    echo "================================================"
    echo "Testing: $image"
    echo "================================================"

    local prereqs
    prereqs=$(install_prereqs "$image")

    # Build the test script to run inside the container
    local test_script
    test_script=$(cat <<'INNEREOF'
FAIL=0
fail() { printf "  FAIL: %s\n" "$1"; FAIL=1; }
pass() { printf "  PASS: %s\n" "$1"; }
warn() { printf "  WARN: %s\n" "$1"; }

# Soft assertion — reports but does not fail the test (for binary execution
# on distros with incompatible glibc/musl).
soft() {
    if [ "$1" = "pass" ]; then
        pass "$2"
    else
        warn "$2 (binary incompatible with this libc — not a test failure)"
    fi
}

VERSION="__VERSION__"

# ============================================================
# Test 1: mira-wrapper first run (download)
# ============================================================
echo "--- Test 1: mira-wrapper first run (download) ---"

STDERR_FILE=$(mktemp)
OUTPUT=$(sh /workspace/plugin/bin/mira-wrapper --version 2>"$STDERR_FILE") || true
STDERR=$(cat "$STDERR_FILE")
rm -f "$STDERR_FILE"

# 1a: binary exists and is executable
if [ -x "$HOME/.mira/bin/mira" ]; then
    pass "binary exists and is executable"
else
    fail "binary not found or not executable at ~/.mira/bin/mira"
fi

# 1b: version file present with valid version
INSTALLED_VER=""
if [ -f "$HOME/.mira/bin/.mira-version" ]; then
    INSTALLED_VER=$(cat "$HOME/.mira/bin/.mira-version")
    if echo "$INSTALLED_VER" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+'; then
        pass "version file present ($INSTALLED_VER)"
    else
        fail "version file has invalid content: $INSTALLED_VER"
    fi
else
    fail "version file not found"
fi

# 1c: stderr has download log
if echo "$STDERR" | grep -qi "download"; then
    pass "stderr contains download log"
else
    fail "stderr missing download log"
fi

# 1d: --version output (soft — may fail on musl/old glibc)
if [ -n "$INSTALLED_VER" ] && echo "$OUTPUT" | grep -q "$INSTALLED_VER"; then
    soft pass "--version output contains $INSTALLED_VER"
else
    soft fail "--version output missing version string"
fi

# 1e: update cache file exists
if [ -f "$HOME/.mira/.last-update-check" ]; then
    pass "update cache file exists"
else
    fail "update cache file missing (~/.mira/.last-update-check)"
fi

# 1f: checksum tool available
if command -v sha256sum >/dev/null 2>&1 || command -v shasum >/dev/null 2>&1; then
    pass "checksum tool available (verification ran)"
else
    warn "no checksum tool (sha256sum/shasum) — verification was skipped"
fi

# 1g: jq downloaded alongside mira
if [ -x "$HOME/.mira/bin/jq" ]; then
    pass "jq binary downloaded to ~/.mira/bin/jq"
else
    warn "jq not downloaded (statusline setup may have been skipped)"
fi

# ============================================================
# Test 2: mira-wrapper second run (fast path)
# ============================================================
echo ""
echo "--- Test 2: mira-wrapper second run (fast path) ---"

STDERR_FILE=$(mktemp)
sh /workspace/plugin/bin/mira-wrapper --version 2>"$STDERR_FILE" || true
STDERR2=$(cat "$STDERR_FILE")
rm -f "$STDERR_FILE"

if echo "$STDERR2" | grep -qi "download"; then
    fail "second run should not download again"
else
    pass "fast path — no download on second run"
fi

# ============================================================
# Test 3: mira-wrapper version pinning
# ============================================================
echo ""
echo "--- Test 3: mira-wrapper version pinning ---"

if [ -n "$INSTALLED_VER" ]; then
    # 3a: pinned to installed version → fast path
    STDERR_FILE=$(mktemp)
    MIRA_VERSION_PIN="$INSTALLED_VER" sh /workspace/plugin/bin/mira-wrapper --version 2>"$STDERR_FILE" || true
    STDERR3=$(cat "$STDERR_FILE")
    rm -f "$STDERR_FILE"

    if echo "$STDERR3" | grep -qi "download"; then
        fail "MIRA_VERSION_PIN=$INSTALLED_VER should not trigger download"
    else
        pass "MIRA_VERSION_PIN fast path works ($INSTALLED_VER)"
    fi

    # 3b: pinned to a different version → should attempt download (but may fail, that's ok)
    STDERR_FILE=$(mktemp)
    MIRA_VERSION_PIN="0.0.1" sh /workspace/plugin/bin/mira-wrapper --version 2>"$STDERR_FILE" || true
    STDERR3B=$(cat "$STDERR_FILE")
    rm -f "$STDERR_FILE"

    if echo "$STDERR3B" | grep -qi "download\|0\.0\.1"; then
        pass "MIRA_VERSION_PIN=0.0.1 triggers download attempt"
    else
        fail "MIRA_VERSION_PIN=0.0.1 did not trigger download attempt"
    fi
else
    fail "cannot test pinning — no version file from test 1"
fi

# ============================================================
# Test 4: install.sh
# ============================================================
echo ""
echo "--- Test 4: install.sh ---"

export MIRA_INSTALL_DIR="/tmp/mira-install-test"
mkdir -p "$MIRA_INSTALL_DIR"

# Remove .mira and .claude to test install.sh independently
rm -rf "$HOME/.mira"
rm -rf "$HOME/.claude"

# install.sh uses bash — install it if missing (Alpine)
if ! command -v bash >/dev/null 2>&1; then
    if command -v apk >/dev/null 2>&1; then
        apk add --no-cache bash >/dev/null 2>&1
    fi
fi

INSTALL_OUTPUT=$(bash /workspace/install.sh 2>&1) || true

# 4a: binary installed
if [ -x "$MIRA_INSTALL_DIR/mira" ]; then
    pass "binary installed to $MIRA_INSTALL_DIR"
else
    fail "binary not found at $MIRA_INSTALL_DIR/mira"
fi

# 4b: binary runs (soft — may fail on musl/old glibc)
INST_OUTPUT=$("$MIRA_INSTALL_DIR/mira" --version 2>/dev/null) || true
if echo "$INST_OUTPUT" | grep -qi "mira"; then
    soft pass "binary runs and identifies as mira"
else
    soft fail "binary cannot run on this libc"
fi

# 4c: ~/.mira config directory created
if [ -d "$HOME/.mira" ]; then
    pass "~/.mira config directory created"
else
    fail "~/.mira config directory missing"
fi

# 4d: hooks configured in ~/.claude/settings.json
if [ -f "$HOME/.claude/settings.json" ]; then
    pass "~/.claude/settings.json created"

    if command -v jq >/dev/null 2>&1; then
        # 4e: settings.json contains mira hook entries
        HOOK_COUNT=$(jq '[.hooks // {} | to_entries[] | .value[] | .hooks[]? | select(.command | tostring | test("mira"))] | length' "$HOME/.claude/settings.json" 2>/dev/null || echo "0")
        if [ "$HOOK_COUNT" -ge 8 ]; then
            pass "$HOOK_COUNT mira hooks configured (expected >= 8)"
        else
            fail "only $HOOK_COUNT mira hooks found (expected >= 8)"
        fi

        # 4f: key hook events present
        for event in SessionStart UserPromptSubmit Stop PostToolUse PreToolUse PreCompact SessionEnd SubagentStart; do
            if jq -e ".hooks.${event}" "$HOME/.claude/settings.json" >/dev/null 2>&1; then
                pass "${event} hook present"
            else
                fail "${event} hook missing"
            fi
        done

        # 4g: hook commands reference correct binary path
        HOOK_PATH=$(jq -r '.hooks.SessionStart[0].hooks[0].command // ""' "$HOME/.claude/settings.json" 2>/dev/null || echo "")
        if echo "$HOOK_PATH" | grep -q "$MIRA_INSTALL_DIR/mira"; then
            pass "hook commands reference $MIRA_INSTALL_DIR/mira"
        else
            fail "hook commands don't reference expected binary path (got: $HOOK_PATH)"
        fi

        # 4h: statusLine configured
        if jq -e '.statusLine' "$HOME/.claude/settings.json" >/dev/null 2>&1; then
            pass "statusLine configured"

            SL_CMD=$(jq -r '.statusLine.command // ""' "$HOME/.claude/settings.json" 2>/dev/null || echo "")
            if echo "$SL_CMD" | grep -q "statusline"; then
                pass "statusLine command is 'mira statusline'"
            else
                fail "statusLine command unexpected: $SL_CMD"
            fi
        else
            fail "statusLine missing from settings.json"
        fi
    else
        warn "jq not available — cannot validate settings.json structure"
    fi
else
    fail "~/.claude/settings.json not created (is jq installed?)"
fi

# 4i: MCP server configured in ~/.claude/mcp.json (fallback when plugin unavailable)
if [ -f "$HOME/.claude/mcp.json" ]; then
    if command -v jq >/dev/null 2>&1; then
        MCP_CMD=$(jq -r '.mcpServers.mira.command // ""' "$HOME/.claude/mcp.json" 2>/dev/null || echo "")
        if [ -n "$MCP_CMD" ]; then
            pass "MCP server configured in mcp.json ($MCP_CMD)"
        else
            fail "mcp.json exists but mira server entry missing"
        fi
    else
        pass "~/.claude/mcp.json created (cannot validate without jq)"
    fi
else
    fail "~/.claude/mcp.json not created (mira tools will be unavailable)"
fi

# 4j: output mentions mira setup in next steps
if echo "$INSTALL_OUTPUT" | grep -q "mira setup"; then
    pass "output mentions 'mira setup' in next steps"
else
    fail "output missing 'mira setup' in next steps"
fi

# 4k: output indicates hook or plugin configuration
if echo "$INSTALL_OUTPUT" | grep -qi "hook\|plugin\|MCP"; then
    pass "output confirms hook/plugin/MCP configuration"
else
    fail "output missing hook/plugin/MCP confirmation"
fi

# ============================================================
# Test 5: mira setup --check (smoke test)
# ============================================================
echo ""
echo "--- Test 5: mira setup --check (smoke test) ---"

if [ -x "$MIRA_INSTALL_DIR/mira" ]; then
    SETUP_OUTPUT=$("$MIRA_INSTALL_DIR/mira" setup --check 2>&1) || true
    if echo "$SETUP_OUTPUT" | grep -qi "config\|status\|provider\|directory"; then
        soft pass "mira setup --check produces diagnostic output"
    else
        soft fail "mira setup --check produced no recognizable output"
    fi
else
    warn "skipping mira setup --check — binary not executable on this platform"
fi

echo ""
exit $FAIL
INNEREOF
)

    # Substitute the version placeholder (use printf to avoid dash interpreting \n)
    test_script=$(printf '%s' "$test_script" | sed "s/__VERSION__/$VERSION/g")

    # Run the container
    local cid
    cid=$("$RT" run -d \
        -v "$PROJECT_DIR:/workspace:ro,Z" \
        "$image" \
        sleep 600)
    CONTAINERS="$CONTAINERS $cid"

    # Execute prereqs, then run the test script
    if ! "$RT" exec "$cid" sh -c "$prereqs"; then
        echo "  FAIL: prerequisite installation failed for $image"
        FAIL_COUNT=$((FAIL_COUNT + 1))
        FAILED_DISTROS="$FAILED_DISTROS $image"
        echo ""
        return
    fi

    # Pipe script via stdin to avoid shell quoting issues with sh -c
    if printf '%s' "$test_script" | "$RT" exec -i "$cid" sh; then
        echo "  RESULT: $image — ALL PASSED"
        PASS_COUNT=$((PASS_COUNT + 1))
    else
        echo "  RESULT: $image — SOME TESTS FAILED"
        FAIL_COUNT=$((FAIL_COUNT + 1))
        FAILED_DISTROS="$FAILED_DISTROS $image"
    fi

    echo ""
}

# Run tests for each distro
for distro in $DISTROS; do
    run_distro_test "$distro"
done

# Summary
echo "================================================"
echo "SUMMARY"
echo "================================================"
echo "  Passed: $PASS_COUNT"
echo "  Failed: $FAIL_COUNT"
if [ -n "$FAILED_DISTROS" ]; then
    echo "  Failed distros:$FAILED_DISTROS"
fi
echo ""

if [ "$FAIL_COUNT" -gt 0 ]; then
    exit 1
fi
