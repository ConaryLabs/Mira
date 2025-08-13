#!/bin/bash
# Phase 9 - Cleanup Script for Legacy References
# Run this from the backend directory

echo "🧹 Starting Phase 9 cleanup of legacy references..."

# Backup files before changes (optional)
echo "📦 Creating backup..."
cp -r src src.backup.phase9
cp -r tests tests.backup.phase9

# Fix test files
echo "✏️ Updating test files..."

# tests/test_openai_connection.rs
sed -i 's/"gpt-4\.1"/"gpt-5"/g' tests/test_openai_connection.rs

# Fix import tool references
echo "✏️ Updating import tools..."

# src/tools/mira_import/openai.rs
sed -i 's/"gpt-4\.1"/"gpt-5"/g' src/tools/mira_import/openai.rs
sed -i 's/GPT-4\.1/GPT-5/g' src/tools/mira_import/openai.rs
# Replace tool_choice with function_call
sed -i 's/"tool_choice"/"function_call"/g' src/tools/mira_import/openai.rs
# Update the format to match Functions API
sed -i 's/"function_call": {"type": "function", "function": {"name": "memory_eval"}}/"function_call": {"name": "memory_eval"}/g' src/tools/mira_import/openai.rs

# Fix LLM modules
echo "✏️ Updating LLM modules..."

# src/llm/emotional_weight.rs
sed -i 's/"gpt-4\.1"/"gpt-5"/g' src/llm/emotional_weight.rs

# src/memory/salience.rs
sed -i 's/GPT-4\.1/GPT-5/g' src/memory/salience.rs

# Fix Claude/Anthropic references
echo "✏️ Removing Claude/Anthropic references..."

# src/tools/mod.rs - Update comments
cat > src/tools/mod.rs.tmp << 'EOF'
pub mod mira_import;

// Internal tools for Mira's operation
// Note: Web search is handled via OpenAI's retrieval capabilities
// Document processing uses OpenAI's vector store API
EOF
mv src/tools/mod.rs.tmp src/tools/mod.rs

# src/persona/mod.rs - Remove Claude references in comments
sed -i 's/Claude/GPT-5/g' src/persona/mod.rs
sed -i 's/ChatService and ClaudeSystem/ChatService/g' src/persona/mod.rs

# src/llm/streaming.rs - Update comments
sed -i 's/Claude orchestration/GPT-5 streaming/g' src/llm/streaming.rs
sed -i 's/ChatService with Claude/ChatService with GPT-5/g' src/llm/streaming.rs
sed -i 's/uses Claude for orchestration/uses GPT-5 for processing/g' src/llm/streaming.rs

# Search for any remaining occurrences
echo ""
echo "🔍 Checking for remaining legacy references..."
echo ""

echo "Checking for gpt-4 references:"
if rg -i "gpt-4" --type rust | grep -v "gpt-5"; then
    echo "⚠️  Found remaining gpt-4 references"
else
    echo "✅ No gpt-4 references found"
fi

echo ""
echo "Checking for Claude/Anthropic references:"
if rg -i "claude\|anthropic" --type rust | grep -v "// Historical"; then
    echo "⚠️  Found remaining Claude/Anthropic references"
else
    echo "✅ No Claude/Anthropic references found"
fi

echo ""
echo "Checking for legacy endpoints:"
if rg "chat/completions\|images/generations" --type rust; then
    echo "⚠️  Found legacy endpoint references"
else
    echo "✅ No legacy endpoint references found"
fi

echo ""
echo "Checking for old tool references:"
if rg "tool_choice\|\"tools\":" --type rust; then
    echo "⚠️  Found old tool API references"
else
    echo "✅ No old tool API references found"
fi

echo ""
echo "🎉 Cleanup complete!"
echo ""
echo "Next steps:"
echo "1. Review the changes: git diff"
echo "2. Run tests: cargo test --all"
echo "3. If tests pass, commit: git add -A && git commit -m 'cleanup: remove all legacy model and API references'"
echo "4. Remove backups if everything works: rm -rf src.backup.phase9 tests.backup.phase9"
