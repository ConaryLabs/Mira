#!/bin/sh
# test-hook-dedup.sh — Fixture tests for the jq hook deduplication logic
# in install.sh. Validates that Mira hooks are replaced while non-Mira
# hooks are preserved across all supported command formats.
#
# Usage:  ./scripts/test-hook-dedup.sh
# Requires: jq

set -e

if ! command -v jq >/dev/null 2>&1; then
    echo "SKIP: jq not found" >&2
    exit 0
fi

PASS=0
FAIL=0

assert_eq() {
    local label="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        printf "  PASS: %s\n" "$label"
        PASS=$((PASS + 1))
    else
        printf "  FAIL: %s\n" "$label"
        printf "    expected: %s\n" "$expected"
        printf "    actual:   %s\n" "$actual"
        FAIL=$((FAIL + 1))
    fi
}

# The jq dedup expression extracted from install.sh.
# $new = new Mira hooks to merge in, input = existing settings.
DEDUP_EXPR='
    .hooks = (reduce ($new | keys[]) as $event (
        (.hooks // {});
        if .[$event] then
            .[$event] = ([.[$event][] |
                .hooks = [(.hooks // [])[] | select(
                    .command | tostring | test("^(\"[^\"]*[/\\\\]|[^\\s\"]*[/\\\\])?mira(\\.exe)?\"? hook ") | not
                )] |
                select((.hooks | length) > 0)
            ] + $new[$event])
        else
            .[$event] = $new[$event]
        end
    ))
'

# New Mira hooks (what the installer would inject)
NEW_HOOKS='{"SessionStart":[{"hooks":[{"type":"command","command":"/usr/local/bin/mira hook session-start"}]}]}'

# --- Test 1: Bare "mira hook" is replaced ---
echo "--- Test 1: Bare mira command ---"
INPUT='{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"mira hook session-start"}]}]}}'
RESULT=$(echo "$INPUT" | jq --argjson new "$NEW_HOOKS" "$DEDUP_EXPR")
COUNT=$(echo "$RESULT" | jq '.hooks.SessionStart | length')
CMD=$(echo "$RESULT" | jq -r '.hooks.SessionStart[0].hooks[0].command')
assert_eq "old bare mira hook replaced" "1" "$COUNT"
assert_eq "new hook command present" "/usr/local/bin/mira hook session-start" "$CMD"

# --- Test 2: Absolute path mira is replaced ---
echo "--- Test 2: Absolute Unix path ---"
INPUT='{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"/opt/mira/bin/mira hook session-start"}]}]}}'
RESULT=$(echo "$INPUT" | jq --argjson new "$NEW_HOOKS" "$DEDUP_EXPR")
COUNT=$(echo "$RESULT" | jq '.hooks.SessionStart | length')
assert_eq "absolute path mira hook replaced" "1" "$COUNT"

# --- Test 3: Windows mira.exe is replaced ---
echo "--- Test 3: Windows mira.exe ---"
INPUT='{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"mira.exe hook session-start"}]}]}}'
RESULT=$(echo "$INPUT" | jq --argjson new "$NEW_HOOKS" "$DEDUP_EXPR")
COUNT=$(echo "$RESULT" | jq '.hooks.SessionStart | length')
assert_eq "bare mira.exe hook replaced" "1" "$COUNT"

# --- Test 4: Quoted Windows path is replaced ---
echo "--- Test 4: Quoted Windows path ---"
INPUT='{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"\"C:/Program Files/Mira/mira.exe\" hook session-start"}]}]}}'
RESULT=$(echo "$INPUT" | jq --argjson new "$NEW_HOOKS" "$DEDUP_EXPR")
COUNT=$(echo "$RESULT" | jq '.hooks.SessionStart | length')
assert_eq "quoted Windows path mira hook replaced" "1" "$COUNT"

# --- Test 5: Backslash Windows path is replaced ---
echo "--- Test 5: Backslash Windows path ---"
INPUT='{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"C:\\Users\\foo\\mira.exe hook session-start"}]}]}}'
RESULT=$(echo "$INPUT" | jq --argjson new "$NEW_HOOKS" "$DEDUP_EXPR")
COUNT=$(echo "$RESULT" | jq '.hooks.SessionStart | length')
assert_eq "backslash Windows path mira hook replaced" "1" "$COUNT"

# --- Test 6: "samira hook" is NOT removed (substring false positive) ---
echo "--- Test 6: samira (substring) preserved ---"
INPUT='{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"samira hook session-start"}]}]}}'
RESULT=$(echo "$INPUT" | jq --argjson new "$NEW_HOOKS" "$DEDUP_EXPR")
COUNT=$(echo "$RESULT" | jq '.hooks.SessionStart | length')
assert_eq "samira entry preserved alongside new hook" "2" "$COUNT"

# --- Test 7: Wrapper command with mira in args is NOT removed ---
echo "--- Test 7: Wrapper command preserved ---"
INPUT='{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"my-wrapper --arg /usr/local/bin/mira hook stop"}]}]}}'
RESULT=$(echo "$INPUT" | jq --argjson new "$NEW_HOOKS" "$DEDUP_EXPR")
COUNT=$(echo "$RESULT" | jq '.hooks.SessionStart | length')
assert_eq "wrapper command preserved alongside new hook" "2" "$COUNT"

# --- Test 8: Quoted arg containing "mira hook" is NOT removed ---
echo "--- Test 8: Quoted argument preserved ---"
INPUT='{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"my-logger --label \"mira hook stop\""}]}]}}'
RESULT=$(echo "$INPUT" | jq --argjson new "$NEW_HOOKS" "$DEDUP_EXPR")
COUNT=$(echo "$RESULT" | jq '.hooks.SessionStart | length')
assert_eq "quoted argument entry preserved alongside new hook" "2" "$COUNT"

# --- Test 9: Mixed entry — non-Mira commands kept, Mira commands stripped ---
echo "--- Test 9: Mixed entry (multi-hook) ---"
INPUT='{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"my-custom-tool start"},{"type":"command","command":"mira hook session-start"}]}]}}'
RESULT=$(echo "$INPUT" | jq --argjson new "$NEW_HOOKS" "$DEDUP_EXPR")
COUNT=$(echo "$RESULT" | jq '.hooks.SessionStart | length')
CUSTOM_CMD=$(echo "$RESULT" | jq -r '.hooks.SessionStart[0].hooks[0].command')
assert_eq "mixed entry: 2 entries (preserved custom + new mira)" "2" "$COUNT"
assert_eq "mixed entry: custom command survives" "my-custom-tool start" "$CUSTOM_CMD"

# --- Test 10: Empty hooks — new event added ---
echo "--- Test 10: No existing hooks ---"
INPUT='{}'
RESULT=$(echo "$INPUT" | jq --argjson new "$NEW_HOOKS" "$DEDUP_EXPR")
COUNT=$(echo "$RESULT" | jq '.hooks.SessionStart | length')
assert_eq "hooks added to empty settings" "1" "$COUNT"

# --- Summary ---
echo ""
echo "================================================"
echo "SUMMARY: $PASS passed, $FAIL failed"
echo "================================================"

[ "$FAIL" -eq 0 ]
