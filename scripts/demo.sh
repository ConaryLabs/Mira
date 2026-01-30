#!/bin/bash
# Mira Demo Script
#
# This script demonstrates Mira's key features for an asciinema recording.
#
# Usage:
#   Option 1 (manual): Read through and type commands yourself while recording
#   Option 2 (auto):   ./scripts/demo.sh auto
#
# To record:
#   asciinema rec demo.cast
#   # Then either type commands or run: source scripts/demo.sh auto

set -e
cd "$(dirname "$0")/.."

MIRA="./target/release/mira"

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Typing simulation for auto mode
type_cmd() {
    if [[ "$1" == "auto" ]]; then
        echo -ne "${CYAN}\$ ${NC}"
        echo "$2" | pv -qL 30  # Simulate typing at 30 chars/sec
        sleep 0.5
        eval "$2"
        echo
        sleep 1.5
    fi
}

header() {
    echo -e "\n${GREEN}━━━ $1 ━━━${NC}\n"
    sleep 1
}

# Check for auto mode
MODE="${1:-manual}"

if [[ "$MODE" == "auto" ]]; then
    # Check for pv (pipe viewer for typing simulation)
    if ! command -v pv &> /dev/null; then
        echo "Installing pv for typing simulation..."
        sudo dnf install -y pv || sudo apt-get install -y pv
    fi
fi

clear
echo -e "${BLUE}"
cat << 'EOF'
  __  __ _
 |  \/  (_)_ __ __ _
 | |\/| | | '__/ _` |
 | |  | | | | | (_| |
 |_|  |_|_|_|  \__,_|

 A second brain for Claude Code
EOF
echo -e "${NC}"
sleep 2

# ============================================================================
header "1. Index a codebase for code intelligence"
# ============================================================================

if [[ "$MODE" == "auto" ]]; then
    type_cmd auto "$MIRA index --path . --quiet --no-embed"
else
    echo "# Run: $MIRA index --path . --quiet"
fi

# ============================================================================
header "2. Store a memory - decisions persist across sessions"
# ============================================================================

if [[ "$MODE" == "auto" ]]; then
    type_cmd auto "$MIRA tool remember '{\"content\": \"We use the builder pattern for all config structs in this project\", \"category\": \"architecture\"}'"
else
    echo "# Run: $MIRA tool remember '{\"content\": \"We use the builder pattern for all config structs\", \"category\": \"architecture\"}'"
fi

# ============================================================================
header "3. Recall memories by semantic search"
# ============================================================================

if [[ "$MODE" == "auto" ]]; then
    type_cmd auto "$MIRA tool recall '{\"query\": \"config struct patterns\"}'"
else
    echo "# Run: $MIRA tool recall '{\"query\": \"config struct patterns\"}'"
fi

# ============================================================================
header "4. Semantic code search - find code by meaning"
# ============================================================================

if [[ "$MODE" == "auto" ]]; then
    type_cmd auto "$MIRA tool search_code '{\"query\": \"error handling and recovery\", \"limit\": 5}'"
else
    echo "# Run: $MIRA tool search_code '{\"query\": \"error handling and recovery\", \"limit\": 5}'"
fi

# ============================================================================
header "5. Call graph analysis - who calls this function?"
# ============================================================================

if [[ "$MODE" == "auto" ]]; then
    type_cmd auto "$MIRA tool find_callers '{\"function_name\": \"remember\", \"limit\": 5}'"
else
    echo "# Run: $MIRA tool find_callers '{\"function_name\": \"remember\", \"limit\": 5}'"
fi

# ============================================================================
header "6. Track goals across sessions"
# ============================================================================

if [[ "$MODE" == "auto" ]]; then
    type_cmd auto "$MIRA tool goal '{\"action\": \"create\", \"title\": \"Ship v1.0 release\", \"priority\": \"high\"}'"
    sleep 1
    type_cmd auto "$MIRA tool goal '{\"action\": \"list\"}'"
else
    echo "# Run: $MIRA tool goal '{\"action\": \"create\", \"title\": \"Ship v1.0 release\", \"priority\": \"high\"}'"
    echo "# Run: $MIRA tool goal '{\"action\": \"list\"}'"
fi

# ============================================================================
header "Done! Mira gives Claude Code persistent memory and deep code understanding."
# ============================================================================

echo -e "Learn more: ${CYAN}https://github.com/ConaryLabs/Mira${NC}"
echo
sleep 3
