// src/hooks/mod.rs
// Claude Code hook handlers

pub mod permission;

use anyhow::Result;

/// Read hook input from stdin (Claude Code passes JSON)
pub fn read_hook_input() -> Result<serde_json::Value> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
    let json: serde_json::Value = serde_json::from_str(&input)?;
    Ok(json)
}

/// Write hook output to stdout
pub fn write_hook_output(output: &serde_json::Value) {
    println!("{}", serde_json::to_string(output).unwrap_or_default());
}
