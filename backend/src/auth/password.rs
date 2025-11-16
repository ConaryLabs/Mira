// backend/src/auth/password.rs

use bcrypt::{hash, verify, BcryptError, DEFAULT_COST};
use anyhow::{Result, anyhow};

pub fn hash_password(password: &str) -> Result<String> {
    hash(password, DEFAULT_COST)
        .map_err(|e: BcryptError| anyhow!("Failed to hash password: {}", e))
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
    verify(password, hash)
        .map_err(|e: BcryptError| anyhow!("Failed to verify password: {}", e))
}
