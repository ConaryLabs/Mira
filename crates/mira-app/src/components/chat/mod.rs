// crates/mira-app/src/components/chat/mod.rs
// Chat component re-exports

mod expandable;
mod typing_indicator;
mod message_bubble;
mod markdown;
mod code_block;

pub use expandable::Expandable;
pub use typing_indicator::{TypingIndicator, ThinkingIndicator};
pub use message_bubble::{MessageBubble, ToolCallInfo};
pub use markdown::Markdown;
pub use code_block::CodeBlock;
