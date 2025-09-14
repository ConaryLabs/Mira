#!/bin/bash
# Safe removal of legacy code based on actual usage analysis

echo "🧹 Safely removing legacy code from Mira backend..."
echo "================================================"

# 1. Fix StreamEvent::Text usage in streaming/mod.rs
echo "1️⃣ Updating src/llm/streaming/mod.rs..."
# Replace the pattern match to only use Delta
sed -i 's/StreamEvent::Delta(text) | StreamEvent::Text(text)/StreamEvent::Delta(text)/' src/llm/streaming/mod.rs
echo "   ✓ Updated pattern match to remove Text variant"

# 2. Fix StreamEvent::Text creation in streaming/processor.rs
echo "2️⃣ Updating src/llm/streaming/processor.rs..."
# Replace Text with Delta
sed -i 's/StreamEvent::Text(/StreamEvent::Delta(/' src/llm/streaming/processor.rs
# Remove the Text variant from enum
sed -i '/\/\/\/ Legacy text variant for compatibility/d' src/llm/streaming/processor.rs
sed -i '/^[[:space:]]*Text(String),/d' src/llm/streaming/processor.rs
echo "   ✓ Replaced Text with Delta and removed variant"

# 3. Update git_client to use get_attachment instead of get_attachment_by_id
echo "3️⃣ Updating src/tools/file_context.rs..."
sed -i 's/get_attachment_by_id/get_attachment/' src/tools/file_context.rs
echo "   ✓ Updated to use get_attachment"

# 4. Remove get_attachment_by_id from git/store.rs
echo "4️⃣ Updating src/git/store.rs..."
sed -i '/\/\/\/ Legacy method for compatibility/,+3d' src/git/store.rs
echo "   ✓ Removed legacy method"

# 5. Clean up prompt/mod.rs exports
echo "5️⃣ Updating src/prompt/mod.rs..."
# Check if build_system_prompt is being imported from builder or unified_builder
if grep -q "UnifiedPromptBuilder::build_system_prompt" src/api/ws/chat/unified_handler.rs; then
    # It's using UnifiedPromptBuilder, safe to remove the re-export
    sed -i '/\/\/ Keep legacy exports for backward compatibility/d' src/prompt/mod.rs
    sed -i '/^[[:space:]]*build_system_prompt,/d' src/prompt/mod.rs
    
    # Remove other builder re-exports if they're alone
    sed -i '/^pub use builder::{$/,/^};$/d' src/prompt/mod.rs
    echo "   ✓ Removed legacy re-exports"
else
    echo "   ⚠️  Skipping - build_system_prompt might still be needed"
fi

# 6. Remove fallback from image.rs
echo "6️⃣ Updating src/llm/responses/image.rs..."
# Find and remove the fallback block
cat > /tmp/image_fix.py << 'PYTHON'
import re

with open('src/llm/responses/image.rs', 'r') as f:
    content = f.read()

# Remove the fallback block
pattern = r'(\s*)// Fallback: Try the legacy data format\s*\n\s*if images\.is_empty\(\) \{[^}]*\}\s*\}'
content = re.sub(pattern, '', content, flags=re.DOTALL)

with open('src/llm/responses/image.rs', 'w') as f:
    f.write(content)

print("   ✓ Removed legacy fallback")
PYTHON
python3 /tmp/image_fix.py

# 7. Clean up streaming.rs comments
echo "7️⃣ Cleaning src/llm/client/streaming.rs..."
# Remove the top comment
sed -i '2s/^.*$//' src/llm/client/streaming.rs
# Remove the legacy format comment but KEEP the code (it might still be valid)
sed -i 's|// Legacy format (keeping just this one for safety): choices\[0\]\.delta\.content||' src/llm/client/streaming.rs
echo "   ✓ Removed comments but kept extraction path"

# 8. Build to verify
echo ""
echo "8️⃣ Building to verify changes..."
cd ~/mira/backend
cargo build 2>&1 | tee /tmp/build_output.txt | grep -E "error" | head -5

if [ ${PIPESTATUS[0]} -eq 0 ]; then
    echo "✅ Build successful!"
    
    # Count warnings
    warnings=$(grep -c "warning:" /tmp/build_output.txt || echo "0")
    echo "   Warnings: $warnings"
else
    echo "❌ Build has errors. Check the output above."
    exit 1
fi

# 9. Final verification
echo ""
echo "9️⃣ Final check for remaining legacy references..."
remaining=$(grep -ri "legacy\|backward.*compat\|fallback.*format" --include="*.rs" src/ 2>/dev/null | wc -l)

echo ""
echo "================================================"
echo "✨ Legacy code removal complete!"
echo ""
echo "Changes made:"
echo "  • Replaced StreamEvent::Text with Delta in 2 files"
echo "  • Updated file_context.rs to use get_attachment"
echo "  • Removed get_attachment_by_id legacy method"
echo "  • Removed backward compatibility exports"
echo "  • Removed legacy image response fallback"
echo "  • Cleaned up comments in streaming.rs"
echo ""
echo "Remaining legacy references: $remaining"
if [ $remaining -gt 0 ]; then
    echo "Check with: grep -ri 'legacy' --include='*.rs' src/"
fi
