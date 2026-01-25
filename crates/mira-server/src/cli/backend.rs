// crates/mira-server/src/cli/backend.rs
// Backend management commands

use super::clients::format_tokens;
use super::get_db_path;
use anyhow::Result;
use mira::db::pool::DatabasePool;
use mira::http::create_shared_client;

/// List configured backends
pub fn run_backend_list() -> Result<()> {
    use mira::proxy::ProxyConfig;

    let config = ProxyConfig::load()?;

    if config.backends.is_empty() {
        println!("No backends configured.");
        println!("\nCreate a config file at: {:?}", ProxyConfig::default_config_path()?);
        return Ok(());
    }

    println!("Configured backends:\n");

    let default = config.default_backend.as_deref();

    for (name, backend) in &config.backends {
        let has_key = backend.get_api_key().is_some();
        let status = if !backend.enabled {
            "disabled"
        } else if !has_key {
            "no API key"
        } else {
            "ready"
        };

        let is_default = default == Some(name.as_str());
        let marker = if is_default { " (default)" } else { "" };

        println!(
            "  {} [{}]{}",
            name,
            status,
            marker
        );
        println!("    URL: {}", backend.base_url);
        if let Some(env_var) = &backend.api_key_env {
            println!("    Key: ${}", env_var);
        }
        if !backend.env.is_empty() {
            println!("    Model: {}", backend.env.get("ANTHROPIC_MODEL").unwrap_or(&"-".to_string()));
        }
        println!();
    }

    Ok(())
}

/// Set the default backend
pub async fn run_backend_use(name: &str) -> Result<()> {
    use mira::proxy::ProxyConfig;

    let mut config = ProxyConfig::load()?;

    // Verify backend exists
    if !config.backends.contains_key(name) {
        eprintln!("Backend '{}' not found.", name);
        eprintln!("\nAvailable backends:");
        for backend_name in config.backends.keys() {
            eprintln!("  {}", backend_name);
        }
        return Ok(());
    }

    // Check if it's usable
    let backend = config.backends.get(name).unwrap();
    if !backend.enabled {
        eprintln!("Backend '{}' is disabled in config.", name);
        return Ok(());
    }
    if backend.get_api_key().is_none() {
        eprintln!("Warning: Backend '{}' has no API key configured.", name);
    }

    // Update config
    config.default_backend = Some(name.to_string());

    // Write back to config file
    let config_path = ProxyConfig::default_config_path()?;
    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, &toml_str)?;

    println!("Default backend set to '{}'", name);
    println!("Config updated: {:?}", config_path);

    // If proxy is running, notify user to restart
    let pid_path = super::proxy::get_proxy_pid_path();
    if pid_path.exists() {
        println!("\nNote: Restart the proxy for changes to take effect:");
        println!("  mira proxy stop && mira proxy start -d");
    }

    Ok(())
}

/// Test connectivity to a backend
pub async fn run_backend_test(name: &str) -> Result<()> {
    use mira::proxy::ProxyConfig;

    let config = ProxyConfig::load()?;

    // Get backend config
    let backend = match config.backends.get(name) {
        Some(b) => b,
        None => {
            eprintln!("Backend '{}' not found.", name);
            eprintln!("\nAvailable backends:");
            for backend_name in config.backends.keys() {
                eprintln!("  {}", backend_name);
            }
            return Ok(());
        }
    };

    // Check prerequisites
    if !backend.enabled {
        eprintln!("Backend '{}' is disabled.", name);
        return Ok(());
    }

    let api_key = match backend.get_api_key() {
        Some(k) => k,
        None => {
            eprintln!("Backend '{}' has no API key configured.", name);
            if let Some(env_var) = &backend.api_key_env {
                eprintln!("Set the {} environment variable.", env_var);
            }
            return Ok(());
        }
    };

    println!("Testing backend '{}'...", name);
    println!("  URL: {}", backend.base_url);

    // Send a minimal test request
    let client = create_shared_client();
    let test_url = format!("{}/v1/messages", backend.base_url);

    let test_body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "Hi"}]
    });

    let start = std::time::Instant::now();
    let response = client
        .post(&test_url)
        .header("content-type", "application/json")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&test_body)
        .send()
        .await;

    let elapsed = start.elapsed();

    match response {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                println!("\n Connection successful!");
                println!("  Status: {}", status);
                println!("  Latency: {:?}", elapsed);
            } else {
                let body = resp.text().await.unwrap_or_default();
                eprintln!("\n Request failed");
                eprintln!("  Status: {}", status);
                if !body.is_empty() {
                    // Try to extract error message
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) {
                        if let Some(msg) = json.get("error").and_then(|e| e.get("message")) {
                            eprintln!("  Error: {}", msg);
                        } else {
                            eprintln!("  Body: {}", body);
                        }
                    } else {
                        eprintln!("  Body: {}", body);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("\n Connection failed");
            eprintln!("  Error: {}", e);
        }
    }

    Ok(())
}

