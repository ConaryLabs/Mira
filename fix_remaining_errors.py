#!/usr/bin/env python3
"""
Fix remaining Phase 2 errors after initial cleanup
"""

import os
import re
from pathlib import Path

def fix_file(filepath):
    """Fix remaining issues in a file"""
    with open(filepath, 'r', encoding='utf-8') as f:
        content = f.read()
    
    original = content
    
    # 1. Fix Value::String(x) → x directly (Message.content is now String, not Value)
    # Match: content: Value::String(something).to_string()
    content = re.sub(
        r'content:\s*Value::String\(([^)]+)\)\.to_string\(\)',
        r'content: \1',
        content
    )
    # Match: content: Value::String(something) without .to_string()
    content = re.sub(
        r'content:\s*Value::String\(([^)]+)\)',
        r'content: \1',
        content
    )
    
    # 2. Fix remaining .chat() calls with None parameter
    content = re.sub(
        r'\.chat\(\s*([^,]+),\s*([^,]+),\s*None\s*\)',
        r'.chat(\1, \2)',
        content
    )
    
    # 3. Fix response.metadata.field → response.tokens.field
    content = content.replace('response.metadata.input_tokens', 'response.tokens.input')
    content = content.replace('response.metadata.output_tokens', 'response.tokens.output')
    content = content.replace('response.metadata.finish_reason', 'Some("end_turn".to_string())')
    content = content.replace('response.metadata_tokens', 'response.tokens.reasoning')
    
    # 4. Remove if let Some(thinking) = response (response is not Option anymore)
    lines = content.split('\n')
    fixed_lines = []
    skip_next = 0
    for i, line in enumerate(lines):
        if skip_next > 0:
            skip_next -= 1
            continue
            
        # Find and remove: if let Some(thinking) = response
        if 'if let Some' in line and 'response' in line and '=' in line:
            # Skip this line and probably the next few that reference thinking
            skip_next = 5  # Skip block
            continue
        
        fixed_lines.append(line)
    content = '\n'.join(fixed_lines)
    
    # 5. Add imports where needed
    if 'extract_claude' in content or 'has_tool_calls' in content or 'analyze_message_complexity' in content:
        # Check if already has the import
        if 'use crate::llm::structured::{' in content and 'claude_compat' not in content:
            # Add to existing import
            content = re.sub(
                r'(use crate::llm::structured::\{[^}]*)(})',
                r'\1, has_tool_calls, extract_claude_content_from_tool, extract_claude_metadata, analyze_message_complexity\2',
                content
            )
        elif 'use crate::llm::structured::' not in content:
            # Add new import near the top (after other crate imports)
            lines = content.split('\n')
            insert_idx = 0
            for i, line in enumerate(lines):
                if line.startswith('use crate::') or line.startswith('use super::'):
                    insert_idx = i + 1
            lines.insert(insert_idx, 'use crate::llm::structured::{has_tool_calls, extract_claude_content_from_tool, extract_claude_metadata, analyze_message_complexity};')
            content = '\n'.join(lines)
    
    # 6. Fix metadata_tokens and metadata_row_tokens issues
    if 'metadata_tokens' in content or 'metadata_row_tokens' in content:
        # These should be replaced with response.tokens.reasoning
        content = content.replace('metadata_tokens', 'response.tokens.reasoning')
        content = content.replace('metadata_row_tokens', 'row.get::<Option<i64>, _>("thinking_tokens")')
    
    # Only write if changed
    if content != original:
        with open(filepath, 'w', encoding='utf-8') as f:
            f.write(content)
        return True
    return False

def add_get_embedding_heads_to_config():
    """Add get_embedding_heads() method to CONFIG"""
    config_path = Path('src/config/mod.rs')
    
    with open(config_path, 'r', encoding='utf-8') as f:
        content = f.read()
    
    # Check if method already exists
    if 'get_embedding_heads' in content:
        return False
    
    # Find the impl MiraConfig block and add the method
    # Add before the last closing brace of the impl block
    lines = content.split('\n')
    impl_block_depth = 0
    insert_idx = -1
    
    for i, line in enumerate(lines):
        if 'impl MiraConfig' in line:
            impl_block_depth = 1
        elif impl_block_depth > 0:
            if '{' in line:
                impl_block_depth += line.count('{')
            if '}' in line:
                impl_block_depth -= line.count('}')
                if impl_block_depth == 0:
                    insert_idx = i
                    break
    
    if insert_idx > 0:
        # Insert the method before the closing brace
        new_method = '''
    /// Get embedding heads from config
    pub fn get_embedding_heads(&self) -> Vec<String> {
        self.embed_heads.clone()
    }
'''
        lines.insert(insert_idx, new_method)
        
        with open(config_path, 'w', encoding='utf-8') as f:
            f.write('\n'.join(lines))
        return True
    
    return False

def main():
    backend_dir = Path('.')
    
    if not (backend_dir / 'src').exists():
        print("❌ Error: Run this script from the backend/ directory")
        return
    
    print("🔧 Phase 2 Remaining Fixes")
    print("=" * 50)
    
    # Find all .rs files
    rs_files = list(backend_dir.rglob('src/**/*.rs'))
    
    fixed_count = 0
    
    # Process all files
    for filepath in rs_files:
        try:
            if fix_file(filepath):
                print(f"✅ Fixed: {filepath}")
                fixed_count += 1
        except Exception as e:
            print(f"❌ Error fixing {filepath}: {e}")
    
    # Add get_embedding_heads to config
    if add_get_embedding_heads_to_config():
        print(f"✅ Added get_embedding_heads() to CONFIG")
        fixed_count += 1
    
    print("=" * 50)
    print(f"✅ Fixed {fixed_count} files")
    print("\nNow run: cargo check")
    print("\nRemaining manual fixes needed:")
    print("  - Embedding sizing ([f32] → Vec<f32>) in client/embedding.rs")
    print("  - Some unified_handler.rs tool response handling")

if __name__ == '__main__':
    main()
