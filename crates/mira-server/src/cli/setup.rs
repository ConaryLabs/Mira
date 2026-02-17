// crates/mira-server/src/cli/setup.rs
// Interactive setup wizard for Mira configuration

use anyhow::Result;
use dialoguer::{Confirm, Password, Select};
use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

/// ASCII banner for setup
const BANNER: &str = r#"
  __  __ _
 |  \/  (_)_ __ __ _
 | |\/| | | '__/ _` |
 | |  | | | | | (_| |
 |_|  |_|_|_|  \__,_|

  Setup Wizard
"#;

/// Known provider keys that indicate a configured provider
const PROVIDER_KEYS: &[&str] = &[
    "DEEPSEEK_API_KEY",
    "OPENAI_API_KEY",
    "BRAVE_API_KEY",
    "OLLAMA_HOST",
];

/// Result of evaluating the setup summary
#[derive(Debug, PartialEq)]
enum SetupSummary {
    /// No providers at all (no existing, no new)
    NoProviders,
    /// Existing providers found but no new keys added
    ExistingUnchanged,
    /// New keys were configured
    NewKeysConfigured,
}

/// Determine the setup summary based on existing and newly collected keys
fn setup_summary(
    existing: &BTreeMap<String, String>,
    new_keys: &BTreeMap<String, String>,
) -> SetupSummary {
    if !new_keys.is_empty() {
        return SetupSummary::NewKeysConfigured;
    }
    let has_existing_provider = existing.keys().any(|k| PROVIDER_KEYS.contains(&k.as_str()));
    if has_existing_provider {
        SetupSummary::ExistingUnchanged
    } else {
        SetupSummary::NoProviders
    }
}

/// Run the setup wizard
pub async fn run(check: bool, non_interactive: bool) -> Result<()> {
    if check {
        return run_check().await;
    }

    if !non_interactive && !std::io::stdin().is_terminal() {
        anyhow::bail!(
            "Setup requires an interactive terminal.\n\
             Use --yes for non-interactive mode, or run in a terminal."
        );
    }

    println!("{}", BANNER);

    let mira_dir = mira_dir()?;
    let env_path = mira_dir.join(".env");
    let config_path = mira_dir.join("config.toml");

    // Step 1: Check environment
    ensure_dir(&mira_dir)?;
    let existing = read_existing_env(&env_path);

    if std::env::var("MIRA_DISABLE_LLM")
        .ok()
        .filter(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .is_some()
    {
        println!("Warning: MIRA_DISABLE_LLM is set. LLM features are currently disabled.");
        if !non_interactive
            && !Confirm::new()
                .with_prompt("Continue setup anyway?")
                .default(true)
                .interact()?
        {
            println!("Setup cancelled.");
            return Ok(());
        }
    }

    let mut keys: BTreeMap<String, String> = BTreeMap::new();
    let mut ollama_selected = false;

    if non_interactive {
        // Non-interactive: skip API key prompts, just auto-detect Ollama
        println!("Running in non-interactive mode (--yes).");
        println!("Skipping API key prompts. Detecting Ollama...");
    } else {
        // Step 2: Background Intelligence (DeepSeek)
        println!("\n--- Background Intelligence (DeepSeek) ---");
        println!("  Powers insights, summaries, and semantic analysis.");
        let ds_choices = &["DeepSeek (recommended)", "Skip"];
        let ds_sel = Select::new()
            .with_prompt("Configure DeepSeek")
            .items(ds_choices)
            .default(0)
            .interact()?;

        if ds_sel == 0 {
            if let Some(key) = prompt_api_key(
                "DeepSeek",
                "DEEPSEEK_API_KEY",
                existing.get("DEEPSEEK_API_KEY"),
            )
            .await?
            {
                keys.insert("DEEPSEEK_API_KEY".into(), key);
            }
        } else {
            println!(
                "Skipping DeepSeek. Background intelligence will be unavailable without DeepSeek or Ollama."
            );
        }

        // Step 3: Embeddings (OpenAI)
        println!("\n--- Embeddings (semantic search) ---");
        let embed_choices = &["OpenAI (required for semantic search)", "Skip"];
        let embed_sel = Select::new()
            .with_prompt("Choose embeddings provider")
            .items(embed_choices)
            .default(0)
            .interact()?;

        if embed_sel == 0 {
            if let Some(key) =
                prompt_api_key("OpenAI", "OPENAI_API_KEY", existing.get("OPENAI_API_KEY")).await?
            {
                keys.insert("OPENAI_API_KEY".into(), key);
            }
        } else {
            println!("Skipping embeddings. Semantic search will be unavailable.");
        }
    }

    // Step 4: Local LLM (Ollama)
    println!("\n--- Local LLM (Ollama) ---");
    let ollama_host = std::env::var("OLLAMA_HOST")
        .ok()
        .unwrap_or_else(|| "http://localhost:11434".into());

    match detect_ollama(&ollama_host).await {
        OllamaStatus::Available(models) => {
            println!(
                "Ollama detected at {} with {} model(s).",
                ollama_host,
                models.len()
            );
            if !models.is_empty() {
                if non_interactive {
                    // Auto-select first model
                    println!("Auto-selected model: {}", models[0]);
                    keys.insert("OLLAMA_HOST".into(), ollama_host.clone());
                    keys.insert("OLLAMA_MODEL".into(), models[0].clone());
                    ollama_selected = true;
                } else {
                    let mut choices: Vec<&str> = models.iter().map(|s| s.as_str()).collect();
                    choices.push("Skip Ollama");
                    let sel = Select::new()
                        .with_prompt("Select Ollama model for background tasks")
                        .items(&choices)
                        .default(0)
                        .interact()?;
                    if sel < models.len() {
                        keys.insert("OLLAMA_HOST".into(), ollama_host.clone());
                        keys.insert("OLLAMA_MODEL".into(), models[sel].clone());
                        ollama_selected = true;
                    } else {
                        println!("Skipping Ollama.");
                    }
                }
            } else {
                println!(
                    "Ollama is running but no models found. Pull a model with: ollama pull llama3.3"
                );
                if !non_interactive
                    && Confirm::new()
                        .with_prompt("Save Ollama host anyway?")
                        .default(true)
                        .interact()?
                {
                    keys.insert("OLLAMA_HOST".into(), ollama_host.clone());
                    ollama_selected = true;
                }
            }
        }
        OllamaStatus::NotAvailable => {
            println!("Ollama not detected at {}.", ollama_host);
            if non_interactive {
                println!("Skipping Ollama (not available).");
            } else {
                println!(
                    "Install from https://ollama.com or set OLLAMA_HOST if running elsewhere."
                );
                // Offer manual URL input
                if Confirm::new()
                    .with_prompt("Enter a custom Ollama URL?")
                    .default(false)
                    .interact()?
                {
                    let custom: String = dialoguer::Input::new()
                        .with_prompt("Ollama URL")
                        .default("http://localhost:11434".into())
                        .interact_text()?;
                    match detect_ollama(&custom).await {
                        OllamaStatus::Available(models) => {
                            println!(
                                "Ollama detected at {} with {} model(s).",
                                custom,
                                models.len()
                            );
                            if !models.is_empty() {
                                let mut choices: Vec<&str> =
                                    models.iter().map(|s| s.as_str()).collect();
                                choices.push("Skip Ollama");
                                let sel = Select::new()
                                    .with_prompt("Select Ollama model")
                                    .items(&choices)
                                    .default(0)
                                    .interact()?;
                                if sel < models.len() {
                                    keys.insert("OLLAMA_HOST".into(), custom);
                                    keys.insert("OLLAMA_MODEL".into(), models[sel].clone());
                                    ollama_selected = true;
                                } else {
                                    println!("Skipping Ollama.");
                                }
                            } else {
                                keys.insert("OLLAMA_HOST".into(), custom);
                                ollama_selected = true;
                            }
                        }
                        OllamaStatus::NotAvailable => {
                            println!("Could not connect to Ollama at {}.", custom);
                            if Confirm::new()
                                .with_prompt("Save this URL anyway?")
                                .default(false)
                                .interact()?
                            {
                                keys.insert("OLLAMA_HOST".into(), custom);
                                ollama_selected = true;
                            }
                        }
                    }
                }
            }
        }
    }

    // Step 5: Web Search (Brave) — optional, last
    if !non_interactive {
        println!("\n--- Web Search (optional) ---");
        let web_choices = &["Brave Search", "Skip"];
        let web_sel = Select::new()
            .with_prompt("Choose web search provider")
            .items(web_choices)
            .default(1)
            .interact()?;

        if web_sel == 0
            && let Some(key) = prompt_brave_key(existing.get("BRAVE_API_KEY"))?
        {
            keys.insert("BRAVE_API_KEY".into(), key);
        }
    }

    // Step 6: Summary + write
    println!("\n--- Summary ---");
    match setup_summary(&existing, &keys) {
        SetupSummary::NoProviders => {
            println!("No providers configured. You can run `mira setup` again later.");
            return Ok(());
        }
        SetupSummary::ExistingUnchanged => {
            println!("No new providers configured. Existing configuration unchanged.");
            return Ok(());
        }
        SetupSummary::NewKeysConfigured => {}
    }

    for (k, v) in &keys {
        let display = if k.contains("KEY") {
            mask_key(v)
        } else {
            v.clone()
        };
        println!("  {} = {}", k, display);
    }

    // Backup existing .env
    if env_path.exists() {
        let backup = mira_dir.join(".env.backup");
        std::fs::copy(&env_path, &backup)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&backup, std::fs::Permissions::from_mode(0o600))?;
        }
        println!("\nBacked up existing .env to .env.backup");
    }

    // Write .env (merge with existing, configured keys override)
    write_env(&env_path, &existing, &keys)?;

    // chmod 600 on .env
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    tracing::debug!(
        "Skipping .env file permission restriction on non-Unix platform: {}",
        env_path.display()
    );

    // Write config.toml if Ollama selected AND no DeepSeek key exists
    // (neither newly entered nor already in .env — DeepSeek takes priority)
    let has_deepseek = keys.contains_key("DEEPSEEK_API_KEY")
        || existing
            .get("DEEPSEEK_API_KEY")
            .is_some_and(|v| !v.is_empty());
    if ollama_selected && !has_deepseek {
        write_config_toml(&config_path)?;
        println!(
            "Updated {} with background_provider = \"ollama\"",
            config_path.display()
        );
    }

    println!(
        "\nSetup complete! Configuration saved to {}",
        env_path.display()
    );
    println!("Restart Claude Code for changes to take effect.");

    Ok(())
}

