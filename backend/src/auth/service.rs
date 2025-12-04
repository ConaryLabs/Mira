// backend/src/auth/service.rs

use anyhow::{Result, anyhow};
use sqlx::SqlitePool;
use uuid::Uuid;

use super::jwt::create_token;
use super::password::{hash_password, verify_password};
use super::models::{User, UserWithPassword, LoginRequest, RegisterRequest, AuthResponse, ChangePasswordRequest, UpdatePreferencesRequest};

pub struct AuthService {
    db: SqlitePool,
}

impl AuthService {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    pub async fn login(&self, req: LoginRequest) -> Result<AuthResponse> {
        let user = self.get_user_by_username(&req.username).await?;

        if !user.is_active {
            return Err(anyhow!("User account is disabled"));
        }

        if !verify_password(&req.password, &user.password_hash)? {
            return Err(anyhow!("Invalid credentials"));
        }

        let token = create_token(&user.id, &user.username)?;

        self.update_last_login(&user.id).await?;

        Ok(AuthResponse {
            user: user.into(),
            token,
        })
    }

    pub async fn register(&self, req: RegisterRequest) -> Result<AuthResponse> {
        if self.username_exists(&req.username).await? {
            return Err(anyhow!("Username already exists"));
        }

        if let Some(ref email) = req.email {
            if self.email_exists(email).await? {
                return Err(anyhow!("Email already exists"));
            }
        }

        let user_id = Uuid::new_v4().to_string();
        let password_hash = hash_password(&req.password)?;
        let now = chrono::Utc::now().timestamp();

        sqlx::query(
            r#"
            INSERT INTO users (id, username, email, password_hash, display_name, created_at, updated_at, is_active)
            VALUES (?, ?, ?, ?, ?, ?, ?, TRUE)
            "#
        )
        .bind(&user_id)
        .bind(&req.username)
        .bind(&req.email)
        .bind(&password_hash)
        .bind(&req.display_name)
        .bind(now)
        .bind(now)
        .execute(&self.db)
        .await?;

        let user = self.get_user_by_id(&user_id).await?;
        let token = create_token(&user.id, &user.username)?;

        Ok(AuthResponse {
            user: user.into(),
            token,
        })
    }

    pub async fn verify_user_id(&self, user_id: &str) -> Result<User> {
        let user = self.get_user_by_id(user_id).await?;

        if !user.is_active {
            return Err(anyhow!("User account is disabled"));
        }

        Ok(user.into())
    }

    async fn get_user_by_username(&self, username: &str) -> Result<UserWithPassword> {
        sqlx::query_as::<_, UserWithPassword>(
            "SELECT * FROM users WHERE username = ?"
        )
        .bind(username)
        .fetch_one(&self.db)
        .await
        .map_err(|_| anyhow!("Invalid credentials"))
    }

    async fn get_user_by_id(&self, user_id: &str) -> Result<UserWithPassword> {
        sqlx::query_as::<_, UserWithPassword>(
            "SELECT * FROM users WHERE id = ?"
        )
        .bind(user_id)
        .fetch_one(&self.db)
        .await
        .map_err(|_| anyhow!("User not found"))
    }

    async fn username_exists(&self, username: &str) -> Result<bool> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM users WHERE username = ?"
        )
        .bind(username)
        .fetch_one(&self.db)
        .await?;

        Ok(count.0 > 0)
    }

    async fn email_exists(&self, email: &str) -> Result<bool> {
        let count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM users WHERE email = ?"
        )
        .bind(email)
        .fetch_one(&self.db)
        .await?;

        Ok(count.0 > 0)
    }

    async fn update_last_login(&self, user_id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();

        sqlx::query("UPDATE users SET last_login_at = ?, updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(now)
            .bind(user_id)
            .execute(&self.db)
            .await?;

        Ok(())
    }

    pub async fn change_password(&self, user_id: &str, req: ChangePasswordRequest) -> Result<()> {
        let user = self.get_user_by_id(user_id).await?;

        if !verify_password(&req.current_password, &user.password_hash)? {
            return Err(anyhow!("Current password is incorrect"));
        }

        if req.new_password.len() < 8 {
            return Err(anyhow!("New password must be at least 8 characters"));
        }

        let new_hash = hash_password(&req.new_password)?;
        let now = chrono::Utc::now().timestamp();

        sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
            .bind(&new_hash)
            .bind(now)
            .bind(user_id)
            .execute(&self.db)
            .await?;

        Ok(())
    }

    pub async fn update_preferences(&self, user_id: &str, req: UpdatePreferencesRequest) -> Result<User> {
        let now = chrono::Utc::now().timestamp();

        if let Some(ref theme) = req.theme_preference {
            // Validate theme value
            if theme != "light" && theme != "dark" {
                return Err(anyhow!("Invalid theme preference. Must be 'light' or 'dark'"));
            }

            sqlx::query("UPDATE users SET theme_preference = ?, updated_at = ? WHERE id = ?")
                .bind(theme)
                .bind(now)
                .bind(user_id)
                .execute(&self.db)
                .await?;
        }

        let user = self.get_user_by_id(user_id).await?;
        Ok(user.into())
    }
}
