use std::time::Duration;

use agro_ops_backend::worker::{Worker, walking_skeleton_test_job};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::mpsc;
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

    let heartbeat_interval = heartbeat_interval();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let db = PgPoolOptions::new()
        .max_connections(2)
        .connect_lazy(&database_url)
        .expect("DATABASE_URL must be valid");
    let worker = Worker::new(heartbeat_interval, db);
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

    info!("Agro Ops worker stopped");
}

fn heartbeat_interval() -> Duration {
    let seconds = std::env::var("WORKER_HEARTBEAT_INTERVAL_SECONDS")
        .unwrap_or_else(|_| "5".to_string())
        .parse::<u64>()
        .expect("WORKER_HEARTBEAT_INTERVAL_SECONDS must be a positive integer");

    assert!(
        seconds > 0,
        "WORKER_HEARTBEAT_INTERVAL_SECONDS must be greater than zero"
    );

    Duration::from_secs(seconds)
}