/// --check mode: read-only validation
async fn run_check() -> Result<()> {
    println!("Mira Configuration Status\n");

    let mira_dir = mira_dir()?;
    let env_path = mira_dir.join(".env");
    let config_path = mira_dir.join("config.toml");

    // Config directory
    if mira_dir.exists() {
        println!("  Config directory: {} (exists)", mira_dir.display());
    } else {
        println!("  Config directory: {} (MISSING)", mira_dir.display());
    }

    // .env file
    if env_path.exists() {
        println!("  .env file: {} (exists)", env_path.display());
    } else {
        println!("  .env file: {} (MISSING)", env_path.display());
    }

    // config.toml
    if config_path.exists() {
        println!("  config.toml: {} (exists)", config_path.display());
    } else {
        println!(
            "  config.toml: {} (not found, using defaults)",
            config_path.display()
        );
    }

    // MIRA_DISABLE_LLM
    if let Ok(val) = std::env::var("MIRA_DISABLE_LLM") {
        println!("  MIRA_DISABLE_LLM: {} (set)", val);
    }

    // Load and validate config
    let config = mira::config::env::EnvConfig::load();
    let validation = config.validate();

    println!("\n  Providers:");
    println!("    {}", config.api_keys.summary());

    if let Some(ref provider) = config.default_provider {
        println!("  Default provider: {}", provider);
    }

    // config.toml settings
    let file_config = mira::config::file::MiraConfig::load();
    if let Some(bp) = file_config.background_provider() {
        println!("  Background provider (config.toml): {}", bp);
    }

    // Validation
    if !validation.warnings.is_empty() || !validation.errors.is_empty() {
        println!("\n{}", validation.report());
    } else {
        println!("\n  Configuration OK");
    }

    // Features enabled summary
    println!("\n  Enabled features:");

    if config.api_keys.openai.is_some() {
        println!("    \u{2713} Memory & search: semantic (OpenAI)");
    } else {
        println!("    - Memory & search: keyword-only (add OPENAI_API_KEY for semantic)");
    }

    if config.api_keys.deepseek.is_some() {
        println!("    \u{2713} Background intelligence: DeepSeek");
    } else if config.api_keys.ollama.is_some() {
        println!("    \u{2713} Background intelligence: Ollama");
    } else {
        println!(
            "    - Background intelligence: disabled (add DEEPSEEK_API_KEY or configure Ollama)"
        );
    }

    println!("    \u{2713} Goal tracking: ready");
    println!("    \u{2713} Code indexing: ready");

    if config.api_keys.brave.is_some() {
        println!("    \u{2713} Web search: Brave");
    } else {
        println!("    - Web search: not configured (optional)");
    }

    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

fn mira_dir() -> Result<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    Ok(home.join(".mira"))
}

