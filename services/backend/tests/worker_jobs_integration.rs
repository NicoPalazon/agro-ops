use std::{sync::Arc, time::Duration};

use agro_ops_backend::{
    jobs::{JobType, NewJob, SafeErrorSummary, claim, enqueue},
    worker::{JobDispatcher, JobExecutionResult, JobHandler, Worker, WorkerSettings},
};
use async_trait::async_trait;
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

struct FixedHandler {
    job_type: JobType,
    result: JobExecutionResult,
}

#[async_trait]
impl JobHandler for FixedHandler {
    fn job_type(&self) -> &JobType {
        &self.job_type
    }

    async fn execute(&self, _job: &agro_ops_backend::jobs::ClaimedJob) -> JobExecutionResult {
        self.result.clone()
    }
}

async fn test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for PostgreSQL tests");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("PostgreSQL must be available with migrations applied")
}

fn unique_type(label: &str) -> JobType {
    JobType::new(format!("worker.{label}_t{}", Uuid::new_v4().simple()))
        .expect("test worker job type must be canonical")
}

async fn persist(db: &PgPool, job_type: JobType, max_attempts: i32) -> Uuid {
    let mut transaction = db.begin().await.expect("transaction must begin");
    let job = enqueue(
        &mut transaction,
        NewJob {
            organization_id: None,
            job_type,
            payload: json!({"input": "worker-test"}),
            max_attempts,
            initial_next_attempt_at: OffsetDateTime::now_utc() - TimeDuration::minutes(1),
        },
    )
    .await
    .expect("worker test job must enqueue");
    transaction.commit().await.expect("transaction must commit");
    job.id
}

fn worker(db: PgPool, handler: Option<FixedHandler>, stale_threshold: Duration) -> Worker {
    let mut dispatcher = JobDispatcher::empty();
    if let Some(handler) = handler {
        dispatcher
            .register(Arc::new(handler))
            .expect("test handler must register once");
    }
    Worker::new(
        WorkerSettings {
            heartbeat_interval: Duration::from_secs(60),
            poll_interval: Duration::from_secs(60),
            claim_batch_size: 10,
            stale_threshold,
        },
        db,
        dispatcher,
    )
}

async fn state(db: &PgPool, job_id: Uuid) -> (String, i32, Option<String>) {
    sqlx::query_as("SELECT estado, intentos, ultimo_error FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(db)
        .await
        .expect("worker test job must be queryable")
}

#[tokio::test]
async fn worker_claims_and_completes_supported_work_without_touching_unsupported_work() {
    let db = test_pool().await;
    let supported = unique_type("success");
    let unsupported = unique_type("unsupported");
    let supported_id = persist(&db, supported.clone(), 3).await;
    let unsupported_id = persist(&db, unsupported, 3).await;
    let worker = worker(
        db.clone(),
        Some(FixedHandler {
            job_type: supported,
            result: JobExecutionResult::Succeeded,
        }),
        Duration::from_secs(300),
    );

    worker.process_queue_once().await;

    assert_eq!(
        state(&db, supported_id).await,
        ("completado".to_owned(), 1, None)
    );
    assert_eq!(
        state(&db, unsupported_id).await,
        ("pendiente".to_owned(), 0, None)
    );
}

#[tokio::test]
async fn worker_schedules_retry_and_exhausts_the_final_attempt() {
    let db = test_pool().await;
    let retry_type = unique_type("retry");
    let retry_id = persist(&db, retry_type.clone(), 2).await;
    let retry_at = OffsetDateTime::now_utc() + TimeDuration::hours(1);
    worker(
        db.clone(),
        Some(FixedHandler {
            job_type: retry_type,
            result: JobExecutionResult::Retry {
                next_attempt_at: retry_at,
                error: SafeErrorSummary::new("Reintento seguro").expect("test error must be safe"),
            },
        }),
        Duration::from_secs(300),
    )
    .process_queue_once()
    .await;
    let retry: (String, i32, String, bool) = sqlx::query_as(
        "SELECT estado, intentos, ultimo_error, next_attempt_at > statement_timestamp() FROM jobs WHERE id = $1",
    )
    .bind(retry_id)
    .fetch_one(&db)
    .await
    .expect("retry state must be queryable");
    assert_eq!(
        retry,
        (
            "pendiente".to_owned(),
            1,
            "Reintento seguro".to_owned(),
            true
        )
    );

    let final_type = unique_type("final");
    let final_id = persist(&db, final_type.clone(), 1).await;
    worker(
        db.clone(),
        Some(FixedHandler {
            job_type: final_type,
            result: JobExecutionResult::Retry {
                next_attempt_at: retry_at,
                error: SafeErrorSummary::new("Falla final segura")
                    .expect("test error must be safe"),
            },
        }),
        Duration::from_secs(300),
    )
    .process_queue_once()
    .await;
    assert_eq!(
        state(&db, final_id).await,
        (
            "agotado".to_owned(),
            1,
            Some("Falla final segura".to_owned())
        )
    );
}

#[tokio::test]
async fn worker_recovers_stale_owned_work_before_consuming_it() {
    let db = test_pool().await;
    let job_type = unique_type("stale");
    let job_id = persist(&db, job_type.clone(), 2).await;
    claim(&db, Uuid::new_v4(), std::slice::from_ref(&job_type), 1)
        .await
        .expect("old worker must claim test job");
    tokio::time::sleep(Duration::from_millis(10)).await;
    let worker = worker(
        db.clone(),
        Some(FixedHandler {
            job_type,
            result: JobExecutionResult::Succeeded,
        }),
        Duration::from_millis(1),
    );

    worker.process_queue_once().await;

    let recovered = state(&db, job_id).await;
    assert_eq!(recovered.0, "completado");
    assert_eq!(recovered.1, 2);
    assert_eq!(
        recovered.2.as_deref(),
        Some(agro_ops_backend::jobs::STALE_RECOVERY_ERROR)
    );
}

#[tokio::test]
async fn queue_enabled_worker_keeps_persisting_the_existing_heartbeat() {
    let db = test_pool().await;
    let runtime = Worker::new(
        WorkerSettings {
            heartbeat_interval: Duration::from_millis(5),
            poll_interval: Duration::from_secs(60),
            claim_batch_size: 1,
            stale_threshold: Duration::from_secs(300),
        },
        db.clone(),
        JobDispatcher::empty(),
    );
    let (job_sender, job_receiver) = mpsc::channel(1);
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let task = tokio::spawn(runtime.run(job_receiver, async {
        let _ = shutdown_receiver.await;
    }));
    tokio::time::sleep(Duration::from_millis(25)).await;
    shutdown_sender
        .send(())
        .expect("worker must remain available for shutdown");
    task.await.expect("worker must stop cleanly");
    drop(job_sender);

    let recent: bool = sqlx::query_scalar(
        "SELECT last_seen_at >= statement_timestamp() - INTERVAL '1 second' FROM service_heartbeats WHERE service_name = 'worker'",
    )
    .fetch_one(&db)
    .await
    .expect("worker heartbeat must be persisted");
    assert!(recent);
}
