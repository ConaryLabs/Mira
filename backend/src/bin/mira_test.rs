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
use mira_backend::testing::dashboard::app::run_dashboard;
use mira_backend::testing::reporters::{OutputFormat, get_reporter};

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

        /// Output format: console, json, junit
        #[arg(long, short, default_value = "console")]
        output: String,

        /// Run scenarios in parallel
        #[arg(long)]
        parallel: bool,

        /// Maximum concurrent scenarios (default: 4, 0 = unlimited)
        #[arg(long, default_value = "4")]
        max_parallel: usize,
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

    /// Launch observability dashboard
    Dashboard {
        /// Backend WebSocket URL
        #[arg(long, default_value = "ws://localhost:3001/ws")]
        backend_url: String,
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
            output,
            parallel,
            max_parallel,
        } => {
            run_scenarios(
                path,
                backend_url,
                timeout,
                mock,
                fail_fast,
                tags,
                name,
                output,
                parallel,
                max_parallel,
                cli.verbose,
            )
            .await
        }
        Commands::List { path, tags } => list_scenarios(path, tags),
        Commands::Validate { path } => validate_scenarios(path),
        Commands::Dashboard { backend_url } => {
            info!("Launching dashboard...");
            run_dashboard(&backend_url).await
        }
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
    output_format: String,
    parallel: bool,
    max_parallel: usize,
    verbose: bool,
) -> Result<()> {
    // Parse output format
    let format = OutputFormat::from_str(&output_format).unwrap_or_else(|| {
        eprintln!("Warning: Unknown output format '{}', using console", output_format);
        OutputFormat::Console
    });

    // Only show info logs for console format
    if format == OutputFormat::Console {
        info!("Mira Test Runner");
        info!("================");
    }

    // Load scenarios
    let mut scenarios = if path.is_dir() {
        ScenarioParser::parse_directory(&path)?
    } else {
        vec![ScenarioParser::parse_file(&path)?]
    };

    if scenarios.is_empty() {
        if format == OutputFormat::Console {
            println!("No scenarios found at {}", path.display());
        }
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
        if format == OutputFormat::Console {
            println!("No scenarios match the specified filters");
        }
        return Ok(());
    }

    if format == OutputFormat::Console {
        info!("Found {} scenario(s)", scenarios.len());
    }

    // Configure runner
    let config = RunnerConfig {
        backend_url,
        default_timeout: Duration::from_secs(timeout),
        mock_mode: mock,
        fail_fast,
        verbose,
        parallel,
        max_parallel,
    };

    let runner = ScenarioRunner::new(config);

    // Run scenarios
    let results = runner.run_scenarios(&scenarios).await;

    // Generate and print report using the selected reporter
    let reporter = get_reporter(format);
    let report = reporter.report(&results, verbose);
    println!("{}", report);

    // Exit with appropriate code
    let summary = RunSummary::from_results(&results);
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
