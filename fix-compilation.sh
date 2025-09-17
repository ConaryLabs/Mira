#!/bin/bash
# fix-compilation.sh - Quick fixes for compilation errors

echo "🔧 Fixing compilation errors..."

# 1. Remove the migration import line from store.rs
echo "Removing old migration import..."
sed -i '/use crate::memory::storage::sqlite::migration;/d' src/memory/storage/sqlite/store.rs

# 2. Comment out any references to migration in store.rs
echo "Commenting out migration references..."
sed -i 's/migration::/\/\/ migration::/g' src/memory/storage/sqlite/store.rs

# 3. Fix the unused variable warning
echo "Fixing unused variable warning..."
sed -i 's/session_id: String,/_session_id: String,/g' src/llm/message_analyzer.rs

echo "✅ Fixes applied!"
echo ""
echo "Note: The embed_document_chunk error means you need to:"
echo "1. Update src/config/mod.rs with the new fields (if not already done)"
echo "2. Then rebuild"
