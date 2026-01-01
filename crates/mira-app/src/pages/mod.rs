// crates/mira-app/src/pages/mod.rs
// Page components for Mira Studio

mod home;
mod ghost;
mod memories;
mod code;
mod tasks;
mod chat;

pub use home::HomePage;
pub use ghost::GhostModePage;
pub use memories::MemoriesPage;
pub use code::CodePage;
pub use tasks::TasksPage;
pub use chat::ChatPage;
