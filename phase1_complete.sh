#!/bin/bash
# Phase 1 Complete - Final cleanup steps

set -e

echo "========================================"
echo "Phase 1: Final Cleanup Steps"
echo "========================================"
echo ""

# 1. Run simple fixes
echo "Step 1: Running simple comment fixes..."
./simple_fixes.sh

# 2. Delete Claude client files
echo ""
echo "Step 2: Deleting Claude client files..."
rm -v src/llm/client/mod.rs
rm -v src/llm/client/config.rs
echo "✅ Deleted Claude client files"

# 3. Remove claude module from provider/mod.rs
echo ""
echo "Step 3: Removing claude module from provider/mod.rs..."
sed -i '/^pub mod claude;/d' src/llm/provider/mod.rs
echo "✅ Removed claude module export"

# 4. Remove claude_processor from structured/mod.rs
echo ""
echo "Step 4: Removing claude_processor from structured/mod.rs..."
sed -i '/^pub mod claude_processor;/d' src/llm/structured/mod.rs
sed -i '/^pub use claude_processor::/d' src/llm/structured/mod.rs
echo "✅ Removed claude_processor module"

# 5. Verify deletions
echo ""
echo "Step 5: Verifying cleanup..."
echo ""

# Check for remaining HIGH severity references
echo "Checking for remaining claude_processor imports..."
if rg -q "use.*claude_processor" src/; then
    echo "⚠️  Found remaining claude_processor imports (expected - will fix in Phase 2):"
    rg "use.*claude_processor" src/ --color always || true
else
    echo "✅ No claude_processor imports found"
fi

echo ""
echo "Checking for remaining CONFIG.anthropic references..."
if rg -q "CONFIG\.anthropic" src/; then
    echo "⚠️  Found remaining CONFIG.anthropic references (expected - will fix in Phase 2):"
    rg "CONFIG\.anthropic" src/ --color always | head -10 || true
else
    echo "✅ No CONFIG.anthropic references found"
fi

echo ""
echo "Checking for remaining ClaudeProvider references..."
if rg -q "ClaudeProvider" src/; then
    echo "⚠️  Found remaining ClaudeProvider references (expected - will fix in Phase 2):"
    rg "ClaudeProvider" src/ --color always || true
else
    echo "✅ No ClaudeProvider references found"
fi

echo ""
echo "========================================"
echo "Phase 1 Cleanup Complete!"
echo "========================================"
echo ""
echo "Remaining HIGH severity issues (24 total):"
echo "  - These will be fixed in Phase 2 when we:"
echo "    * Create the new router"
echo "    * Rewrite state.rs"
echo "    * Update tool processors"
echo ""
echo "Next steps:"
echo "  1. Copy updated config/mod.rs from artifact"
echo "  2. Copy updated .env from artifact"
echo "  3. Try: cargo check (will fail - expected)"
echo "  4. Commit Phase 1"
echo ""
