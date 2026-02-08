use schemars::JsonSchema;
use serde::Serialize;

use super::ToolOutput;

pub type RecipeOutput = ToolOutput<RecipeData>;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum RecipeData {
    List(RecipeListData),
    Get(RecipeGetData),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecipeListData {
    pub recipes: Vec<RecipeListItem>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecipeListItem {
    pub name: String,
    pub description: String,
    pub member_count: usize,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecipeGetData {
    pub name: String,
    pub description: String,
    pub members: Vec<RecipeMemberData>,
    pub tasks: Vec<RecipeTaskData>,
    pub coordination: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecipeMemberData {
    pub name: String,
    pub agent_type: String,
    pub prompt: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecipeTaskData {
    pub subject: String,
    pub description: String,
    pub assignee: String,
}
