// crates/mira-server/src/hooks/mod.rs
// Claude Code hook handlers

pub mod permission;
pub mod post_tool;
pub mod pre_tool;
pub mod precompact;
pub mod session;
pub mod stop;
pub mod subagent;
pub mod user_prompt;

use anyhow::Result;
use std::time::Instant;

/// Performance threshold in milliseconds - warn if hook exceeds this
const HOOK_PERF_THRESHOLD_MS: u128 = 100;

/// Read hook input from stdin (Claude Code passes JSON)
pub fn read_hook_input() -> Result<serde_json::Value> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
    let json: serde_json::Value = serde_json::from_str(&input)?;
    Ok(json)
}

/// Write hook output to stdout
pub fn write_hook_output(output: &serde_json::Value) {
    println!(
        "{}",
        serde_json::to_string(output).expect("hook output must be serializable")
    );
}

/// Timer guard for hook performance monitoring
/// Logs execution time to stderr on drop
pub struct HookTimer {
    hook_name: &'static str,
    start: Instant,
}

impl HookTimer {
    /// Start timing a hook
    pub fn start(hook_name: &'static str) -> Self {
        Self {
            hook_name,
            start: Instant::now(),
        }
    }
}

impl Drop for HookTimer {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_millis();
        if elapsed > HOOK_PERF_THRESHOLD_MS {
            eprintln!(
                "[mira] PERF WARNING: {} hook took {}ms (threshold: {}ms)",
                self.hook_name, elapsed, HOOK_PERF_THRESHOLD_MS
            );
        } else {
            eprintln!("[mira] {} hook completed in {}ms", self.hook_name, elapsed);
        }
    }
}
