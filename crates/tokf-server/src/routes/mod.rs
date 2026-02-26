pub mod auth;
mod filters;
mod gain;
mod health;
mod machines;
mod ready;
mod sync;

#[cfg(test)]
pub mod test_helpers;

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
        .route(
            "/api/machines",
            post(machines::register_machine).get(machines::list_machines),
        )
        .route(
            "/api/filters",
            post(filters::publish_filter).get(filters::search_filters),
        )
        .route("/api/filters/{hash}", get(filters::get_filter))
        .route(
            "/api/filters/{hash}/download",
            get(filters::download_filter),
        )
        .route("/api/sync", post(sync::sync_usage))
        .route("/api/gain", get(gain::get_gain))
        .route("/api/gain/global", get(gain::get_global_gain))
        .route("/api/gain/filter/{hash}", get(gain::get_filter_gain))
        .with_state(state)
}