/// Print environment variables for a backend in shell export format
pub fn run_backend_env(name: Option<&str>) -> Result<()> {
    use mira::proxy::ProxyConfig;

    let config = ProxyConfig::load()?;

    // Get backend name (use default if not specified)
    let backend_name = match name {
        Some(n) => n.to_string(),
        None => match &config.default_backend {
            Some(d) => d.clone(),
            None => {
                eprintln!("No backend specified and no default set.");
                eprintln!("Usage: mira backend env <name>");
                return Ok(());
            }
        }
    };

    // Get backend config
    let backend = match config.backends.get(&backend_name) {
        Some(b) => b,
        None => {
            eprintln!("Backend '{}' not found.", backend_name);
            eprintln!("\nAvailable backends:");
            for name in config.backends.keys() {
                eprintln!("  {}", name);
            }
            return Ok(());
        }
    };

    // Print base URL and auth token
    println!("export ANTHROPIC_BASE_URL=\"{}\"", backend.base_url);

    // Print API key (from env var or inline)
    if let Some(env_var) = &backend.api_key_env {
        // Reference the env var
        println!("export ANTHROPIC_AUTH_TOKEN=\"${}\"", env_var);
    } else if let Some(key) = &backend.api_key {
        println!("export ANTHROPIC_AUTH_TOKEN=\"{}\"", key);
    }

    // Print all env overrides
    for (key, value) in &backend.env {
        println!("export {}=\"{}\"", key, value);
    }

    // Show activation message with model info
    let model = backend.env.get("ANTHROPIC_MODEL")
        .map(|s| s.as_str())
        .unwrap_or("default");
    eprintln!("# Activated: {} ({})", backend_name, model);

    Ok(())
}

