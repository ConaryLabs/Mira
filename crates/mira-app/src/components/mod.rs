// crates/mira-app/src/components/mod.rs
// Component re-exports

mod layout;
mod sidebar;
pub mod chat;

pub use layout::{Layout, Nav, NotFound};
pub use sidebar::ProjectSidebar;
