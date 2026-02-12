// crates/mira-server/src/cli/config.rs
// CLI config subcommand for viewing and updating Mira configuration

use anyhow::{Result, bail};
use mira::config::MiraConfig;
use mira::llm::Provider;
use std::io::Write;

/// Valid config keys that can be set
const VALID_KEYS: &[&str] = &["background_provider", "default_provider"];

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
                Some(v) => println!(
                    "default_provider    = \"{}\" (from DEFAULT_LLM_PROVIDER env)",
                    v
                ),
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

    // Use the canonical provider parser instead of a hardcoded list
    if Provider::from_str(value).is_none() {
        bail!(
            "Unknown provider '{}'. Valid providers: deepseek, zhipu (or glm), ollama, sampling",
            value
        );
    }

    let path = MiraConfig::config_path();

    // Read existing config or start fresh
    let content = std::fs::read_to_string(&path).unwrap_or_default();

    // Parse existing TOML — fail loudly if malformed instead of silently overwriting
    let mut table: toml::Table = match toml::from_str(&content) {
        Ok(t) => t,
        Err(e) if content.is_empty() => {
            // Empty/missing file is fine — start fresh
            let _ = e;
            toml::Table::new()
        }
        Err(e) => {
            bail!(
                "Cannot update config: {} has a syntax error.\n  Error: {}\n  Fix the file manually or delete it to start fresh.",
                path.display(),
                e
            );
        }
    };

    // Ensure [llm] section exists and is a table
    let llm = table
        .entry("llm")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));

    if let toml::Value::Table(llm_table) = llm {
        llm_table.insert(key.to_string(), toml::Value::String(value.to_string()));
    } else {
        bail!(
            "Cannot update config: 'llm' in {} is not a table section.\n  Expected [llm], found: llm = {:?}\n  Fix the file manually or delete it to start fresh.",
            path.display(),
            llm
        );
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
