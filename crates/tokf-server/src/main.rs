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
    if cfg.r2_bucket.is_some()
        && cfg.r2_access_key_id.is_some()
        && cfg.r2_secret_access_key.is_some()
        && cfg.r2_endpoint_url().is_some()
    {
        Ok(Arc::new(storage::r2::R2StorageClient::new(cfg)?))
    } else {
        tracing::warn!(
            "R2 storage not configured — using no-op storage (uploads will be discarded)"
        );
        Ok(Arc::new(storage::noop::NoOpStorageClient))
    }
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
