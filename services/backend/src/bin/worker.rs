use agro_ops_backend::{
    config::RuntimeConfig,
    shutdown::shutdown_signal,
    telemetry::init_tracing,
    worker::{JobDispatcher, Worker, WorkerSettings},
};
use sqlx::postgres::PgPoolOptions;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    init_tracing();

    let config = RuntimeConfig::from_env().unwrap_or_else(|error| {
        error!(service = "worker", %error, "startup configuration invalid");
        std::process::exit(1);
    });

    info!(
        service = "worker",
        version = env!("CARGO_PKG_VERSION"),
        environment = %config.app_environment(),
        "Agro Ops worker started"
    );

    let db = PgPoolOptions::new()
        .max_connections(2)
        .connect_lazy(config.database_url())
        .unwrap_or_else(|_| {
            error!(
                service = "worker",
                "invalid configuration: DATABASE_URL must be a valid PostgreSQL connection URL"
            );
            std::process::exit(1);
        });
    let worker = Worker::new(
        WorkerSettings {
            heartbeat_interval: config.worker_heartbeat_interval(),
            poll_interval: config.job_poll_interval(),
            claim_batch_size: config.job_claim_batch_size(),
            stale_threshold: config.job_stale_threshold(),
        },
        db,
        JobDispatcher::empty(),
    );
    worker
        .run(async {
            let signal = shutdown_signal().await;
            info!(
                service = "worker",
                signal = signal.as_str(),
                "shutdown signal received"
            );
        })
        .await;

    info!(service = "worker", "Agro Ops worker stopped");
}
