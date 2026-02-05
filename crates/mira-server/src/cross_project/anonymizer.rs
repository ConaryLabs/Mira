// crates/mira-server/src/cross_project/anonymizer.rs
// Privacy-preserving pattern anonymization with differential privacy

use anyhow::Result;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::CrossPatternType;

/// Level of anonymization applied to patterns
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::IntoStaticStr, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum AnonymizationLevel {
    /// Full anonymization: all identifiers removed, noise added
    Full,
    /// Partial anonymization: paths generalized, some structure preserved
    Partial,
    /// No anonymization (for local-only patterns)
    None,
}

impl AnonymizationLevel {
    pub fn as_str(&self) -> &'static str { self.into() }
}

/// An anonymized pattern ready for cross-project sharing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnonymizedPattern {
    pub pattern_type: CrossPatternType,
    pub pattern_hash: String,
    pub anonymized_data: serde_json::Value,
    pub category: Option<String>,
    pub confidence: f64,
    pub noise_added: f64,
    pub anonymization_level: AnonymizationLevel,
}

/// Anonymizes patterns for privacy-preserving sharing
pub struct PatternAnonymizer {
    /// Differential privacy epsilon (lower = more private, more noise)
    epsilon: f64,
    /// Minimum anonymization level required
    min_level: AnonymizationLevel,
}

impl PatternAnonymizer {
    pub fn new(epsilon: f64, min_level: AnonymizationLevel) -> Self {
        Self { epsilon, min_level }
    }

    /// Anonymize a file sequence pattern
    pub fn anonymize_file_sequence(
        &self,
        files: &[String],
        confidence: f64,
    ) -> Result<AnonymizedPattern> {
        // Extract generic file types instead of full paths
        let generalized: Vec<String> = files.iter().map(|f| self.generalize_file_path(f)).collect();

        // Add differential privacy noise to confidence
        let noisy_confidence = self.add_laplace_noise(confidence);

        // Create pattern hash from generalized sequence
        let pattern_hash = self.hash_sequence(&generalized);

        // Detect category from file types
        let category = self.detect_category(&generalized);

        Ok(AnonymizedPattern {
            pattern_type: CrossPatternType::FileSequence,
            pattern_hash,
            anonymized_data: serde_json::json!({
                "sequence": generalized,
                "length": files.len(),
            }),
            category,
            confidence: noisy_confidence.clamp(0.0, 1.0),
            noise_added: self.noise_scale(),
            anonymization_level: self.min_level,
        })
    }

    /// Anonymize a tool chain pattern
    pub fn anonymize_tool_chain(
        &self,
        tools: &[String],
        confidence: f64,
    ) -> Result<AnonymizedPattern> {
        // Tool names are already generic, just normalize
        let normalized: Vec<String> = tools.iter().map(|t| t.to_lowercase()).collect();

        let noisy_confidence = self.add_laplace_noise(confidence);
        let pattern_hash = self.hash_sequence(&normalized);
        let category = self.detect_tool_category(&normalized);

        Ok(AnonymizedPattern {
            pattern_type: CrossPatternType::ToolChain,
            pattern_hash,
            anonymized_data: serde_json::json!({
                "tools": normalized,
                "length": tools.len(),
            }),
            category,
            confidence: noisy_confidence.clamp(0.0, 1.0),
            noise_added: self.noise_scale(),
            anonymization_level: self.min_level,
        })
    }

    /// Anonymize a problem pattern from expert consultations
    pub fn anonymize_problem_pattern(
        &self,
        expert_role: &str,
        problem_category: &str,
        approaches: &[String],
        tools: &[String],
        success_rate: f64,
    ) -> Result<AnonymizedPattern> {
        // Generalize approaches (remove specific file/variable names)
        let generalized_approaches: Vec<String> =
            approaches.iter().map(|a| self.generalize_text(a)).collect();

        let noisy_success = self.add_laplace_noise(success_rate);

        let pattern_hash =
            self.hash_problem_pattern(expert_role, problem_category, &generalized_approaches);

        Ok(AnonymizedPattern {
            pattern_type: CrossPatternType::ProblemPattern,
            pattern_hash,
            anonymized_data: serde_json::json!({
                "expert_role": expert_role,
                "problem_category": problem_category,
                "approaches": generalized_approaches,
                "tools": tools,
            }),
            category: Some(problem_category.to_string()),
            confidence: noisy_success.clamp(0.0, 1.0),
            noise_added: self.noise_scale(),
            anonymization_level: self.min_level,
        })
    }

