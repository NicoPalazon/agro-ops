use agro_ops_backend::app;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let address = format!("0.0.0.0:{port}");

    let listener = TcpListener::bind(&address)
        .await
        .expect("failed to bind API listener");

    info!(%address, "Agro Ops API started");

    axum::serve(listener, app())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("API server failed");
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for shutdown signal");

    info!("shutdown signal received");
}
