use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type ExpertOutput = ToolOutput<ExpertData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ExpertData {
    Consult(ConsultData),
    Configure(ConfigureData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ConsultData {
    pub opinions: Vec<ExpertOpinion>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExpertOpinion {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ConfigureData {
    pub configs: Vec<ExpertConfigEntry>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ExpertConfigEntry {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_custom_prompt: Option<bool>,
}
