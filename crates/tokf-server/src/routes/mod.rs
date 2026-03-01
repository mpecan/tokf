pub mod auth;
mod filters;
mod gain;
mod health;
pub mod ip;
mod machines;
mod middleware;
mod ready;
mod sync;

#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post, put},
};

use crate::state::AppState;
use middleware::general_rate_limit;

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
        .route("/api/filters/{hash}/tests", put(filters::update_tests))
        .route(
            "/api/filters/publish-stdlib",
            post(filters::publish_stdlib).layer(DefaultBodyLimit::max(5 * 1024 * 1024)),
        )
        .route("/api/sync", post(sync::sync_usage))
        .route("/api/gain", get(gain::get_gain))
        .route("/api/gain/global", get(gain::get_global_gain))
        .route("/api/gain/filter/{hash}", get(gain::get_filter_gain))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            general_rate_limit,
        ))
        .with_state(state)
}
