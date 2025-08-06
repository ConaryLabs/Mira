#!/bin/bash

echo "🔧 Fixing test file imports..."

# Fix all test files to use state::AppState instead of handlers::AppState
echo "Updating test imports..."
find tests -name "*.rs" -type f -exec sed -i 's/handlers::AppState/state::AppState/g' {} +

# Also fix the specific test_helpers.rs file
sed -i 's/use mira_backend::{$/use mira_backend::{/g' tests/test_helpers.rs
sed -i 's/    handlers::AppState,/    state::AppState,/g' tests/test_helpers.rs

# Fix any ResponsesManager references in tests
find tests -name "*.rs" -type f -exec sed -i 's/llm::assistant::{AssistantManager/llm::responses::{ResponsesManager/g' {} +
find tests -name "*.rs" -type f -exec sed -i 's/AssistantManager/ResponsesManager/g' {} +

echo "✅ Test imports fixed! Running cargo build..."
cargo build

echo "🧪 Running cargo test..."
cargo test
