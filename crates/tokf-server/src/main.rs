use std::sync::Arc;

use tokf_server::{
    auth::github::RealGitHubClient,
    config, db, routes, state,
    storage::{self, StorageClient},
};

use anyhow::Result;
use tokio::net::TcpListener;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tokf_server=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cfg = config::Config::from_env();
    let storage_client = build_storage_client(&cfg)?;

    let database_url = cfg
        .database_url
        .ok_or_else(|| anyhow::anyhow!("DATABASE_URL environment variable is required"))?;
    let pool = db::create_pool(&database_url).await?;

    if cfg.run_migrations {
        db::run_migrations(&pool).await?;
    } else {
        tracing::info!("skipping migrations (RUN_MIGRATIONS=false)");
    }

    let github_client_id = cfg
        .github_client_id
        .ok_or_else(|| anyhow::anyhow!("GITHUB_CLIENT_ID environment variable is required"))?;
    let github_client_secret = cfg
        .github_client_secret
        .ok_or_else(|| anyhow::anyhow!("GITHUB_CLIENT_SECRET environment variable is required"))?;

    let app_state = state::AppState {
        db: pool,
        github: Arc::new(RealGitHubClient::new()?),
        storage: storage_client,
        github_client_id,
        github_client_secret,
        trust_proxy: cfg.trust_proxy,
    };
    let app = routes::create_router(app_state).layer(
        // R11: explicitly disable header capture to prevent accidental secret leakage
        // when auth headers are added in the future.
        TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::new().include_headers(false)),
    );
    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("listening on {addr}");

    // O-1: graceful shutdown with a 30-second drain timeout.
    // A oneshot channel decouples OS-signal detection from axum's shutdown
    // trigger, allowing tokio::select! to race the drain against a hard deadline.
    let (drain_tx, drain_rx) = tokio::sync::oneshot::channel::<()>();

    // Wrap in async {} so the IntoFuture impl is resolved before select!
    let serve = async {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                drain_rx.await.ok();
                tracing::info!("draining in-flight requests (30 s deadline)…");
            })
            .await
    };

    tokio::select! {
        result = serve => { result?; }
        () = async {
            shutdown_signal().await;
            drain_tx.send(()).ok();
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            tracing::warn!("graceful-shutdown drain timeout after 30 s; stopping now");
        } => {}
    }

    tracing::info!("server stopped");
    Ok(())
}

fn build_storage_client(cfg: &config::Config) -> Result<Arc<dyn StorageClient>> {
    let r2_vars = [
        ("R2_BUCKET_NAME", &cfg.r2_bucket_name),
        ("R2_ACCESS_KEY_ID", &cfg.r2_access_key_id),
        ("R2_SECRET_ACCESS_KEY", &cfg.r2_secret_access_key),
    ];

    // Check if endpoint is available (either explicit or derived from account_id)
    let has_endpoint = cfg.r2_endpoint_url().is_some();

    let set_vars: Vec<&str> = r2_vars
        .iter()
        .filter(|(_, val)| val.is_some())
        .map(|(name, _)| *name)
        .collect();

    let missing_vars: Vec<&str> = r2_vars
        .iter()
        .filter(|(_, val)| val.is_none())
        .map(|(name, _)| *name)
        .collect();

    let missing_endpoint =
        !has_endpoint && cfg.r2_endpoint.is_none() && cfg.r2_account_id.is_none();

    // All required vars present
    if set_vars.len() == 3 && has_endpoint {
        return Ok(Arc::new(storage::r2::R2StorageClient::new(cfg)?));
    }

    // Partial configuration is an error
    if !set_vars.is_empty() || cfg.r2_endpoint.is_some() || cfg.r2_account_id.is_some() {
        let mut all_missing = missing_vars.clone();
        if missing_endpoint {
            all_missing.push("R2_ENDPOINT or R2_ACCOUNT_ID");
        }

        anyhow::bail!(
            "Partial R2 configuration detected. Either set all required R2 environment variables or none. Missing: {}",
            all_missing.join(", ")
        );
    }

    // No R2 configuration at all - use no-op storage
    tracing::warn!("R2 storage not configured — using no-op storage (uploads will be discarded)");
    Ok(Arc::new(storage::noop::NoOpStorageClient))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!("failed to listen for ctrl_c: {e}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => tracing::warn!("failed to install SIGTERM handler: {e}"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    tracing::info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn full_r2_config() -> config::Config {
        config::Config {
            port: 8080,
            database_url: Some("postgres://localhost/test".to_string()),
            run_migrations: true,
            trust_proxy: false,
            r2_bucket_name: Some("test-bucket".to_string()),
            r2_access_key_id: Some("AKID".to_string()),
            r2_secret_access_key: Some("secret".to_string()),
            r2_endpoint: Some("https://r2.example.com".to_string()),
            r2_account_id: None,
            github_client_id: Some("gh-client".to_string()),
            github_client_secret: Some("gh-secret".to_string()),
        }
    }

    fn empty_r2_config() -> config::Config {
        config::Config {
            port: 8080,
            database_url: Some("postgres://localhost/test".to_string()),
            run_migrations: true,
            trust_proxy: false,
            r2_bucket_name: None,
            r2_access_key_id: None,
            r2_secret_access_key: None,
            r2_endpoint: None,
            r2_account_id: None,
            github_client_id: Some("gh-client".to_string()),
            github_client_secret: Some("gh-secret".to_string()),
        }
    }

    #[test]
    fn build_storage_client_succeeds_with_full_config() {
        let cfg = full_r2_config();
        assert!(build_storage_client(&cfg).is_ok());
    }

    #[test]
    fn build_storage_client_succeeds_with_no_config() {
        let cfg = empty_r2_config();
        assert!(build_storage_client(&cfg).is_ok());
    }

    #[test]
    fn build_storage_client_fails_with_partial_config_bucket_only() {
        let mut cfg = empty_r2_config();
        cfg.r2_bucket_name = Some("test-bucket".to_string());
        let result = build_storage_client(&cfg);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(
            err.to_string()
                .contains("Partial R2 configuration detected")
        );
        assert!(err.to_string().contains("R2_ACCESS_KEY_ID"));
        assert!(err.to_string().contains("R2_SECRET_ACCESS_KEY"));
    }

    #[test]
    fn build_storage_client_fails_with_partial_config_missing_endpoint() {
        let mut cfg = full_r2_config();
        cfg.r2_endpoint = None;
        cfg.r2_account_id = None;
        let result = build_storage_client(&cfg);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(
            err.to_string()
                .contains("Partial R2 configuration detected")
        );
        assert!(err.to_string().contains("R2_ENDPOINT or R2_ACCOUNT_ID"));
    }

    #[test]
    fn build_storage_client_fails_with_partial_config_missing_credentials() {
        let mut cfg = full_r2_config();
        cfg.r2_access_key_id = None;
        cfg.r2_secret_access_key = None;
        let result = build_storage_client(&cfg);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(
            err.to_string()
                .contains("Partial R2 configuration detected")
        );
        assert!(err.to_string().contains("R2_ACCESS_KEY_ID"));
        assert!(err.to_string().contains("R2_SECRET_ACCESS_KEY"));
    }
}
