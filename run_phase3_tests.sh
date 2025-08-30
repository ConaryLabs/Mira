#!/bin/bash
# run_phase3_tests.sh - Script to run Phase 3 tests with proper environment

set -e

echo "🧪 Running Phase 3 Robust Memory Tests"
echo "====================================="

# Set environment variables for testing
export RUST_LOG=info
export MIRA_ROBUST_MEMORY_ENABLED=true
export MIRA_EMBED_HEADS=semantic,code,summary
export QDRANT_TEST_URL=${QDRANT_URL:-http://localhost:6333}

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BLUE}Environment:${NC}"
echo "  MIRA_ROBUST_MEMORY_ENABLED=$MIRA_ROBUST_MEMORY_ENABLED"
echo "  MIRA_EMBED_HEADS=$MIRA_EMBED_HEADS" 
echo "  QDRANT_TEST_URL=$QDRANT_TEST_URL"
echo ""

# Test 1: Basic multi-store functionality
echo -e "${BLUE}Test 1: Multi-store creation and basic operations${NC}"
cargo test test_multi_store_creation_and_basic_ops -- --nocapture

# Test 2: Text chunker functionality
echo -e "${BLUE}Test 2: Text chunker for different heads${NC}"
cargo test test_text_chunker_functionality -- --nocapture

# Test 3: Memory entry with Phase 3 fields
echo -e "${BLUE}Test 3: MemoryEntry with Phase 3 fields${NC}"
cargo test test_memory_entry_with_phase3_fields -- --nocapture

# Test 4: Config-driven setup
echo -e "${BLUE}Test 4: Config-driven multi-head setup${NC}"
cargo test test_config_driven_multi_head_setup -- --nocapture

# Test 5: Embedding head parsing
echo -e "${BLUE}Test 5: EmbeddingHead parsing${NC}"
cargo test test_embedding_head_parsing -- --nocapture

# Test 6: Integration workflow
echo -e "${BLUE}Test 6: Phase 3 integration workflow${NC}"
cargo test test_phase3_integration_workflow -- --nocapture

# Test 7: Original Qdrant test (fixed)
echo -e "${BLUE}Test 7: Original Qdrant semantic memory test${NC}"
cargo test qdrant_save_and_recall_roundtrip -- --nocapture

echo ""
echo -e "${GREEN}🎉 All Phase 3 tests completed!${NC}"
echo ""
echo -e "${YELLOW}Note: Some tests may show warnings about unused imports - this is normal${NC}"
echo -e "${YELLOW}      and doesn't affect functionality.${NC}"
