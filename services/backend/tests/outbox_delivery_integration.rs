use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

use agro_ops_backend::{
    jobs::{JobState, SafeErrorSummary, claim},
    outbox::{
        IdempotencyKey, NewOutboxEvent, OutboxDeliveryAdapter, OutboxDeliveryEnvelope,
        OutboxDestination, OutboxEventType, OutboxJobHandler, load_delivery_envelope, record,
        register_delivery_adapter,
    },
    worker::{JobDispatcher, JobExecutionResult, JobHandler, Worker, WorkerSettings},
};
use async_trait::async_trait;
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};
use time::{Duration as TimeDuration, OffsetDateTime};
use uuid::Uuid;

struct RecordingAdapter {
    destination: OutboxDestination,
    observed: Arc<Mutex<Vec<OutboxDeliveryEnvelope>>>,
    outcomes: Mutex<VecDeque<JobExecutionResult>>,
}

impl RecordingAdapter {
    fn new(
        destination: &str,
        observed: Arc<Mutex<Vec<OutboxDeliveryEnvelope>>>,
        outcomes: Vec<JobExecutionResult>,
    ) -> Self {
        Self {
            destination: OutboxDestination::new(destination)
                .expect("test destination must be canonical"),
            observed,
            outcomes: Mutex::new(outcomes.into()),
        }
    }
}

#[async_trait]
impl OutboxDeliveryAdapter for RecordingAdapter {
    fn destination(&self) -> &OutboxDestination {
        &self.destination
    }

    async fn deliver(&self, envelope: &OutboxDeliveryEnvelope) -> JobExecutionResult {
        self.observed
            .lock()
            .expect("observation lock must be available")
            .push(envelope.clone());
        self.outcomes
            .lock()
            .expect("outcome lock must be available")
            .pop_front()
            .unwrap_or(JobExecutionResult::Succeeded)
    }
}

async fn test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for PostgreSQL tests");
    PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("PostgreSQL must be available with migrations applied")
}

async fn persist(db: &PgPool, destination: &str, max_attempts: i32) -> (Uuid, Uuid, String) {
    let key = format!("delivery-{}", Uuid::new_v4());
    let mut transaction = db.begin().await.expect("transaction must begin");
    let recorded = record(
        &mut transaction,
        NewOutboxEvent {
            organization_id: None,
            destination: OutboxDestination::new(destination)
                .expect("destination must be canonical"),
            event_type: OutboxEventType::new("test.delivery_requested")
                .expect("event type must be canonical"),
            entity_type: None,
            entity_id: None,
            reference: Some("DELIVERY-TEST".to_owned()),
            idempotency_key: IdempotencyKey::new(&key).expect("key must be valid"),
            payload: json!({"explicit": "delivery payload"}),
            delivery_max_attempts: max_attempts,
            delivery_next_attempt_at: OffsetDateTime::now_utc() - TimeDuration::minutes(1),
        },
    )
    .await
    .expect("outbox event must record");
    transaction.commit().await.expect("transaction must commit");
    (recorded.event_id, recorded.job_id, key)
}

fn worker(db: PgPool, dispatcher: JobDispatcher, stale_threshold: Duration) -> Worker {
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

async fn state(db: &PgPool, job_id: Uuid) -> (JobState, i32) {
    let (state, attempts): (String, i32) =
        sqlx::query_as("SELECT estado, intentos FROM jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(db)
            .await
            .expect("job state must load");
    (
        JobState::try_from(state.as_str()).expect("state must be supported"),
        attempts,
    )
}

async fn linked_counts(db: &PgPool, event_id: Uuid, job_id: Uuid) -> (i64, i64, i64) {
    sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*) FROM outbox_events WHERE id = $1),
            (SELECT COUNT(*) FROM jobs WHERE id = $2),
            (SELECT COUNT(*) FROM outbox_job_links WHERE outbox_event_id = $1 AND job_id = $2)
        "#,
    )
    .bind(event_id)
    .bind(job_id)
    .fetch_one(db)
    .await
    .expect("linked counts must load")
}

#[tokio::test]
async fn registered_destination_loads_relational_envelope_and_completes_while_unregistered_waits() {
    let db = test_pool().await;
    let destination = format!("registered_{}", &Uuid::new_v4().simple().to_string()[..12]);
    let unregistered = format!(
        "unregistered_{}",
        &Uuid::new_v4().simple().to_string()[..12]
    );
    let (event_id, job_id, key) = persist(&db, &destination, 3).await;
    let (_, unsupported_job_id, _) = persist(&db, &unregistered, 3).await;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let adapter = Arc::new(RecordingAdapter::new(
        &destination,
        observed.clone(),
        vec![JobExecutionResult::Succeeded],
    ));
    let mut dispatcher = JobDispatcher::empty();
    register_delivery_adapter(&mut dispatcher, db.clone(), adapter)
        .expect("adapter must register once");

    worker(db.clone(), dispatcher, Duration::from_secs(300))
        .process_queue_once()
        .await;

    assert_eq!(state(&db, job_id).await, (JobState::Completado, 1));
    assert_eq!(
        state(&db, unsupported_job_id).await,
        (JobState::Pendiente, 0)
    );
    let observed = observed.lock().expect("observations must be available");
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].event_id, event_id);
    assert_eq!(observed[0].idempotency_key.as_str(), key);
    assert_eq!(observed[0].payload, json!({"explicit": "delivery payload"}));
}