fn ensure_dir(path: &PathBuf) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
        println!("Created config directory: {}", path.display());
    }
    Ok(())
}

/// Read existing .env file into a key-value map.
/// Strips surrounding quotes from values to match dotenvy runtime behavior.
fn read_existing_env(path: &PathBuf) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(contents) = std::fs::read_to_string(path) {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                let val = val.trim();
                // Strip surrounding quotes to match dotenvy behavior
                let val = if val.len() >= 2
                    && ((val.starts_with('"') && val.ends_with('"'))
                        || (val.starts_with('\'') && val.ends_with('\'')))
                {
                    &val[1..val.len() - 1]
                } else {
                    val
                };
                map.insert(key.trim().to_string(), val.to_string());
            }
        }
    }
    map
}

/// Prompt for an API key with validation
async fn prompt_api_key(
    provider_name: &str,
    env_var: &str,
    existing: Option<&String>,
) -> Result<Option<String>> {
    if let Some(existing_key) = existing {
        println!(
            "  Existing {} key found: {}",
            provider_name,
            mask_key(existing_key)
        );
        if !Confirm::new()
            .with_prompt("Replace existing key?")
            .default(false)
            .interact()?
        {
            return Ok(Some(existing_key.clone()));
        }
    }

    let key = Password::new()
        .with_prompt(format!("Enter {} API key", provider_name))
        .interact()?;

    if key.trim().is_empty() {
        println!("  Empty key, skipping {}.", provider_name);
        return Ok(None);
    }

    // Validate with a test API call
    println!("  Validating {}...", provider_name);
    match validate_api_key(env_var, &key).await {
        ValidationResult::Ok => {
            println!("  {} key validated successfully.", provider_name);
            Ok(Some(key))
        }
        ValidationResult::Failed(err) => {
            println!("  Validation failed: {}", err);
            if Confirm::new()
                .with_prompt("Save key anyway?")
                .default(false)
                .interact()?
            {
                Ok(Some(key))
            } else {
                Ok(None)
            }
        }
    }
}

