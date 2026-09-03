use agro_ops_backend::{AppState, app, telemetry::init_tracing};
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() {
    init_tracing();

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect_lazy(&database_url)
        .expect("DATABASE_URL must be valid");

    let state = AppState { db };

    let address = format!("0.0.0.0:{port}");

    let listener = TcpListener::bind(&address)
        .await
        .expect("failed to bind API listener");

    info!(
        service = "api",
        version = env!("CARGO_PKG_VERSION"),
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
