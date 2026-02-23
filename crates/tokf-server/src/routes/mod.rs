pub mod auth;
mod health;
mod ready;

use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(ready::ready))
        .route("/api/auth/device", post(auth::initiate_device_flow))
        .route("/api/auth/token", post(auth::poll_token))
        .with_state(state)
}
