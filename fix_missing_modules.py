#!/usr/bin/env python3
"""
Fix missing modules and imports after Phase 1
"""

import os
from pathlib import Path
import argparse
import sys
import re

def create_utils_mod(backend_path):
    """Create the missing utils module"""
    utils_path = backend_path / "src" / "utils.rs"
    
    print("Creating utils.rs...")
    
    content = '''// src/utils.rs
// Utility functions module

use std::time::{SystemTime, UNIX_EPOCH};

/// Get current timestamp in seconds
pub fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Get current timestamp in milliseconds
pub fn get_timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
'''
    
    with open(utils_path, 'w') as f:
        f.write(content)
    
    print("✅ Created utils.rs")

def create_persona_mod(backend_path):
    """Create the missing persona module"""
    persona_path = backend_path / "src" / "persona.rs"
    
    print("Creating persona.rs...")
    
    content = '''// src/persona.rs
// Persona overlay module for personality management

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonaOverlay {
    pub name: String,
    pub description: Option<String>,
    pub system_prompt_addon: Option<String>,
    pub active: bool,
}

impl Default for PersonaOverlay {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            description: None,
            system_prompt_addon: None,
            active: true,
        }
    }
}

impl PersonaOverlay {
    pub fn new(name: String) -> Self {
        Self {
            name,
            description: None,
            system_prompt_addon: None,
            active: true,
        }
    }
    
    pub fn with_prompt(mut self, prompt: String) -> Self {
        self.system_prompt_addon = Some(prompt);
        self
    }
}
'''
    
    with open(persona_path, 'w') as f:
        f.write(content)
    
    print("✅ Created persona.rs")

def fix_main_rs(backend_path):
    """Add missing modules to main.rs"""
    main_path = backend_path / "src" / "main.rs"
    
    print("Fixing main.rs modules...")
    
    with open(main_path, 'r') as f:
        content = f.read()
    
    # Add persona module if missing
    if 'mod persona;' not in content:
        # Add it after other mod declarations
        content = re.sub(
            r'(mod utils;)',
            r'\1\nmod persona;',
            content
        )
    
    # Fix AppState::new() to use create_app_state
    content = re.sub(
        r'let app_state = Arc::new\(AppState::new\(\)\.await\?\);',
        r'let app_state = state::create_app_state().await?;',
        content
    )
    
    with open(main_path, 'w') as f:
        f.write(content)
    
    print("✅ Fixed main.rs")

def fix_lib_rs(backend_path):
    """Add missing modules to lib.rs"""
    lib_path = backend_path / "src" / "lib.rs"
    
    if not lib_path.exists():
        print("Creating lib.rs...")
        content = '''// src/lib.rs
// Library root for mira-backend

pub mod api;
pub mod config;
pub mod git;
pub mod handlers;
pub mod llm;
pub mod memory;
pub mod persona;
pub mod project;
pub mod services;
pub mod state;
pub mod utils;

// Re-export commonly used items
pub use config::CONFIG;
pub use state::AppState;
'''
        with open(lib_path, 'w') as f:
            f.write(content)
        print("✅ Created lib.rs")
    else:
        print("Fixing lib.rs...")
        with open(lib_path, 'r') as f:
            content = f.read()
        
        # Add missing modules
        if 'pub mod persona;' not in content:
            content = re.sub(
                r'(pub mod handlers;)',
                r'\1\npub mod persona;',
                content
            )
        
        if 'pub mod utils;' not in content:
            content = re.sub(
                r'(pub mod state;)',
                r'\1\npub mod utils;',
                content
            )
        
        with open(lib_path, 'w') as f:
            f.write(content)
        print("✅ Fixed lib.rs")

def fix_imports(backend_path):
    """Fix various import issues"""
    
    # Fix session_state.rs
    session_state_path = backend_path / "src" / "api" / "ws" / "session_state.rs"
    if session_state_path.exists():
        print("Fixing session_state.rs imports...")
        with open(session_state_path, 'r') as f:
            content = f.read()
        
        content = re.sub(
            r'use crate::persona::PersonaOverlay;',
            r'use crate::persona::PersonaOverlay;',
            content
        )
        
        with open(session_state_path, 'w') as f:
            f.write(content)
    
    # Fix other files with persona imports
    for file_path in [
        backend_path / "src" / "services" / "chat" / "response.rs",
        backend_path / "src" / "services" / "chat" / "mod.rs",
        backend_path / "src" / "state.rs"
    ]:
        if file_path.exists():
            print(f"Fixing {file_path.name} imports...")
            with open(file_path, 'r') as f:
                content = f.read()
            
            # The imports should already be correct, just make sure
            content = re.sub(
                r'use crate::persona::PersonaOverlay;',
                r'use crate::persona::PersonaOverlay;',
                content
            )
            
            with open(file_path, 'w') as f:
                f.write(content)

def main():
    parser = argparse.ArgumentParser(description='Fix missing modules and imports')
    parser.add_argument('backend_path', help='Path to the Mira backend directory')
    parser.add_argument('--execute', action='store_true', 
                       help='Actually execute the changes (default is dry-run)')
    
    args = parser.parse_args()
    
    backend_path = Path(args.backend_path)
    
    if not args.execute:
        print("\n⚠️  DRY RUN MODE - No changes will be made")
        print("Add --execute flag to actually perform the fixes\n")
        print("Will create/fix:")
        print("  - utils.rs module")
        print("  - persona.rs module")
        print("  - lib.rs")
        print("  - main.rs AppState initialization")
        print("  - Various import statements")
        return
    
    print("\n" + "="*60)
    print("FIXING MISSING MODULES AND IMPORTS")
    print("="*60 + "\n")
    
    try:
        create_utils_mod(backend_path)
        create_persona_mod(backend_path)
        fix_lib_rs(backend_path)
        fix_main_rs(backend_path)
        fix_imports(backend_path)
        
        print("\n" + "="*60)
        print("✅ All modules and imports fixed!")
        print("="*60 + "\n")
        print("Next: Run 'cargo build' again")
        print("\nNote: There will be many unused import warnings - that's OK!")
        print("These are from the HTTP purge and will be cleaned up as we implement")
        print("the WebSocket handlers in Phases 2-6.")
        
    except Exception as e:
        print(f"\n❌ Error: {e}", file=sys.stderr)
        sys.exit(1)

if __name__ == "__main__":
    main()
