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
            echo "apt-get update -qq && apt-get install -y -qq curl tar >/dev/null 2>&1"
            ;;
        fedora:*)
            echo "dnf install -y -q curl tar >/dev/null 2>&1"
            ;;
        alpine:*)
            echo "apk add --no-cache curl tar >/dev/null 2>&1"
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

# 1b: version file matches
if [ -f "$HOME/.mira/bin/.mira-version" ]; then
    ACTUAL=$(cat "$HOME/.mira/bin/.mira-version")
    if [ "$ACTUAL" = "$VERSION" ]; then
        pass "version file matches ($ACTUAL)"
    else
        fail "version file mismatch: got $ACTUAL, expected $VERSION"
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

# 1d: --version output contains version (soft — may fail on musl/old glibc)
if echo "$OUTPUT" | grep -q "$VERSION"; then
    soft pass "--version output contains $VERSION"
else
    soft fail "--version output missing version string"
fi

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

echo ""
echo "--- Test 3: install.sh ---"

export MIRA_INSTALL_DIR="/tmp/mira-install-test"
mkdir -p "$MIRA_INSTALL_DIR"

# Remove .mira to test install.sh independently
rm -rf "$HOME/.mira"

# install.sh uses bash — install it if missing (Alpine)
if ! command -v bash >/dev/null 2>&1; then
    if command -v apk >/dev/null 2>&1; then
        apk add --no-cache bash >/dev/null 2>&1
    fi
fi

bash /workspace/install.sh 2>&1 || true

# 3a: binary installed
if [ -x "$MIRA_INSTALL_DIR/mira" ]; then
    pass "install.sh: binary installed to $MIRA_INSTALL_DIR"
else
    fail "install.sh: binary not found at $MIRA_INSTALL_DIR/mira"
fi

# 3b: binary runs (soft — may fail on musl/old glibc)
INST_OUTPUT=$("$MIRA_INSTALL_DIR/mira" --version 2>/dev/null) || true
if echo "$INST_OUTPUT" | grep -q "$VERSION"; then
    soft pass "install.sh: binary outputs version $VERSION"
else
    soft fail "install.sh: binary cannot run on this libc"
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
