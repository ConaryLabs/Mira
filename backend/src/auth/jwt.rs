// backend/src/auth/jwt.rs

use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::env;
use anyhow::{Result, anyhow};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,        // user_id
    pub username: String,
    pub exp: usize,         // expiration timestamp
    pub iat: usize,         // issued at timestamp
}

fn get_jwt_secret() -> String {
    env::var("JWT_SECRET").unwrap_or_else(|_| {
        "mira-jwt-secret-change-in-production-please".to_string()
    })
}

pub fn create_token(user_id: &str, username: &str) -> Result<String> {
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::days(365))
        .ok_or_else(|| anyhow!("Failed to calculate expiration"))?
        .timestamp() as usize;

    let issued_at = chrono::Utc::now().timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        exp: expiration,
        iat: issued_at,
    };

    let header = Header::default();
    let key = EncodingKey::from_secret(get_jwt_secret().as_bytes());

    encode(&header, &claims, &key)
        .map_err(|e| anyhow!("Failed to create token: {}", e))
}

pub fn verify_token(token: &str) -> Result<Claims> {
    let key = DecodingKey::from_secret(get_jwt_secret().as_bytes());
    let validation = Validation::default();

    decode::<Claims>(token, &key, &validation)
        .map(|data| data.claims)
        .map_err(|e| anyhow!("Invalid token: {}", e))
}
