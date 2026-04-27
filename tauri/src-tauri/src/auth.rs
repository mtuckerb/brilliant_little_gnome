// JWT issue/verify for the optional embedded REST API.
// Algorithm: HS256, secret stored per-user in user_preferences.jwt_secret.

use crate::error::{AppError, Result};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
}

pub async fn ensure_secret(pool: &SqlitePool) -> Result<String> {
    let row: Option<(Option<String>,)> = sqlx::query_as("SELECT jwt_secret FROM user_preferences LIMIT 1")
        .fetch_optional(pool)
        .await?;
    if let Some((Some(s),)) = row {
        if !s.is_empty() { return Ok(s); }
    }
    let new_secret = random_secret();
    sqlx::query("UPDATE user_preferences SET jwt_secret = ?, updated_at = CURRENT_TIMESTAMP WHERE id = (SELECT id FROM user_preferences LIMIT 1)")
        .bind(&new_secret)
        .execute(pool)
        .await?;
    Ok(new_secret)
}

pub fn issue(secret: &str, subject: &str, ttl_seconds: i64) -> Result<String> {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: subject.to_string(),
        iat: now,
        exp: now + ttl_seconds,
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn verify(secret: &str, token: &str) -> Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )?;
    Ok(data.claims)
}

pub fn verify_api_key(provided: &str, expected: Option<&str>) -> Result<()> {
    let expected = expected.ok_or(AppError::Unauthenticated)?;
    if !constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
        return Err(AppError::Unauthenticated);
    }
    Ok(())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn random_secret() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
