use agro_ops_backend::{
    config::RuntimeConfig,
    telemetry::init_tracing,
    worker::{Worker, walking_skeleton_test_job},
};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::mpsc;
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
    let worker = Worker::new(config.worker_heartbeat_interval(), db);
    let (job_sender, job_receiver) = mpsc::channel(1);

    if std::env::args().any(|argument| argument == "--test-job-once") {
        let (job, completed) = walking_skeleton_test_job();

        job_sender
            .send(job)
            .await
            .expect("worker must be available for the walking skeleton test job");

        worker
            .run(job_receiver, async {
                completed
                    .await
                    .expect("walking skeleton test job must complete");
            })
            .await;
    } else {
        let _job_sender = job_sender;

        worker
            .run(job_receiver, async {
                tokio::signal::ctrl_c()
                    .await
                    .expect("failed to listen for shutdown signal");
            })
            .await;
    }

    info!(service = "worker", "Agro Ops worker stopped");
}