/// Show usage statistics from the database
pub async fn run_backend_usage(backend: Option<&str>, days: u32) -> Result<()> {
    let db_path = get_db_path();
    let pool = DatabasePool::open(&db_path).await?;

    // Calculate date range
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    let cutoff_str = cutoff.format("%Y-%m-%d").to_string();
    let backend_owned = backend.map(|s| s.to_string());

    // Query usage from database
    let cutoff_clone = cutoff_str.clone();
    let usage_result = pool.interact(move |conn| {
        let sql = if let Some(ref backend_name) = backend_owned {
            format!(
                "SELECT backend_name, model,
                        SUM(input_tokens) as total_input,
                        SUM(output_tokens) as total_output,
                        SUM(cache_creation_tokens) as total_cache_create,
                        SUM(cache_read_tokens) as total_cache_read,
                        SUM(cost_estimate) as total_cost,
                        COUNT(*) as request_count
                 FROM proxy_usage
                 WHERE backend_name = '{}' AND created_at >= '{}'
                 GROUP BY backend_name, model
                 ORDER BY total_cost DESC",
                backend_name, cutoff_clone
            )
        } else {
            format!(
                "SELECT backend_name, model,
                        SUM(input_tokens) as total_input,
                        SUM(output_tokens) as total_output,
                        SUM(cache_creation_tokens) as total_cache_create,
                        SUM(cache_read_tokens) as total_cache_read,
                        SUM(cost_estimate) as total_cost,
                        COUNT(*) as request_count
                 FROM proxy_usage
                 WHERE created_at >= '{}'
                 GROUP BY backend_name, model
                 ORDER BY total_cost DESC",
                cutoff_clone
            )
        };

        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Ok::<_, anyhow::Error>(None),
        };

        let rows: Vec<(String, Option<String>, i64, i64, i64, i64, f64, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get::<_, f64>(6).unwrap_or(0.0),
                    row.get(7)?,
                ))
            })
            .map_err(|e| anyhow::anyhow!("{}", e))?
            .filter_map(Result::ok)
            .collect();

        Ok(Some(rows))
    }).await?;

    let Some(rows) = usage_result else {
        println!("No usage data available yet.");
        println!("\nUsage tracking starts when requests go through the proxy.");
        println!("Start the proxy with: mira proxy start -d");
        return Ok(());
    };

    if rows.is_empty() {
        println!("No usage data in the last {} days.", days);
        return Ok(());
    }

    println!("Usage Statistics (last {} days)\n", days);
    println!("{:<12} {:<25} {:>10} {:>10} {:>10} {:>8}",
        "Backend", "Model", "Input", "Output", "Requests", "Cost");
    println!("{}", "-".repeat(80));

    let mut total_cost = 0.0;
    let mut total_requests = 0i64;

    for (backend_name, model, input, output, _cache_create, _cache_read, cost, requests) in &rows {
        let model_str = model.as_deref().unwrap_or("-");
        let model_display = if model_str.len() > 24 {
            format!("{}...", &model_str[..21])
        } else {
            model_str.to_string()
        };

        println!("{:<12} {:<25} {:>10} {:>10} {:>10} ${:>7.4}",
            backend_name,
            model_display,
            format_tokens(*input),
            format_tokens(*output),
            requests,
            cost
        );

        total_cost += cost;
        total_requests += requests;
    }

    println!("{}", "-".repeat(80));
    println!("{:<12} {:<25} {:>10} {:>10} {:>10} ${:>7.4}",
        "TOTAL", "", "", "", total_requests, total_cost);

    // Also show embedding usage
    let embed_rows = pool.interact(move |conn| {
        let embed_sql = format!(
            "SELECT provider, model,
                    SUM(tokens) as total_tokens,
                    SUM(text_count) as total_texts,
                    SUM(cost_estimate) as total_cost,
                    COUNT(*) as request_count
             FROM embeddings_usage
             WHERE created_at >= '{}'
             GROUP BY provider, model
             ORDER BY total_cost DESC",
            cutoff_str
        );

        let mut embed_stmt = match conn.prepare(&embed_sql) {
            Ok(s) => s,
            Err(_) => return Ok::<_, anyhow::Error>(Vec::new()),
        };

        let rows: Vec<(String, String, i64, i64, f64, i64)> = embed_stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get::<_, f64>(4).unwrap_or(0.0),
                    row.get(5)?,
                ))
            })
            .map_err(|e| anyhow::anyhow!("{}", e))?
            .filter_map(Result::ok)
            .collect();

        Ok(rows)
    }).await?;

    if !embed_rows.is_empty() {
        println!("\n\nEmbedding Usage\n");
        println!("{:<12} {:<25} {:>12} {:>10} {:>10} {:>8}",
            "Provider", "Model", "Tokens", "Texts", "Requests", "Cost");
        println!("{}", "-".repeat(80));

        let mut embed_total_cost = 0.0;
        let mut embed_total_requests = 0i64;

        for (provider, model, tokens, texts, cost, requests) in &embed_rows {
            let model_display = if model.len() > 24 {
                format!("{}...", &model[..21])
            } else {
                model.clone()
            };

            println!("{:<12} {:<25} {:>12} {:>10} {:>10} ${:>7.4}",
                provider,
                model_display,
                format_tokens(*tokens),
                texts,
                requests,
                cost
            );

            embed_total_cost += cost;
            embed_total_requests += requests;
        }

        println!("{}", "-".repeat(80));
        println!("{:<12} {:<25} {:>12} {:>10} {:>10} ${:>7.4}",
            "TOTAL", "", "", "", embed_total_requests, embed_total_cost);
    }

    Ok(())
}
