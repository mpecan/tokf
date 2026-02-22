use axum::Json;
use serde_json::{Value, json};

pub async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_returns_ok() {
        let app = crate::routes::create_router();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("failed to build request"),
            )
            .await
            .expect("failed to get response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp
            .into_body()
            .collect()
            .await
            .expect("failed to collect body")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("failed to parse JSON");
        assert_eq!(json["status"], "ok");
        assert!(json["version"].is_string());
    }
}
