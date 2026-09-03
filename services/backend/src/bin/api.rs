use std::net::SocketAddr;

use agro_ops_backend::{AppState, app, config::RuntimeConfig, telemetry::init_tracing};
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    init_tracing();

    let config = RuntimeConfig::from_env().unwrap_or_else(|error| {
        error!(service = "api", %error, "startup configuration invalid");
        std::process::exit(1);
    });

    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect_lazy(config.database_url())
        .unwrap_or_else(|_| {
            error!(
                service = "api",
                "invalid configuration: DATABASE_URL must be a valid PostgreSQL connection URL"
            );
            std::process::exit(1);
        });

    let state = AppState { db };

    let address = SocketAddr::from(([0, 0, 0, 0], config.port()));

    let listener = TcpListener::bind(address).await.unwrap_or_else(|error| {
        error!(service = "api", %error, "failed to bind API listener");
        std::process::exit(1);
    });

    info!(
        service = "api",
        version = env!("CARGO_PKG_VERSION"),
        environment = %config.app_environment(),
        %address,
        "Agro Ops API started"
    );

    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("API server failed");
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for shutdown signal");

    info!(service = "api", "shutdown signal received");
}