#[tokio::test]
async fn retry_and_exhaustion_are_owned_only_by_the_linked_job() {
    let db = test_pool().await;
    let retry_destination = format!("retry_{}", &Uuid::new_v4().simple().to_string()[..12]);
    let (event_id, job_id, _) = persist(&db, &retry_destination, 2).await;
    let observed = Arc::new(Mutex::new(Vec::new()));
    let retry = JobExecutionResult::Retry {
        next_attempt_at: OffsetDateTime::now_utc() - TimeDuration::seconds(1),
        error: SafeErrorSummary::new("Falla externa segura").expect("summary must be safe"),
    };
    let adapter = Arc::new(RecordingAdapter::new(
        &retry_destination,
        observed.clone(),
        vec![retry, JobExecutionResult::Succeeded],
    ));
    let mut dispatcher = JobDispatcher::empty();
    register_delivery_adapter(&mut dispatcher, db.clone(), adapter).expect("adapter must register");
    let runtime = worker(db.clone(), dispatcher, Duration::from_secs(300));

    runtime.process_queue_once().await;
    assert_eq!(state(&db, job_id).await, (JobState::Pendiente, 1));
    runtime.process_queue_once().await;
    assert_eq!(state(&db, job_id).await, (JobState::Completado, 2));
    assert_eq!(linked_counts(&db, event_id, job_id).await, (1, 1, 1));
    let observed_snapshot = observed
        .lock()
        .expect("observations must be available")
        .clone();
    assert_eq!(observed_snapshot.len(), 2);
    assert_eq!(observed_snapshot[0], observed_snapshot[1]);

    let final_destination = format!("final_{}", &Uuid::new_v4().simple().to_string()[..12]);
    let (final_event_id, final_job_id, _) = persist(&db, &final_destination, 1).await;
    let final_adapter = Arc::new(RecordingAdapter::new(
        &final_destination,
        Arc::new(Mutex::new(Vec::new())),
        vec![JobExecutionResult::Retry {
            next_attempt_at: OffsetDateTime::now_utc(),
            error: SafeErrorSummary::new("Falla final segura").expect("summary must be safe"),
        }],
    ));
    let mut dispatcher = JobDispatcher::empty();
    register_delivery_adapter(&mut dispatcher, db.clone(), final_adapter)
        .expect("adapter must register");
    worker(db.clone(), dispatcher, Duration::from_secs(300))
        .process_queue_once()
        .await;
    assert_eq!(state(&db, final_job_id).await, (JobState::Agotado, 1));
    assert_eq!(
        linked_counts(&db, final_event_id, final_job_id).await,
        (1, 1, 1)
    );
}

#[tokio::test]
async fn stale_recovery_redelivers_the_same_event_key_and_payload_after_remote_success_before_ack()
{
    let db = test_pool().await;
    let destination = format!("crash_{}", &Uuid::new_v4().simple().to_string()[..12]);
    let (event_id, job_id, key) = persist(&db, &destination, 3).await;
    let destination_code =
        OutboxDestination::new(&destination).expect("destination must be canonical");
    let old_worker_id = Uuid::new_v4();
    let claimed = claim(
        &db,
        old_worker_id,
        &[destination_code.delivery_job_type()],
        1,
    )
    .await
    .expect("old worker must claim delivery");
    assert_eq!(claimed.len(), 1);

    let observed = Arc::new(Mutex::new(Vec::new()));
    let adapter = Arc::new(RecordingAdapter::new(
        &destination,
        observed.clone(),
        vec![JobExecutionResult::Succeeded, JobExecutionResult::Succeeded],
    ));
    let handler = OutboxJobHandler::new(db.clone(), adapter.clone());
    assert_eq!(
        handler.execute(&claimed[0]).await,
        JobExecutionResult::Succeeded
    );
    // Simulate a crash/ownership loss after the remote effect but before mark_succeeded.
    tokio::time::sleep(Duration::from_millis(10)).await;
    let mut dispatcher = JobDispatcher::empty();
    register_delivery_adapter(&mut dispatcher, db.clone(), adapter).expect("adapter must register");
    worker(db.clone(), dispatcher, Duration::from_millis(1))
        .process_queue_once()
        .await;

    assert_eq!(state(&db, job_id).await, (JobState::Completado, 2));
    assert_eq!(linked_counts(&db, event_id, job_id).await, (1, 1, 1));
    let observed_snapshot = observed
        .lock()
        .expect("observations must be available")
        .clone();
    assert_eq!(observed_snapshot.len(), 2);
    assert_eq!(observed_snapshot[0].event_id, event_id);
    assert_eq!(observed_snapshot[1].event_id, event_id);
    assert_eq!(observed_snapshot[0].idempotency_key.as_str(), key);
    assert_eq!(observed_snapshot[1].idempotency_key.as_str(), key);
    assert_eq!(observed_snapshot[0].payload, observed_snapshot[1].payload);

    let loaded = load_delivery_envelope(&db, job_id)
        .await
        .expect("relational envelope must remain loadable");
    assert_eq!(loaded, observed_snapshot[0]);
}
