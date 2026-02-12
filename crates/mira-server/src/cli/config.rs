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
    let provider = match Provider::from_str(value) {
        Some(p) => p,
        None => bail!(
            "Unknown provider '{}'. Valid providers: deepseek, zhipu (or glm), ollama",
            value
        ),
    };

    // Sampling is not a real background provider — it has no LlmClient impl,
    // so setting it here would make the statusline lie about the active provider.
    if provider == Provider::Sampling {
        bail!(
            "Provider 'sampling' cannot be set via config. \
             MCP sampling is used automatically as a last-resort fallback \
             when no API keys are configured."
        );
    }

    let path = MiraConfig::config_path();

    // Read existing config — only treat missing file as empty, all other errors are fatal
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => bail!(
            "Cannot read config file {}: {}\n  Check file permissions and encoding.",
            path.display(),
            e
        ),
    };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_unknown_key() {
        let err = run_config_set("unknown_key", "deepseek").unwrap_err();
        assert!(
            err.to_string().contains("Unknown config key"),
            "Should reject unknown key, got: {}",
            err
        );
    }

    #[test]
    fn test_reject_unknown_provider() {
        let err = run_config_set("background_provider", "gpt4").unwrap_err();
        assert!(
            err.to_string().contains("Unknown provider"),
            "Should reject unknown provider, got: {}",
            err
        );
    }

    #[test]
    fn test_reject_sampling_provider() {
        let err = run_config_set("background_provider", "sampling").unwrap_err();
        assert!(
            err.to_string().contains("cannot be set via config"),
            "Should reject sampling, got: {}",
            err
        );
    }

    #[test]
    fn test_reject_sampling_as_default() {
        let err = run_config_set("default_provider", "sampling").unwrap_err();
        assert!(
            err.to_string().contains("cannot be set via config"),
            "Should reject sampling for default_provider too, got: {}",
            err
        );
    }

    #[test]
    fn test_valid_providers_are_accepted() {
        // Verify valid providers parse correctly and aren't Sampling
        // (i.e. they'd pass the validation gates in run_config_set without touching disk)
        for name in &["deepseek", "zhipu", "glm", "ollama"] {
            let provider = Provider::from_str(name);
            assert!(
                provider.is_some(),
                "Provider '{}' should parse successfully",
                name
            );
            assert_ne!(
                provider.unwrap(),
                Provider::Sampling,
                "Provider '{}' should not resolve to Sampling",
                name
            );
        }
    }
}