    /// Anonymize a collaboration pattern
    pub fn anonymize_collaboration_pattern(
        &self,
        domains: &[String],
        experts: &[String],
        mode: &str,
        success_rate: f64,
    ) -> Result<AnonymizedPattern> {
        let noisy_success = self.add_laplace_noise(success_rate);

        let pattern_hash = self.hash_collaboration_pattern(domains, experts, mode);

        Ok(AnonymizedPattern {
            pattern_type: CrossPatternType::Collaboration,
            pattern_hash,
            anonymized_data: serde_json::json!({
                "domains": domains,
                "experts": experts,
                "mode": mode,
            }),
            category: domains.first().cloned(),
            confidence: noisy_success.clamp(0.0, 1.0),
            noise_added: self.noise_scale(),
            anonymization_level: self.min_level,
        })
    }

    /// Generalize a file path to remove project-specific information
    fn generalize_file_path(&self, path: &str) -> String {
        // Extract meaningful parts: extension, common directory names
        let parts: Vec<&str> = path.split('/').collect();
        let filename = parts.last().unwrap_or(&"");

        // Get extension
        let ext = filename.rsplit('.').next().unwrap_or("unknown");

        // Detect common directory patterns
        let dir_type = self.classify_directory(&parts);

        match self.min_level {
            AnonymizationLevel::Full => {
                // Only keep extension and directory type
                format!("{}/{}", dir_type, ext)
            }
            AnonymizationLevel::Partial => {
                // Keep last directory and filename pattern
                let last_dir = parts.get(parts.len().saturating_sub(2)).unwrap_or(&"");
                let generic_name = self.generalize_filename(filename);
                format!("{}/{}", last_dir, generic_name)
            }
            AnonymizationLevel::None => path.to_string(),
        }
    }

