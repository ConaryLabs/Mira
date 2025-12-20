//! Shell execution tool
//!
//! Thin wrapper delegating to core::ops::shell for shared implementation.

use anyhow::Result;
use serde_json::Value;
use std::path::Path;

use crate::core::ops::shell as core_shell;

/// Shell tool implementations
pub struct ShellTools<'a> {
    pub cwd: &'a Path,
}

impl<'a> ShellTools<'a> {
    pub async fn bash(&self, args: &Value) -> Result<String> {
        let command = args["command"].as_str().unwrap_or("");

        let input = core_shell::BashInput {
            command: command.to_string(),
            cwd: self.cwd.to_path_buf(),
            timeout: None,
        };

        match core_shell::bash(input.clone()).await {
            Ok(output) => Ok(core_shell::format_output(&input, &output)),
            Err(e) => Ok(format!("Error: {}", e)),
        }
    }
}
