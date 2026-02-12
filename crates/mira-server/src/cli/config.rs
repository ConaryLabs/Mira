// crates/mira-server/src/cli/config.rs
// CLI config subcommand for viewing and updating Mira configuration

use mira::config::MiraConfig;
use anyhow::{bail, Result};
use std::io::Write;

/// Valid config keys that can be set
const VALID_KEYS: &[&str] = &["background_provider", "default_provider"];

/// Valid provider values
const VALID_PROVIDERS: &[&str] = &["deepseek", "zhipu", "ollama"];

/// Run `mira config show`
pub fn run_config_show() -> Result<()> {
    let config = MiraConfig::load();
    let path = MiraConfig::config_path();

    println!("Config file: {}", path.display());
    println!();

    // Show background_provider
    match config.background_provider() {
        Some(p) => println!("background_provider = \"{}\"", p),
        None => println!("background_provider = (not set)"),
    }

    // Show default_provider
    match config.default_provider() {
        Some(p) => println!("default_provider    = \"{}\"", p),
        None => {
            // Check env var fallback
            match std::env::var("DEFAULT_LLM_PROVIDER").ok() {
                Some(v) => println!("default_provider    = \"{}\" (from DEFAULT_LLM_PROVIDER env)", v),
                None => println!("default_provider    = (not set)"),
            }
        }
    }

    Ok(())
}

/// Run `mira config set <key> <value>`
pub fn run_config_set(key: &str, value: &str) -> Result<()> {
    if !VALID_KEYS.contains(&key) {
        bail!(
            "Unknown config key '{}'. Valid keys: {}",
            key,
            VALID_KEYS.join(", ")
        );
    }

    if !VALID_PROVIDERS.contains(&value) {
        bail!(
            "Unknown provider '{}'. Valid providers: {}",
            value,
            VALID_PROVIDERS.join(", ")
        );
    }

    let path = MiraConfig::config_path();

    // Read existing config or start fresh
    let content = std::fs::read_to_string(&path).unwrap_or_default();

    // Parse, update, and rewrite
    let mut table: toml::Table = toml::from_str(&content).unwrap_or_default();

    // Ensure [llm] section exists
    let llm = table
        .entry("llm")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));

    if let toml::Value::Table(llm_table) = llm {
        llm_table.insert(key.to_string(), toml::Value::String(value.to_string()));
    }

    // Write back with header comment
    let toml_str = toml::to_string_pretty(&table)?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = std::fs::File::create(&path)?;
    writeln!(file, "# Mira configuration\n")?;
    file.write_all(toml_str.as_bytes())?;

    println!("Set {} = \"{}\" in {}", key, value, path.display());

    Ok(())
}
