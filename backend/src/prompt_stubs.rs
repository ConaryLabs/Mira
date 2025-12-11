// src/prompt_stubs.rs
// Stub prompts for code intelligence features
// In power suit mode, these are used for internal analysis only

pub mod internal {
    pub mod code_intelligence {
        /// System prompt for domain pattern analysis
        pub const DOMAIN_PATTERN_ANALYZER: &str = "You are a code analyzer that identifies domain patterns and suggests clustering.";

        /// System prompt for design pattern detection
        pub const DESIGN_PATTERN_DETECTOR: &str = "You are a code analyzer that detects design patterns in code.";

        /// System prompt for semantic analysis
        pub const SEMANTIC_ANALYZER: &str = "You are a code analyzer that identifies semantic relationships between code elements.";

        /// Prompt for detecting design patterns
        pub fn detect_design_patterns(code: &str, language: &str) -> String {
            format!(
                "Analyze this {} code and identify any design patterns:\n\n{}",
                language, code
            )
        }

        /// Prompt for inferring semantic relationships
        pub fn infer_relationships(code: &str) -> String {
            format!(
                "Analyze this code and identify relationships between components:\n\n{}",
                code
            )
        }

        /// Prompt for clustering code into domains
        pub fn suggest_clustering(code_elements: &str) -> String {
            format!(
                "Suggest how these code elements should be clustered:\n\n{}",
                code_elements
            )
        }
    }

    pub mod analysis {
        /// System prompt for message analysis
        pub const MESSAGE_ANALYZER: &str = "You are a message analyzer that extracts key information from conversations.";

        /// System prompt for batch analysis
        pub const BATCH_ANALYZER: &str = "You are a batch message analyzer that processes multiple messages efficiently.";

        /// Prompt for analyzing a message
        pub fn analyze_message(message: &str) -> String {
            format!(
                "Analyze this message and extract key information:\n\n{}",
                message
            )
        }
    }

    pub mod summarization {
        /// System prompt for rolling summaries
        pub const ROLLING_SUMMARIZER: &str = "You are a summarization assistant that creates concise rolling summaries of conversations.";

        /// System prompt for snapshot summaries
        pub const SNAPSHOT_SUMMARIZER: &str = "You are a summarization assistant that creates comprehensive snapshot summaries.";

        /// Prompt for rolling summary
        pub fn rolling_summary(conversation: &str) -> String {
            format!(
                "Create a rolling summary of this conversation:\n\n{}",
                conversation
            )
        }

        /// Prompt for snapshot summary
        pub fn snapshot_summary(context: &str) -> String {
            format!(
                "Create a snapshot summary of this context:\n\n{}",
                context
            )
        }
    }
}
