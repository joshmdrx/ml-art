//! `GET /v1/me` — current authenticated user.

use axum::Json;
use ml_art_core::error::ApiError;
use serde::Serialize;
use uuid::Uuid;

use crate::extractors::AuthedUser;

#[derive(Serialize)]
pub struct MeResponse {
    pub id: Uuid,
    pub clerk_user_id: String,
    pub email: String,
    pub is_admin: bool,
}

pub async fn current_user(AuthedUser(user): AuthedUser) -> Result<Json<MeResponse>, ApiError> {
    Ok(Json(MeResponse {
        id: user.id,
        clerk_user_id: user.clerk_user_id,
        email: user.email,
        is_admin: user.is_admin,
    }))
}