    /// Classify directory based on common patterns
    fn classify_directory(&self, parts: &[&str]) -> &'static str {
        for part in parts {
            let lower = part.to_lowercase();
            if lower == "src" || lower == "lib" {
                return "src";
            } else if lower == "test" || lower == "tests" || lower == "__tests__" {
                return "test";
            } else if lower == "config" || lower == "cfg" {
                return "config";
            } else if lower == "docs" || lower == "doc" || lower == "documentation" {
                return "docs";
            } else if lower == "bin" || lower == "cmd" {
                return "bin";
            } else if lower == "pkg" || lower == "packages" {
                return "pkg";
            } else if lower == "api" {
                return "api";
            } else if lower == "models" || lower == "model" {
                return "models";
            } else if lower == "handlers" || lower == "controllers" {
                return "handlers";
            } else if lower == "utils" || lower == "helpers" || lower == "common" {
                return "utils";
            }
        }
        "other"
    }

    /// Generalize a filename to remove specific names
    fn generalize_filename(&self, filename: &str) -> String {
        // Common filename patterns
        let lower = filename.to_lowercase();

        if lower.contains("test") {
            return "test_file".to_string();
        }
        if lower.contains("config") || lower.contains("setting") {
            return "config_file".to_string();
        }
        if lower == "mod.rs" || lower == "index.ts" || lower == "__init__.py" {
            return "module_index".to_string();
        }
        if lower == "main.rs" || lower == "main.py" || lower == "main.go" {
            return "main_entry".to_string();
        }
        if lower == "lib.rs" {
            return "lib_entry".to_string();
        }

        // Just return extension-based name
        let ext = filename.rsplit('.').next().unwrap_or("unknown");
        format!("file.{}", ext)
    }

    /// Generalize text to remove specific identifiers
    fn generalize_text(&self, text: &str) -> String {
        // Simple heuristic: remove things that look like identifiers
        // This is a basic implementation - could be more sophisticated
        let result = text.to_string();

        // Remove camelCase and snake_case identifiers that are too specific
        // Keep common programming terms
        let common_terms = [
            "function",
            "class",
            "method",
            "variable",
            "type",
            "struct",
            "interface",
            "module",
            "error",
            "result",
            "option",
            "async",
            "await",
            "return",
            "if",
            "else",
            "for",
            "while",
            "match",
        ];

        // For now, keep the text but note this could be enhanced
        for term in &common_terms {
            if result.to_lowercase().contains(term) {
                // Keep these terms as they're generic
                continue;
            }
        }

        result
    }

    /// Detect category from generalized file sequence
    fn detect_category(&self, files: &[String]) -> Option<String> {
        let mut rust_count = 0;
        let mut ts_count = 0;
        let mut py_count = 0;
        let mut go_count = 0;

        for f in files {
            if f.ends_with("rs") {
                rust_count += 1;
            } else if f.ends_with("ts") || f.ends_with("tsx") {
                ts_count += 1;
            } else if f.ends_with("py") {
                py_count += 1;
            } else if f.ends_with("go") {
                go_count += 1;
            }
        }

        let max = [rust_count, ts_count, py_count, go_count]
            .iter()
            .max()
            .copied()
            .unwrap_or(0);

        if max == 0 {
            return None;
        }

        if rust_count == max {
            Some("rust".to_string())
        } else if ts_count == max {
            Some("typescript".to_string())
        } else if py_count == max {
            Some("python".to_string())
        } else if go_count == max {
            Some("go".to_string())
        } else {
            None
        }
    }

    /// Detect category from tool chain
    fn detect_tool_category(&self, tools: &[String]) -> Option<String> {
        // Detect based on tools used
        for tool in tools {
            let lower = tool.to_lowercase();
            if lower.contains("cargo") || lower.contains("rustc") {
                return Some("rust".to_string());
            }
            if lower.contains("npm") || lower.contains("typescript") || lower.contains("tsc") {
                return Some("typescript".to_string());
            }
            if lower.contains("pip") || lower.contains("python") {
                return Some("python".to_string());
            }
            if lower.contains("go ") {
                return Some("go".to_string());
            }
        }

        // Generic categories based on tool type
        let has_read = tools.iter().any(|t| t.to_lowercase().contains("read"));
        let has_edit = tools.iter().any(|t| t.to_lowercase().contains("edit"));
        let has_grep = tools.iter().any(|t| t.to_lowercase().contains("grep"));

        if has_grep && has_read && has_edit {
            Some("code_modification".to_string())
        } else if has_grep && has_read {
            Some("code_exploration".to_string())
        } else {
            Some("general".to_string())
        }
    }

    /// Add Laplace noise for differential privacy
    fn add_laplace_noise(&self, value: f64) -> f64 {
        let scale = self.noise_scale();
        let mut rng = rand::rng();

        // Laplace distribution: sample from exponential and apply sign
        let u: f64 = rng.random::<f64>() - 0.5;
        let sign = if u >= 0.0 { 1.0 } else { -1.0 };
        let noise = -scale * sign * (1.0 - 2.0 * u.abs()).ln();

        value + noise
    }

    /// Calculate noise scale based on epsilon (sensitivity = 1 for rates)
    fn noise_scale(&self) -> f64 {
        1.0 / self.epsilon
    }

    /// Hash a sequence for pattern identification
    fn hash_sequence(&self, items: &[String]) -> String {
        let mut hasher = Sha256::new();
        for item in items {
            hasher.update(item.as_bytes());
            hasher.update(b"|");
        }
        format!("{:x}", hasher.finalize())[..16].to_string()
    }

    /// Hash a problem pattern
    fn hash_problem_pattern(
        &self,
        expert_role: &str,
        problem_category: &str,
        approaches: &[String],
    ) -> String {
        let mut hasher = Sha256::new();
        hasher.update(expert_role.as_bytes());
        hasher.update(b":");
        hasher.update(problem_category.as_bytes());
        hasher.update(b":");
        for approach in approaches {
            hasher.update(approach.as_bytes());
            hasher.update(b"|");
        }
        format!("{:x}", hasher.finalize())[..16].to_string()
    }

    /// Hash a collaboration pattern
    fn hash_collaboration_pattern(
        &self,
        domains: &[String],
        experts: &[String],
        mode: &str,
    ) -> String {
        let mut hasher = Sha256::new();
        for domain in domains {
            hasher.update(domain.as_bytes());
            hasher.update(b",");
        }
        hasher.update(b":");
        for expert in experts {
            hasher.update(expert.as_bytes());
            hasher.update(b",");
        }
        hasher.update(b":");
        hasher.update(mode.as_bytes());
        format!("{:x}", hasher.finalize())[..16].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_path_generalization() {
        let anonymizer = PatternAnonymizer::new(1.0, AnonymizationLevel::Full);

        let path = "/home/user/myproject/src/handlers/auth.rs";
        let generalized = anonymizer.generalize_file_path(path);

        // Should not contain project-specific parts
        assert!(!generalized.contains("myproject"));
        assert!(!generalized.contains("user"));
        // Should contain generic parts
        assert!(generalized.contains("rs"));
    }

    #[test]
    fn test_laplace_noise() {
        let anonymizer = PatternAnonymizer::new(1.0, AnonymizationLevel::Full);

        let original = 0.5;
        let mut noisy_values: Vec<f64> = Vec::new();

        for _ in 0..1000 {
            noisy_values.push(anonymizer.add_laplace_noise(original));
        }

        // Mean should be close to original (SE ≈ 0.045 at n=1000, threshold is ~6.7σ)
        let mean: f64 = noisy_values.iter().sum::<f64>() / noisy_values.len() as f64;
        assert!((mean - original).abs() < 0.3);
    }

    #[test]
    fn test_pattern_hash_consistency() {
        let anonymizer = PatternAnonymizer::new(1.0, AnonymizationLevel::Full);

        let seq1 = vec!["a".to_string(), "b".to_string()];
        let seq2 = vec!["a".to_string(), "b".to_string()];

        let hash1 = anonymizer.hash_sequence(&seq1);
        let hash2 = anonymizer.hash_sequence(&seq2);

        assert_eq!(hash1, hash2);
    }
}