/// Prompt for Brave API key (format check only, no API validation)
fn prompt_brave_key(existing: Option<&String>) -> Result<Option<String>> {
    if let Some(existing_key) = existing {
        println!("  Existing Brave key found: {}", mask_key(existing_key));
        if !Confirm::new()
            .with_prompt("Replace existing key?")
            .default(false)
            .interact()?
        {
            return Ok(Some(existing_key.clone()));
        }
    }

    let key = Password::new()
        .with_prompt("Enter Brave Search API key")
        .interact()?;

    if key.trim().is_empty() {
        println!("  Empty key, skipping Brave Search.");
        return Ok(None);
    }

    // Basic format check
    if key.len() < 10 {
        println!("  Warning: Key seems too short. Brave keys are typically 30+ characters.");
    }

    Ok(Some(key))
}

enum ValidationResult {
    Ok,
    Failed(String),
}

/// Validate an API key by making a test call
async fn validate_api_key(env_var: &str, key: &str) -> ValidationResult {
    let client = reqwest::Client::new();
    match env_var {
        "DEEPSEEK_API_KEY" => {
            let resp = client
                .post("https://api.deepseek.com/chat/completions")
                .header("Authorization", format!("Bearer {}", key))
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({
                    "model": "deepseek-chat",
                    "messages": [{"role": "user", "content": "hi"}],
                    "max_tokens": 1
                }))
                .timeout(Duration::from_secs(10))
                .send()
                .await;
            check_response(resp).await
        }
        "OPENAI_API_KEY" => {
            let resp = client
                .post("https://api.openai.com/v1/embeddings")
                .header("Authorization", format!("Bearer {}", key))
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({
                    "model": "text-embedding-3-small",
                    "input": "test",
                    "dimensions": 256
                }))
                .timeout(Duration::from_secs(15))
                .send()
                .await;
            check_response(resp).await
        }
        _ => ValidationResult::Ok,
    }
}

