#!/usr/bin/env python3
"""
Claude reference cleanup script for Mira backend migration
Identifies and helps fix Claude/Anthropic references in the codebase
"""

import os
import re
from pathlib import Path
from collections import defaultdict

# Files we're going to DELETE entirely
DELETE_FILES = [
    "src/llm/client/mod.rs",
    "src/llm/client/config.rs",
]

# Simple string replacements (comments, simple references)
SIMPLE_REPLACEMENTS = {
    # Comment updates
    r"Claude uses similar": "LLMs use similar",
    r"Claude sometimes": "LLMs sometimes",
    r"Claude with extended thinking": "Models with extended reasoning",
    r"Claude decides": "the model decides",
    r"Claude didn't": "Model didn't",
    r"Claude ended": "Model ended",
    r"Claude called": "Model called",
    r"Claude finished": "Model finished",
    r"trust Claude": "trust the model",
    r"Claude API": "LLM API",
    r"Claude client": "LLM client",
    r"Claude provider": "LLM provider",
    r"Claude-specific": "Provider-specific",
    r"Claude format": "unified format",
    r"Claude-format": "unified format",
    r"Claude-compatible": "provider-compatible",
    
    # Variable/function name patterns (careful with these)
    r"claude_processor::": "// REMOVED: claude_processor::",
    r"claude_content": "content",
    r"claude_response": "response",
    r"ClaudeProvider": "// REMOVED: ClaudeProvider",
}

# CONFIG references that need removal
CONFIG_ANTHROPIC_PATTERNS = [
    r"CONFIG\.anthropic_api_key",
    r"CONFIG\.anthropic_base_url", 
    r"CONFIG\.anthropic_model",
    r"CONFIG\.anthropic_max_tokens",
]

def scan_file(filepath):
    """Scan a file for Claude/Anthropic references"""
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            content = f.read()
            lines = content.split('\n')
    except Exception as e:
        return None
    
    issues = []
    
    # Check for claude_processor imports/usage
    if 'claude_processor' in content:
        for i, line in enumerate(lines, 1):
            if 'claude_processor' in line:
                issues.append({
                    'type': 'claude_processor',
                    'line': i,
                    'content': line.strip(),
                    'severity': 'HIGH'
                })
    
    # Check for CONFIG.anthropic_ usage
    for pattern in CONFIG_ANTHROPIC_PATTERNS:
        if re.search(pattern, content):
            for i, line in enumerate(lines, 1):
                if re.search(pattern, line):
                    issues.append({
                        'type': 'anthropic_config',
                        'line': i,
                        'content': line.strip(),
                        'severity': 'HIGH'
                    })
    
    # Check for ClaudeProvider usage
    if 'ClaudeProvider' in content:
        for i, line in enumerate(lines, 1):
            if 'ClaudeProvider' in line:
                issues.append({
                    'type': 'claude_provider',
                    'line': i,
                    'content': line.strip(),
                    'severity': 'HIGH'
                })
    
    # Check for simple comment references
    for i, line in enumerate(lines, 1):
        lower = line.lower()
        if 'claude' in lower and '//' in line:
            # It's a comment
            issues.append({
                'type': 'comment',
                'line': i,
                'content': line.strip(),
                'severity': 'LOW'
            })
    
    return issues if issues else None

def main():
    backend_path = Path('.')
    
    print("=" * 80)
    print("CLAUDE CLEANUP ANALYSIS")
    print("=" * 80)
    print()
    
    # Files to delete
    print("FILES TO DELETE:")
    print("-" * 80)
    for file in DELETE_FILES:
        filepath = backend_path / file
        if filepath.exists():
            print(f"  ❌ {file}")
        else:
            print(f"  ⚠️  {file} (already deleted)")
    print()
    
    # Scan all rust files
    rust_files = list(backend_path.glob('**/*.rs'))
    
    files_by_severity = defaultdict(list)
    
    for filepath in rust_files:
        if any(str(filepath).endswith(df) for df in DELETE_FILES):
            continue  # Skip files we're deleting
        
        rel_path = filepath.relative_to(backend_path)
        issues = scan_file(filepath)
        
        if issues:
            high_severity = any(i['severity'] == 'HIGH' for i in issues)
            files_by_severity['HIGH' if high_severity else 'LOW'].append((rel_path, issues))
    
    # Print HIGH severity issues
    if files_by_severity['HIGH']:
        print("HIGH SEVERITY ISSUES (Require code changes):")
        print("-" * 80)
        for filepath, issues in sorted(files_by_severity['HIGH']):
            print(f"\n📄 {filepath}")
            high_issues = [i for i in issues if i['severity'] == 'HIGH']
            for issue in high_issues[:5]:  # Show first 5
                print(f"   Line {issue['line']:4d}: {issue['content']}")
            if len(high_issues) > 5:
                print(f"   ... and {len(high_issues) - 5} more")
        print()
    
    # Print LOW severity issues
    if files_by_severity['LOW']:
        print("LOW SEVERITY ISSUES (Comments/simple fixes):")
        print("-" * 80)
        for filepath, issues in sorted(files_by_severity['LOW']):
            low_issues = [i for i in issues if i['severity'] == 'LOW']
            if low_issues:
                print(f"  {filepath}: {len(low_issues)} comment(s)")
        print()
    
    # Summary
    print("=" * 80)
    print("SUMMARY:")
    print("-" * 80)
    total_high = sum(len([i for i in issues if i['severity'] == 'HIGH']) 
                     for _, issues in files_by_severity['HIGH'])
    total_low = sum(len([i for i in issues if i['severity'] == 'LOW']) 
                    for _, issues in files_by_severity['LOW'])
    
    print(f"  Files to delete: {len(DELETE_FILES)}")
    print(f"  Files with HIGH severity issues: {len(files_by_severity['HIGH'])}")
    print(f"  Files with LOW severity issues: {len(files_by_severity['LOW'])}")
    print(f"  Total HIGH severity issues: {total_high}")
    print(f"  Total LOW severity issues: {total_low}")
    print()
    
    print("NEXT STEPS:")
    print("-" * 80)
    print("1. Delete the listed files:")
    for file in DELETE_FILES:
        print(f"     rm {file}")
    print()
    print("2. Files needing code changes (HIGH severity):")
    print("     - These need manual review and updates")
    print("     - Focus on removing claude_processor imports/usage")
    print("     - Update CONFIG.anthropic_* to new config vars")
    print()
    print("3. Update comments (LOW severity):")
    print("     - Can be done with sed/find-replace")
    print("     - Or manually during code review")
    print()

if __name__ == '__main__':
    main()
