#!/bin/bash
# Simple automated fixes for Claude references in comments and strings

set -e

echo "========================================"
echo "Simple Claude Reference Cleanup"
echo "========================================"
echo ""

# Backup warning
echo "⚠️  This will modify files in place!"
echo "   Make sure you have committed Phase 1 first."
echo ""
read -p "Continue? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 1
fi

echo ""
echo "Starting replacements..."
echo ""

# Comment replacements (safe - only in comments)
find src -name "*.rs" -type f -exec sed -i \
    -e 's/Claude uses similar/LLMs use similar/g' \
    -e 's/Claude sometimes/LLMs sometimes/g' \
    -e 's/Claude with extended thinking/Models with extended reasoning/g' \
    -e 's/Claude decides/the model decides/g' \
    -e "s/Claude didn't/Model didn't/g" \
    -e 's/Claude ended/Model ended/g' \
    -e 's/Claude called/Model called/g' \
    -e 's/Claude finished/Model finished/g' \
    -e 's/trust Claude/trust the model/g' \
    -e 's/Claude API/LLM API/g' \
    -e 's/Claude client/LLM client/g' \
    -e 's/Claude provider/LLM provider/g' \
    -e 's/Claude-specific/Provider-specific/g' \
    -e 's/Claude format/unified format/g' \
    -e 's/Claude-format/unified format/g' \
    -e 's/Claude-compatible/provider-compatible/g' \
    {} +

echo "✅ Updated comment references"

# Remove anthropic-version headers (these will fail anyway)
find src -name "*.rs" -type f -exec sed -i \
    -e '/anthropic-version/d' \
    -e '/anthropic-beta/d' \
    {} +

echo "✅ Removed Anthropic API headers"

echo ""
echo "========================================"
echo "Simple fixes complete!"
echo "========================================"
echo ""
echo "Next: Run the Python analysis script to see what's left:"
echo "  python3 claude_cleanup.py"
echo ""