async fn check_response(resp: Result<reqwest::Response, reqwest::Error>) -> ValidationResult {
    match resp {
        Ok(r) => {
            let status = r.status();
            if status.is_success() {
                ValidationResult::Ok
            } else if status.as_u16() == 401 {
                ValidationResult::Failed("Invalid API key (401 Unauthorized)".into())
            } else if status.as_u16() == 429 {
                // Rate limit is actually fine — key is valid
                ValidationResult::Ok
            } else {
                let body = r.text().await.unwrap_or_default();
                ValidationResult::Failed(format!("HTTP {} — {}", status, truncate(&body, 200)))
            }
        }
        Err(e) => {
            if e.is_timeout() {
                ValidationResult::Failed("Connection timed out".into())
            } else if e.is_connect() {
                ValidationResult::Failed("Could not connect to API server".into())
            } else {
                ValidationResult::Failed(format!("Network error: {}", e))
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> &str {
    mira::utils::truncate_at_boundary(s, max)
}

/// Mask an API key for display. Shows first 4 and last 4 chars only if the key
/// is long enough (>12 chars) to actually hide something meaningful.
fn mask_key(key: &str) -> String {
    if key.len() > 12 {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    } else {
        // Key too short to meaningfully mask with prefix/suffix — hide it entirely
        format!("****({} chars)", key.len())
    }
}

enum OllamaStatus {
    Available(Vec<String>),
    NotAvailable,
}

/// Try to detect Ollama and list available models
async fn detect_ollama(host: &str) -> OllamaStatus {
    let client = reqwest::Client::new();
    let base = host.trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);
    let url = format!("{}/api/tags", base);
    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            if let Ok(body) = r.json::<serde_json::Value>().await {
                let models: Vec<String> = body
                    .get("models")
                    .and_then(|m| m.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                OllamaStatus::Available(models)
            } else {
                OllamaStatus::Available(vec![])
            }
        }
        _ => OllamaStatus::NotAvailable,
    }
}

/// Write the .env file, merging existing keys with new ones (new overrides)
fn write_env(
    path: &PathBuf,
    existing: &BTreeMap<String, String>,
    new_keys: &BTreeMap<String, String>,
) -> Result<()> {
    let mut merged = existing.clone();
    for (k, v) in new_keys {
        merged.insert(k.clone(), v.clone());
    }

    let mut file = std::fs::File::create(path)?;
    writeln!(file, "# Mira Environment Configuration")?;
    writeln!(file, "# Generated by `mira setup`")?;
    writeln!(file)?;

    for (k, v) in &merged {
        writeln!(file, "{}={}", k, v)?;
    }

    Ok(())
}

/// Write or update config.toml with background_provider = "ollama"
fn write_config_toml(path: &PathBuf) -> Result<()> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();

    // Check for an UNCOMMENTED background_provider line
    let has_active_setting = existing.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.starts_with('#') && trimmed.starts_with("background_provider")
    });

    if has_active_setting {
        // Update existing uncommented value, preserving indentation
        let mut lines: Vec<String> = existing.lines().map(|l| l.to_string()).collect();
        for line in &mut lines {
            if !line.trim().starts_with('#') && line.trim().starts_with("background_provider") {
                let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                *line = format!("{}background_provider = \"ollama\"", indent);
            }
        }
        std::fs::write(path, lines.join("\n") + "\n")?;
    } else {
        // Check if [llm] section exists as an actual section header (not in a comment).
        // Handles "[llm]", "[llm] # comment", etc.
        let is_llm_header = |line: &str| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                return false;
            }
            // Strip inline comment: "[llm] # comment" -> "[llm]"
            let without_comment = trimmed.split('#').next().unwrap_or(trimmed).trim();
            without_comment == "[llm]"
        };

        let has_llm_section = existing.lines().any(is_llm_header);

        if has_llm_section {
            // Insert background_provider right after the [llm] header
            let mut lines: Vec<String> = existing.lines().map(|l| l.to_string()).collect();
            let mut insert_idx = None;
            for (i, line) in lines.iter().enumerate() {
                if is_llm_header(line) {
                    insert_idx = Some(i + 1);
                    break;
                }
            }
            if let Some(idx) = insert_idx {
                lines.insert(idx, "background_provider = \"ollama\"".to_string());
            }
            std::fs::write(path, lines.join("\n") + "\n")?;
        } else {
            // Append new [llm] section with the setting
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            writeln!(file, "\n[llm]")?;
            writeln!(file, "background_provider = \"ollama\"")?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn keys(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn no_existing_no_new_is_no_providers() {
        let existing = BTreeMap::new();
        let new_keys = BTreeMap::new();
        assert_eq!(
            setup_summary(&existing, &new_keys),
            SetupSummary::NoProviders
        );
    }

    #[test]
    fn non_provider_env_only_is_no_providers() {
        let existing = keys(&[("MIRA_USER_ID", "abc")]);
        let new_keys = BTreeMap::new();
        assert_eq!(
            setup_summary(&existing, &new_keys),
            SetupSummary::NoProviders
        );
    }

    #[test]
    fn existing_provider_no_new_is_unchanged() {
        let existing = keys(&[("DEEPSEEK_API_KEY", "sk-test")]);
        let new_keys = BTreeMap::new();
        assert_eq!(
            setup_summary(&existing, &new_keys),
            SetupSummary::ExistingUnchanged
        );
    }

    #[test]
    fn existing_ollama_no_new_is_unchanged() {
        let existing = keys(&[("OLLAMA_HOST", "http://localhost:11434")]);
        let new_keys = BTreeMap::new();
        assert_eq!(
            setup_summary(&existing, &new_keys),
            SetupSummary::ExistingUnchanged
        );
    }

    #[test]
    fn existing_provider_plus_non_provider_no_new_is_unchanged() {
        let existing = keys(&[("OPENAI_API_KEY", "sk-test"), ("MIRA_USER_ID", "abc")]);
        let new_keys = BTreeMap::new();
        assert_eq!(
            setup_summary(&existing, &new_keys),
            SetupSummary::ExistingUnchanged
        );
    }

    #[test]
    fn new_keys_with_no_existing_is_configured() {
        let existing = BTreeMap::new();
        let new_keys = keys(&[("DEEPSEEK_API_KEY", "sk-new")]);
        assert_eq!(
            setup_summary(&existing, &new_keys),
            SetupSummary::NewKeysConfigured
        );
    }

    #[test]
    fn new_keys_with_existing_is_configured() {
        let existing = keys(&[("OPENAI_API_KEY", "sk-old")]);
        let new_keys = keys(&[("DEEPSEEK_API_KEY", "sk-new")]);
        assert_eq!(
            setup_summary(&existing, &new_keys),
            SetupSummary::NewKeysConfigured
        );
    }

    #[test]
    fn mask_key_hides_short_keys_entirely() {
        // Keys <= 12 chars should be fully hidden
        assert_eq!(mask_key("x"), "****(1 chars)");
        assert_eq!(mask_key("abcd"), "****(4 chars)");
        assert_eq!(mask_key("12345678"), "****(8 chars)");
        assert_eq!(mask_key("123456789012"), "****(12 chars)");
    }

    #[test]
    fn mask_key_shows_prefix_suffix_for_long_keys() {
        // Keys > 12 chars show first 4 and last 4
        assert_eq!(mask_key("sk-abcdefghijklm"), "sk-a...jklm");
        let long_key = "sk-proj-1234567890abcdefghij";
        let masked = mask_key(long_key);
        assert!(masked.starts_with("sk-p"));
        assert!(masked.ends_with("ghij"));
        assert!(masked.contains("..."));
    }

    #[test]
    fn read_env_strips_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        std::fs::write(
            &env_path,
            "DOUBLE=\"quoted-value\"\nSINGLE='single-quoted'\nBARE=bare-value\nEMPTY=\n",
        )
        .unwrap();
        let map = read_existing_env(&env_path.to_path_buf());
        assert_eq!(map.get("DOUBLE").unwrap(), "quoted-value");
        assert_eq!(map.get("SINGLE").unwrap(), "single-quoted");
        assert_eq!(map.get("BARE").unwrap(), "bare-value");
        assert_eq!(map.get("EMPTY").unwrap(), "");
    }

    #[test]
    fn read_env_single_char_quote_no_panic() {
        // A lone quote character should not panic (Codex finding #2)
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        std::fs::write(&env_path, "BAD_KEY=\"\nALSO_BAD='\nOK=value\n").unwrap();
        let map = read_existing_env(&env_path.to_path_buf());
        // Single quote chars are kept as-is (not stripped, since len < 2)
        assert_eq!(map.get("BAD_KEY").unwrap(), "\"");
        assert_eq!(map.get("ALSO_BAD").unwrap(), "'");
        assert_eq!(map.get("OK").unwrap(), "value");
    }
}
