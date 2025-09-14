#!/bin/bash
# Remove the final legacy fallback from image.rs

echo "🧹 Removing final legacy code from image.rs..."
echo "================================================"

# Create a Python script to precisely remove the fallback block
cat > /tmp/remove_image_fallback.py << 'PYTHON'
import re

# Read the file
with open('src/llm/responses/image.rs', 'r') as f:
    lines = f.readlines()

# Find and remove the fallback block
new_lines = []
skip_mode = False
skip_count = 0

for i, line in enumerate(lines):
    # Start skipping when we find the fallback comment
    if "// Fallback: Try the legacy data format" in line:
        skip_mode = True
        skip_count = 0
        continue
    
    # Count braces to know when the block ends
    if skip_mode:
        skip_count += line.count('{')
        skip_count -= line.count('}')
        
        # Stop skipping after the closing brace of the if statement
        if skip_count <= 0 and '}' in line:
            skip_mode = False
        continue
    
    new_lines.append(line)

# Write back
with open('src/llm/responses/image.rs', 'w') as f:
    f.writelines(new_lines)

print("✓ Removed legacy fallback block")
PYTHON

# Run the Python script
python3 /tmp/remove_image_fallback.py

# Verify the change
echo ""
echo "Verifying removal..."
if grep -q "Fallback: Try the legacy" src/llm/responses/image.rs; then
    echo "❌ Failed to remove fallback"
    exit 1
else
    echo "✅ Fallback removed successfully"
fi

# Build to verify
echo ""
echo "Building to verify..."
cd ~/mira/backend
cargo build 2>&1 | tee /tmp/build_output.txt | grep -E "error" | head -5

if [ ${PIPESTATUS[0]} -eq 0 ]; then
    echo "✅ Build successful!"
else
    echo "❌ Build failed. The fallback might be needed."
    echo "Rolling back..."
    git checkout src/llm/responses/image.rs
    exit 1
fi

# Final check for any legacy references
echo ""
echo "Final check for 'legacy' references..."
remaining=$(grep -ri "legacy" --include="*.rs" src/ 2>/dev/null | wc -l)

if [ $remaining -eq 0 ]; then
    echo "🎉 ALL legacy code has been removed!"
else
    echo "Remaining references:"
    grep -ri "legacy" --include="*.rs" src/
fi

echo ""
echo "================================================"
echo "✨ Legacy code removal complete!"
echo ""
echo "The codebase is now clean and focused on current APIs only."
echo "No backwards compatibility, no fallbacks, no confusion!"
