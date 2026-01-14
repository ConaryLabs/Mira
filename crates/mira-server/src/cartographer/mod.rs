// crates/mira-server/src/cartographer/mod.rs
// Codebase mapping and structure analysis

mod detection;
mod map;
mod summaries;
mod types;

use std::collections::HashMap;

// Re-export public API
pub use detection::{
    detect_modules, detect_rust_modules, find_entry_points as detect_entry_points,
    is_workspace, parse_crate_name,
};
pub use map::{get_modules_with_purposes, get_or_generate_map};
pub use summaries::{
    build_summary_prompt, get_module_code_preview, get_module_full_code,
    get_modules_needing_summaries, parse_summary_response, update_module_purposes,
};
pub use types::{CodebaseMap, Module, ModuleSummaryContext};

/// Format codebase map in compact text format
pub fn format_compact(map: &CodebaseMap) -> String {
    let mut output = String::new();

    // Group modules by top-level (crate name)
    let mut grouped: HashMap<String, Vec<&Module>> = HashMap::new();
    for module in &map.modules {
        let top = module.id.split('/').next().unwrap_or(&module.id);
        grouped.entry(top.to_string()).or_default().push(module);
    }

    for (crate_name, modules) in grouped.iter() {
        output.push_str(&format!("\n{}:\n", crate_name));

        for module in modules {
            // Skip if this is just the crate root
            if module.id == *crate_name {
                continue;
            }

            let purpose = module.purpose.as_deref().unwrap_or("");
            let deps = if module.depends_on.is_empty() {
                String::new()
            } else {
                let dep_names: Vec<_> = module
                    .depends_on
                    .iter()
                    .map(|d| d.split('/').next_back().unwrap_or(d))
                    .take(3)
                    .collect();
                format!(" -> {}", dep_names.join(", "))
            };

            output.push_str(&format!("  {} - {}{}\n", module.name, purpose, deps));
        }
    }

    if !map.entry_points.is_empty() {
        output.push_str(&format!("\nEntry: {}\n", map.entry_points.join(", ")));
    }

    output
}
