//! crates/mira-server/src/tools/core/cross_project.rs
//! Cross-project intelligence sharing tools

use crate::cross_project::{
    self, CrossProjectConfig, SharingPreferences,
};
use crate::tools::core::ToolContext;

/// Unified cross-project tool with actions: get_preferences, enable_sharing, disable_sharing,
/// reset_budget, get_stats, extract_patterns, sync
pub async fn cross_project<C: ToolContext>(
    ctx: &C,
    action: String,
    export: Option<bool>,
    import: Option<bool>,
    min_confidence: Option<f64>,
    epsilon: Option<f64>,
) -> Result<String, String> {
    let project_id = ctx
        .project_id()
        .await
        .ok_or("No project set. Call session_start first.")?;

    match action.as_str() {
        "get_preferences" | "status" => {
            let prefs = ctx
                .pool()
                .interact(move |conn| {
                    cross_project::get_preferences(conn, project_id)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            let stats = ctx
                .pool()
                .interact(move |conn| {
                    cross_project::get_sharing_stats(conn, project_id)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            Ok(format_preferences(&prefs, &stats))
        }

        "enable_sharing" => {
            let export_val = export.unwrap_or(true);
            let import_val = import.unwrap_or(true);

            ctx.pool()
                .interact(move |conn| {
                    cross_project::enable_sharing(conn, project_id, export_val, import_val)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            Ok(format!(
                "Enabled cross-project sharing for this project.\n  Export patterns: {}\n  Import patterns: {}",
                export_val, import_val
            ))
        }

        "disable_sharing" => {
            ctx.pool()
                .interact(move |conn| {
                    cross_project::disable_sharing(conn, project_id)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            Ok("Disabled cross-project sharing for this project.".to_string())
        }

        "reset_budget" => {
            ctx.pool()
                .interact(move |conn| {
                    cross_project::reset_privacy_budget(conn, project_id)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            Ok("Privacy budget reset to 0. Full budget is now available.".to_string())
        }

        "get_stats" => {
            let stats = ctx
                .pool()
                .interact(move |conn| {
                    cross_project::get_sharing_stats(conn, project_id)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            Ok(format!(
                "Sharing statistics:\n  Exports: {}\n  Imports: {}\n  Privacy budget spent: {:.2}",
                stats.exports, stats.imports, stats.epsilon_spent
            ))
        }

        "extract_patterns" => {
            let config = CrossProjectConfig {
                min_confidence: min_confidence.unwrap_or(0.6),
                epsilon: epsilon.unwrap_or(1.0),
                ..Default::default()
            };

            let count = ctx
                .pool()
                .interact(move |conn| {
                    cross_project::extract_and_store_patterns(conn, project_id, &config)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            if count == 0 {
                Ok("No patterns extracted. Either sharing is disabled, privacy budget exhausted, or no qualifying patterns found.".to_string())
            } else {
                Ok(format!("Extracted and stored {} patterns for cross-project sharing.", count))
            }
        }

        "sync" => {
            // First extract patterns
            let config = CrossProjectConfig {
                min_confidence: min_confidence.unwrap_or(0.6),
                epsilon: epsilon.unwrap_or(1.0),
                ..Default::default()
            };

            let exported = ctx
                .pool()
                .interact(move |conn| {
                    cross_project::extract_and_store_patterns(conn, project_id, &config)
                        .map_err(|e| anyhow::anyhow!("{}", e))
                })
                .await
                .map_err(|e| e.to_string())?;

            // Then get shareable patterns from network
            let min_conf = min_confidence.unwrap_or(0.6);
            let imported = ctx
                .pool()
                .interact(move |conn| {
                    let prefs = cross_project::get_preferences(conn, project_id)?;
                    if !prefs.sharing_enabled || !prefs.import_patterns {
                        return Ok(0usize);
                    }

                    let patterns = cross_project::get_shareable_patterns(
                        conn, None, None, min_conf, 50
                    )?;
                    let mut count = 0;
                    for pattern in &patterns {
                        if cross_project::import_pattern(conn, project_id, pattern).is_ok() {
                            count += 1;
                        }
                    }
                    Ok(count)
                })
                .await
                .map_err(|e: anyhow::Error| e.to_string())?;

            Ok(format!(
                "Sync complete.\n  Exported: {} patterns\n  Imported: {} patterns",
                exported, imported
            ))
        }

        _ => Err(format!(
            "Unknown action: {}. Valid actions: get_preferences, status, enable_sharing, disable_sharing, reset_budget, get_stats, extract_patterns, sync",
            action
        )),
    }
}

fn format_preferences(prefs: &SharingPreferences, stats: &cross_project::SharingStats) -> String {
    let status = if prefs.sharing_enabled {
        "ENABLED"
    } else {
        "DISABLED"
    };

    let mut response = format!("Cross-project sharing: {}\n", status);
    response.push_str(&format!("  Export patterns: {}\n", prefs.export_patterns));
    response.push_str(&format!("  Import patterns: {}\n", prefs.import_patterns));
    response.push_str(&format!(
        "  Min anonymization: {:?}\n",
        prefs.min_anonymization_level
    ));
    response.push_str(&format!(
        "  Privacy budget: {:.2} / {:.2} ({:.0}% remaining)\n",
        prefs.privacy_epsilon_used,
        prefs.privacy_epsilon_budget,
        (prefs.remaining_privacy_budget() / prefs.privacy_epsilon_budget) * 100.0
    ));
    response.push_str(&format!("\nActivity:\n"));
    response.push_str(&format!("  Patterns exported: {}\n", stats.exports));
    response.push_str(&format!("  Patterns imported: {}\n", stats.imports));

    response
}
