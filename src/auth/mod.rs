// src/auth/mod.rs
// Authentication and session management module
// Currently uses hardcoded values but structured for easy auth integration

pub mod session;

pub use session::{AuthContext, get_session_id, validate_auth};

// Future modules:
// pub mod jwt;
// pub mod oauth;
// pub mod middleware;
