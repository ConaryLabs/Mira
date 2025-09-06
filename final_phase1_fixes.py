#!/usr/bin/env python3
"""
Final fixes for Phase 1 compilation errors
"""

import os
from pathlib import Path
import argparse
import sys
import re

def fix_ws_message_enum(backend_path):
    """Fix the WsClientMessage enum in message.rs"""
    message_path = backend_path / "src" / "api" / "ws" / "message.rs"
    
    print("Fixing WsClientMessage enum...")
    
    with open(message_path, 'r') as f:
        content = f.read()
    
    # Find the enum and fix it properly
    # The issue is the new variants need to be at the same level as the others
    
    # First, let's see what we're working with
    if 'ProjectCommand {' in content:
        # The variants were added but with wrong syntax
        # We need to fix the enum completely
        
        # Find where the enum starts and ends
        enum_start = content.find('pub enum WsClientMessage {')
        if enum_start == -1:
            enum_start = content.find('enum WsClientMessage {')
        
        if enum_start != -1:
            # Find the matching closing brace
            brace_count = 0
            i = enum_start
            enum_end = -1
            started = False
            
            while i < len(content):
                if content[i] == '{':
                    brace_count += 1
                    started = True
                elif content[i] == '}' and started:
                    brace_count -= 1
                    if brace_count == 0:
                        enum_end = i + 1
                        break
                i += 1
            
            if enum_end != -1:
                # Extract everything before and after the enum
                before_enum = content[:enum_start]
                after_enum = content[enum_end:]
                
                # Create the fixed enum
                fixed_enum = '''pub enum WsClientMessage {
    Chat {
        content: String,
        project_id: Option<String>,
        metadata: Option<MessageMetadata>,
    },
    Command {
        command: String,
        args: Option<serde_json::Value>,
    },
    Status {
        message: String,
    },
    Typing {
        active: bool,
    },
    // New message types for WebSocket-only operations
    ProjectCommand {
        method: String,
        params: serde_json::Value,
    },
    MemoryCommand {
        method: String,
        params: serde_json::Value,
    },
    GitCommand {
        method: String,
        params: serde_json::Value,
    },
    FileTransfer {
        operation: String,
        data: serde_json::Value,
    },
}'''
                
                # Reconstruct the file
                content = before_enum + fixed_enum + after_enum
    
    # Also ensure the enum has the Deserialize derive
    if '#[derive(' in content and 'Deserialize' not in content:
        # Add Deserialize to the derive
        content = re.sub(
            r'#\[derive\(([^)]+)\)\]\s*pub enum WsClientMessage',
            r'#[derive(\1, Deserialize)]\npub enum WsClientMessage',
            content
        )
    elif '#[derive' not in content:
        # Add the derive if it's missing entirely
        content = re.sub(
            r'pub enum WsClientMessage',
            r'#[derive(Debug, Clone, Serialize, Deserialize)]\n#[serde(tag = "type", rename_all = "snake_case")]\npub enum WsClientMessage',
            content
        )
    
    with open(message_path, 'w') as f:
        f.write(content)
    
    print("✅ Fixed WsClientMessage enum")

def fix_file_search(backend_path):
    """Fix the file_search.rs path type issue"""
    file_search_path = backend_path / "src" / "services" / "file_search.rs"
    
    if not file_search_path.exists():
        print("⚠️  file_search.rs not found, skipping")
        return
    
    print("Fixing file_search.rs...")
    
    with open(file_search_path, 'r') as f:
        content = f.read()
    
    # Fix the should_index_file call to convert Path to str
    content = re.sub(
        r'should_index_file\(path_obj\)',
        r'should_index_file(path_obj.to_str().unwrap_or(""))',
        content
    )
    
    with open(file_search_path, 'w') as f:
        f.write(content)
    
    print("✅ Fixed file_search.rs")

def fix_unused_warnings(backend_path):
    """Fix unused variable warnings"""
    router_path = backend_path / "src" / "api" / "ws" / "chat" / "message_router.rs"
    
    if router_path.exists():
        print("Fixing unused variable warnings...")
        
        with open(router_path, 'r') as f:
            content = f.read()
        
        # Fix unused meta variables
        content = re.sub(
            r'if let Some\(meta\) = metadata',
            r'if let Some(_meta) = metadata',
            content
        )
        
        with open(router_path, 'w') as f:
            f.write(content)
        
        print("✅ Fixed unused variable warnings")

def main():
    parser = argparse.ArgumentParser(description='Final Phase 1 compilation fixes')
    parser.add_argument('backend_path', help='Path to the Mira backend directory')
    parser.add_argument('--execute', action='store_true', 
                       help='Actually execute the changes (default is dry-run)')
    
    args = parser.parse_args()
    
    backend_path = Path(args.backend_path)
    
    if not args.execute:
        print("\n⚠️  DRY RUN MODE - No changes will be made")
        print("Add --execute flag to actually perform the fixes\n")
        print("Will fix:")
        print("  - WsClientMessage enum in message.rs")
        print("  - file_search.rs path type issue")
        print("  - Unused variable warnings")
        return
    
    print("\n" + "="*60)
    print("APPLYING FINAL PHASE 1 FIXES")
    print("="*60 + "\n")
    
    try:
        fix_ws_message_enum(backend_path)
        fix_file_search(backend_path)
        fix_unused_warnings(backend_path)
        
        print("\n" + "="*60)
        print("✅ All fixes applied!")
        print("="*60 + "\n")
        print("Next: Run 'cargo build' again")
        
    except Exception as e:
        print(f"\n❌ Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
