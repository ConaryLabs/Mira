// src/bin/mira_test.rs
// CLI entry point for Mira testing framework

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use mira_backend::testing::harness::runner::{RunSummary, RunnerConfig, ScenarioRunner};
use mira_backend::testing::scenarios::parser::ScenarioParser;

#[derive(Parser)]
#[command(name = "mira-test")]
#[command(about = "Mira Testing Framework - Automated testing and observability")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run test scenarios
    Run {
        /// Path to scenario file or directory
        path: PathBuf,

        /// Backend WebSocket URL
        #[arg(long, default_value = "ws://localhost:3001/ws")]
        backend_url: String,

        /// Default timeout in seconds
        #[arg(long, default_value = "60")]
        timeout: u64,

        /// Use mock LLM (when implemented)
        #[arg(long)]
        mock: bool,

        /// Stop on first failure
        #[arg(long)]
        fail_fast: bool,

        /// Filter by tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,

        /// Filter by name pattern
        #[arg(long)]
        name: Option<String>,
    },

    /// List available scenarios
    List {
        /// Path to scenario file or directory
        path: PathBuf,

        /// Filter by tags (comma-separated)
        #[arg(long)]
        tags: Option<String>,
    },

    /// Validate scenario files without running
    Validate {
        /// Path to scenario file or directory
        path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { Level::DEBUG } else { Level::INFO };
    FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_target(false)
        .with_thread_ids(false)
        .compact()
        .init();

    match cli.command {
        Commands::Run {
            path,
            backend_url,
            timeout,
            mock,
            fail_fast,
            tags,
            name,
        } => {
            run_scenarios(
                path,
                backend_url,
                timeout,
                mock,
                fail_fast,
                tags,
                name,
                cli.verbose,
            )
            .await
        }
        Commands::List { path, tags } => list_scenarios(path, tags),
        Commands::Validate { path } => validate_scenarios(path),
    }
}

async fn run_scenarios(
    path: PathBuf,
    backend_url: String,
    timeout: u64,
    mock: bool,
    fail_fast: bool,
    tags: Option<String>,
    name: Option<String>,
    verbose: bool,
) -> Result<()> {
    info!("Mira Test Runner");
    info!("================");

    // Load scenarios
    let mut scenarios = if path.is_dir() {
        ScenarioParser::parse_directory(&path)?
    } else {
        vec![ScenarioParser::parse_file(&path)?]
    };

    if scenarios.is_empty() {
        println!("No scenarios found at {}", path.display());
        return Ok(());
    }

    // Apply filters
    if let Some(ref tag_str) = tags {
        let tag_list: Vec<String> = tag_str.split(',').map(|s| s.trim().to_string()).collect();
        scenarios = ScenarioParser::filter_by_tags(scenarios, &tag_list);
    }

    if let Some(ref pattern) = name {
        scenarios = ScenarioParser::filter_by_name(scenarios, pattern);
    }

    if scenarios.is_empty() {
        println!("No scenarios match the specified filters");
        return Ok(());
    }

    info!("Found {} scenario(s)", scenarios.len());

    // Configure runner
    let config = RunnerConfig {
        backend_url,
        default_timeout: Duration::from_secs(timeout),
        mock_mode: mock,
        fail_fast,
        verbose,
    };

    let runner = ScenarioRunner::new(config);

    // Run scenarios
    let results = runner.run_scenarios(&scenarios).await;

    // Print results
    println!();
    println!("RESULTS");
    println!("-------");

    for result in &results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        println!(
            "[{}] {} ({}ms)",
            status, result.scenario_name, result.duration_ms
        );

        for step in &result.step_results {
            let step_status = if step.skipped {
                "SKIP"
            } else if step.passed {
                "PASS"
            } else {
                "FAIL"
            };

            println!("  [{}] {}", step_status, step.step_name);

            if !step.tool_executions.is_empty() && verbose {
                println!("    Tools: {}", step.tool_executions.join(", "));
            }

            for assertion in &step.assertion_results {
                if !assertion.passed || verbose {
                    let a_status = if assertion.passed { "PASS" } else { "FAIL" };
                    println!(
                        "    [{}] {}: {}",
                        a_status, assertion.assertion_type, assertion.message
                    );
                }
            }

            if let Some(ref error) = step.error {
                println!("    Error: {}", error);
            }
        }

        if let Some(ref error) = result.error {
            println!("  Error: {}", error);
        }
    }

    // Print summary
    let summary = RunSummary::from_results(&results);
    summary.print();

    // Exit with appropriate code
    if summary.failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn list_scenarios(path: PathBuf, tags: Option<String>) -> Result<()> {
    let mut scenarios = if path.is_dir() {
        ScenarioParser::parse_directory(&path)?
    } else {
        vec![ScenarioParser::parse_file(&path)?]
    };

    // Apply tag filter
    if let Some(ref tag_str) = tags {
        let tag_list: Vec<String> = tag_str.split(',').map(|s| s.trim().to_string()).collect();
        scenarios = ScenarioParser::filter_by_tags(scenarios, &tag_list);
    }

    println!("Available Scenarios");
    println!("===================");
    println!();

    for scenario in &scenarios {
        println!("Name: {}", scenario.name);
        if !scenario.description.is_empty() {
            println!("  Description: {}", scenario.description);
        }
        if !scenario.tags.is_empty() {
            println!("  Tags: {}", scenario.tags.join(", "));
        }
        println!("  Steps: {}", scenario.steps.len());
        println!("  Timeout: {}s", scenario.timeout_seconds);
        println!();
    }

    println!("Total: {} scenario(s)", scenarios.len());

    Ok(())
}

fn validate_scenarios(path: PathBuf) -> Result<()> {
    println!("Validating Scenarios");
    println!("====================");
    println!();

    let mut valid_count = 0;
    let mut invalid_count = 0;

    let paths: Vec<PathBuf> = if path.is_dir() {
        std::fs::read_dir(&path)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .map(|ext| ext == "yaml" || ext == "yml")
                    .unwrap_or(false)
            })
            .collect()
    } else {
        vec![path]
    };

    for file_path in paths {
        match ScenarioParser::parse_file(&file_path) {
            Ok(scenario) => {
                println!("[VALID] {} - {}", file_path.display(), scenario.name);
                valid_count += 1;
            }
            Err(e) => {
                println!("[INVALID] {} - {}", file_path.display(), e);
                invalid_count += 1;
            }
        }
    }

    println!();
    println!("Valid: {}, Invalid: {}", valid_count, invalid_count);

    if invalid_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}
