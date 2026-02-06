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
    "ZHIPU_API_KEY",
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
        // Step 2: Expert provider
        println!("\n--- Expert Provider (reasoning, code review) ---");
        let expert_choices = &["DeepSeek (recommended)", "Zhipu GLM-4.7", "Skip"];
        let expert_sel = Select::new()
            .with_prompt("Choose expert provider")
            .items(expert_choices)
            .default(0)
            .interact()?;

        match expert_sel {
            0 => {
                if let Some(key) = prompt_api_key(
                    "DeepSeek",
                    "DEEPSEEK_API_KEY",
                    existing.get("DEEPSEEK_API_KEY"),
                )
                .await?
                {
                    keys.insert("DEEPSEEK_API_KEY".into(), key);
                }
            }
            1 => {
                if let Some(key) =
                    prompt_api_key("Zhipu", "ZHIPU_API_KEY", existing.get("ZHIPU_API_KEY")).await?
                {
                    keys.insert("ZHIPU_API_KEY".into(), key);
                }
            }
            _ => println!("Skipping expert provider."),
        }

        // Step 3: Embeddings
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

        // Step 4: Web search
        println!("\n--- Web Search (expert consultations) ---");
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

    // Step 5: Local LLM (Ollama)
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
            format!(
                "{}...{}",
                &v[..4.min(v.len())],
                &v[v.len().saturating_sub(4)..]
            )
        } else {
            v.clone()
        };
        println!("  {} = {}", k, display);
    }

    // Backup existing .env
    if env_path.exists() {
        let backup = mira_dir.join(".env.backup");
        std::fs::copy(&env_path, &backup)?;
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

    // Write config.toml if Ollama selected
    if ollama_selected {
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
    if let Some(ep) = file_config.expert_provider() {
        println!("  Expert provider (config.toml): {}", ep);
    }
    if let Some(bp) = file_config.background_provider() {
        println!("  Background provider (config.toml): {}", bp);
    }

    // Project-level .env
    if std::path::Path::new(".env").exists() {
        println!("\n  Project .env: .env (exists, overrides global)");
    }

    // Validation
    if !validation.warnings.is_empty() || !validation.errors.is_empty() {
        println!("\n{}", validation.report());
    } else {
        println!("\n  Configuration OK");
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

/// Read existing .env file into a key-value map
fn read_existing_env(path: &PathBuf) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(contents) = std::fs::read_to_string(path) {
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, val)) = line.split_once('=') {
                map.insert(key.trim().to_string(), val.trim().to_string());
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
        let masked = format!(
            "{}...{}",
            &existing_key[..4.min(existing_key.len())],
            &existing_key[existing_key.len().saturating_sub(4)..]
        );
        println!("  Existing {} key found: {}", provider_name, masked);
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
        let masked = format!(
            "{}...{}",
            &existing_key[..4.min(existing_key.len())],
            &existing_key[existing_key.len().saturating_sub(4)..]
        );
        println!("  Existing Brave key found: {}", masked);
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
        "ZHIPU_API_KEY" => {
            let resp = client
                .post("https://api.z.ai/api/coding/paas/v4/chat/completions")
                .header("Authorization", format!("Bearer {}", key))
                .header("Content-Type", "application/json")
                .json(&serde_json::json!({
                    "model": "GLM-4.7",
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
    if s.len() <= max {
        return s;
    }
    // Find the last char boundary at or before `max`
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
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
        // Update existing uncommented value
        let mut lines: Vec<String> = existing.lines().map(|l| l.to_string()).collect();
        for line in &mut lines {
            if !line.trim().starts_with('#') && line.trim().starts_with("background_provider") {
                *line = "background_provider = \"ollama\"".to_string();
            }
        }
        std::fs::write(path, lines.join("\n") + "\n")?;
    } else {
        // Append — create [llm] section if needed
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        if !existing.contains("[llm]") {
            writeln!(file, "\n[llm]")?;
        }
        writeln!(file, "background_provider = \"ollama\"")?;
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
}
