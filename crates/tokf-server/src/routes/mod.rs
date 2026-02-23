mod health;
mod ready;

use axum::{Router, routing::get};

use crate::state::AppState;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/ready", get(ready::ready))
        .with_state(state)
}
