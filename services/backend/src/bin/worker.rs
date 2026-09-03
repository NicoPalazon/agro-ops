use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Agro Ops worker started");

    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for shutdown signal");

    info!("Agro Ops worker stopped");
}
