// backend/src/git/intelligence/mod.rs
// Git Intelligence: Commit tracking, co-change patterns, blame, expertise, historical fixes

pub mod blame;
pub mod cochange;
pub mod commits;
pub mod expertise;
pub mod fixes;

pub use blame::{BlameAnnotation, BlameService};
pub use cochange::{CochangePattern, CochangeService, CochangeSuggestion};
pub use commits::{CommitService, GitCommit, CommitFileChange};
pub use expertise::{AuthorExpertise, ExpertiseService, ExpertiseQuery};
pub use fixes::{HistoricalFix, FixService, FixMatch};
