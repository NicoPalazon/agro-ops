use std::{sync::OnceLock, time::Duration};

use agro_ops_backend::jobs::{
    JobState, JobType, NewJob, OwnershipTransition, SafeErrorSummary, claim, enqueue, mark_failed,
    mark_succeeded, recover_stale,
};
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};
use time::{Duration as TimeDuration, OffsetDateTime};
use uuid::Uuid;

async fn test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for PostgreSQL tests");
    PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("PostgreSQL must be available with migrations applied")
}

fn database_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn unique_type(label: &str) -> JobType {
    JobType::new(format!("test.{label}_t{}", Uuid::new_v4().simple()))
        .expect("test job type must be canonical")
}

fn new_job(job_type: JobType, max_attempts: i32, next: OffsetDateTime) -> NewJob {
    NewJob {
        organization_id: None,
        job_type,
        payload: json!({"input": "explicit-test-value"}),
        max_attempts,
        initial_next_attempt_at: next,
    }
}

async fn persist(db: &PgPool, job: NewJob) -> agro_ops_backend::jobs::Job {
    let mut transaction = db.begin().await.expect("transaction must begin");
    let job = enqueue(&mut transaction, job)
        .await
        .expect("job must enqueue");
    transaction.commit().await.expect("transaction must commit");
    job
}

fn due() -> OffsetDateTime {
    OffsetDateTime::now_utc() - TimeDuration::minutes(1)
}

fn future() -> OffsetDateTime {
    OffsetDateTime::now_utc() + TimeDuration::hours(1)
}

fn safe_error() -> SafeErrorSummary {
    SafeErrorSummary::new("Falla operativa segura").expect("safe error must be valid")
}

#[tokio::test]
async fn enqueue_persists_pending_and_joins_the_callers_transaction() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let committed_type = unique_type("enqueue");
    let committed = persist(&db, new_job(committed_type, 3, future())).await;

    assert_eq!(committed.state, JobState::Pendiente);
    assert_eq!(committed.attempts, 0);
    assert_eq!(committed.max_attempts, 3);
    assert!(committed.locked_at.is_none());
    assert!(committed.locked_by.is_none());
    assert!(committed.completed_at.is_none());
    let persisted_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM jobs WHERE id = $1")
            .bind(committed.id)
            .fetch_one(&db)
            .await
            .expect("persisted job must be queryable");
    assert_eq!(persisted_count, 1);

    let rolled_back_type = unique_type("rollback");
    let mut transaction = db.begin().await.expect("transaction must begin");
    let rolled_back = enqueue(&mut transaction, new_job(rolled_back_type, 2, future()))
        .await
        .expect("job must enqueue inside caller transaction");
    transaction
        .rollback()
        .await
        .expect("caller must be able to roll back");
    let rolled_back_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM jobs WHERE id = $1")
            .bind(rolled_back.id)
            .fetch_one(&db)
            .await
            .expect("rolled back job absence must be queryable");
    assert_eq!(rolled_back_count, 0);
}

#[tokio::test]
async fn claim_uses_database_due_time_and_records_one_attempt_and_owner() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let job_type = unique_type("eligibility");
    let future_job = persist(&db, new_job(job_type.clone(), 3, future())).await;
    assert!(
        claim(&db, Uuid::new_v4(), std::slice::from_ref(&job_type), 10)
            .await
            .expect("future claim must execute")
            .is_empty()
    );

    let due_job = persist(&db, new_job(job_type.clone(), 3, due())).await;
    let worker_id = Uuid::new_v4();
    let claimed = claim(&db, worker_id, std::slice::from_ref(&job_type), 10)
        .await
        .expect("due claim must execute");
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, due_job.id);
    assert_eq!(claimed[0].state, JobState::Ejecutando);
    assert_eq!(claimed[0].attempts, 1);
    assert_eq!(claimed[0].locked_by, Some(worker_id));
    assert!(claimed[0].locked_at.is_some());

    let future_state: String = sqlx::query_scalar("SELECT estado FROM jobs WHERE id = $1")
        .bind(future_job.id)
        .fetch_one(&db)
        .await
        .expect("future job must remain queryable");
    assert_eq!(future_state, "pendiente");
}

#[tokio::test]
async fn successful_completion_is_terminal_and_enforces_ownership() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let job_type = unique_type("success");
    let original = persist(&db, new_job(job_type.clone(), 3, due())).await;
    let owner = Uuid::new_v4();
    claim(&db, owner, std::slice::from_ref(&job_type), 1)
        .await
        .expect("job must claim");

    assert_eq!(
        mark_succeeded(&db, original.id, Uuid::new_v4())
            .await
            .expect("non-owner transition must execute"),
        OwnershipTransition::OwnershipLost
    );
    let completed = match mark_succeeded(&db, original.id, owner)
        .await
        .expect("owner completion must execute")
    {
        OwnershipTransition::Applied(job) => job,
        OwnershipTransition::OwnershipLost => panic!("owner unexpectedly lost ownership"),
    };
    assert_eq!(completed.state, JobState::Completado);
    assert_eq!(completed.attempts, 1);
    assert!(completed.completed_at.is_some());
    assert!(completed.locked_at.is_none());
    assert!(completed.locked_by.is_none());
    assert!(
        claim(&db, Uuid::new_v4(), &[job_type], 1)
            .await
            .expect("terminal claim must execute")
            .is_empty()
    );
}

#[tokio::test]
async fn retryable_and_final_failures_follow_attempt_limits_and_enforce_ownership() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let retry_type = unique_type("retry");
    let retry_job = persist(&db, new_job(retry_type.clone(), 2, due())).await;
    let owner = Uuid::new_v4();
    claim(&db, owner, std::slice::from_ref(&retry_type), 1)
        .await
        .expect("job must claim");
    assert_eq!(
        mark_failed(&db, retry_job.id, Uuid::new_v4(), future(), &safe_error())
            .await
            .expect("non-owner failure must execute"),
        OwnershipTransition::OwnershipLost
    );
    let requested_retry = future();
    let retried = match mark_failed(&db, retry_job.id, owner, requested_retry, &safe_error())
        .await
        .expect("owner failure must execute")
    {
        OwnershipTransition::Applied(job) => job,
        OwnershipTransition::OwnershipLost => panic!("owner unexpectedly lost ownership"),
    };
    assert_eq!(retried.state, JobState::Pendiente);
    assert_eq!(
        retried.next_attempt_at.unix_timestamp_nanos() / 1_000,
        requested_retry.unix_timestamp_nanos() / 1_000
    );
    assert_eq!(
        retried.last_error.as_deref(),
        Some("Falla operativa segura")
    );

    let final_type = unique_type("final_failure");
    let final_job = persist(&db, new_job(final_type.clone(), 1, due())).await;
    let final_owner = Uuid::new_v4();
    claim(&db, final_owner, std::slice::from_ref(&final_type), 1)
        .await
        .expect("final-attempt job must claim");
    let exhausted = match mark_failed(&db, final_job.id, final_owner, future(), &safe_error())
        .await
        .expect("final failure must execute")
    {
        OwnershipTransition::Applied(job) => job,
        OwnershipTransition::OwnershipLost => panic!("owner unexpectedly lost ownership"),
    };
    assert_eq!(exhausted.state, JobState::Agotado);
    assert_eq!(exhausted.attempts, 1);
    assert!(exhausted.locked_at.is_none());
    assert!(exhausted.locked_by.is_none());
    assert!(
        claim(&db, Uuid::new_v4(), &[final_type], 1)
            .await
            .expect("exhausted claim must execute")
            .is_empty()
    );
}

#[tokio::test]
async fn concurrent_workers_skip_locked_rows_without_duplicate_claims() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let one_type = unique_type("concurrent_one");
    let one_job = persist(&db, new_job(one_type.clone(), 3, due())).await;
    let (first, second) = tokio::join!(
        claim(&db, Uuid::new_v4(), std::slice::from_ref(&one_type), 1),
        claim(&db, Uuid::new_v4(), std::slice::from_ref(&one_type), 1),
    );
    let claimed: Vec<_> = first
        .expect("first concurrent claim must execute")
        .into_iter()
        .chain(second.expect("second concurrent claim must execute"))
        .collect();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, one_job.id);
    assert_eq!(claimed[0].attempts, 1);

    let many_type = unique_type("concurrent_many");
    let first_job = persist(&db, new_job(many_type.clone(), 3, due())).await;
    let second_job = persist(&db, new_job(many_type.clone(), 3, due())).await;
    let (first, second) = tokio::join!(
        claim(&db, Uuid::new_v4(), std::slice::from_ref(&many_type), 1),
        claim(&db, Uuid::new_v4(), std::slice::from_ref(&many_type), 1),
    );
    let claimed: Vec<_> = first
        .expect("first multi-row claim must execute")
        .into_iter()
        .chain(second.expect("second multi-row claim must execute"))
        .collect();
    assert_eq!(claimed.len(), 2);
    let mut ids = claimed.iter().map(|job| job.id).collect::<Vec<_>>();
    ids.sort();
    let mut expected = vec![first_job.id, second_job.id];
    expected.sort();
    assert_eq!(ids, expected);
    assert_ne!(claimed[0].locked_by, claimed[1].locked_by);
}

#[tokio::test]
async fn claim_order_is_deterministic_and_unsupported_types_are_untouched() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let supported = unique_type("ordering");
    let unsupported = unique_type("unsupported");
    let first = persist(
        &db,
        new_job(
            supported.clone(),
            3,
            OffsetDateTime::now_utc() - TimeDuration::minutes(3),
        ),
    )
    .await;
    let second = persist(
        &db,
        new_job(
            supported.clone(),
            3,
            OffsetDateTime::now_utc() - TimeDuration::minutes(2),
        ),
    )
    .await;
    let third = persist(
        &db,
        new_job(
            supported.clone(),
            3,
            OffsetDateTime::now_utc() - TimeDuration::minutes(1),
        ),
    )
    .await;
    let unsupported_job = persist(&db, new_job(unsupported, 1, due())).await;

    let claimed = claim(&db, Uuid::new_v4(), &[supported], 3)
        .await
        .expect("ordered claim must execute");
    assert_eq!(
        claimed.iter().map(|job| job.id).collect::<Vec<_>>(),
        vec![first.id, second.id, third.id]
    );
    let untouched: (String, i32) =
        sqlx::query_as("SELECT estado, intentos FROM jobs WHERE id = $1")
            .bind(unsupported_job.id)
            .fetch_one(&db)
            .await
            .expect("unsupported job must be queryable");
    assert_eq!(untouched, ("pendiente".to_owned(), 0));
}

#[tokio::test]
async fn stale_jobs_recover_to_pending_or_exhausted_and_old_owners_cannot_overwrite() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let retry_type = unique_type("stale_retry");
    let retry_job = persist(&db, new_job(retry_type.clone(), 2, due())).await;
    let old_owner = Uuid::new_v4();
    claim(&db, old_owner, std::slice::from_ref(&retry_type), 1)
        .await
        .expect("stale retry job must claim");
    tokio::time::sleep(Duration::from_millis(10)).await;
    let recovered = recover_stale(&db, Duration::from_millis(1), 100)
        .await
        .expect("stale retry job must recover");
    assert!(recovered.pending >= 1);
    let recovered_row: (String, Option<Uuid>, String, bool) = sqlx::query_as(
        "SELECT estado, bloqueado_por, ultimo_error, next_attempt_at <= statement_timestamp() FROM jobs WHERE id = $1",
    )
    .bind(retry_job.id)
    .fetch_one(&db)
    .await
    .expect("recovered retry job must be queryable");
    assert_eq!(recovered_row.0, "pendiente");
    assert!(recovered_row.1.is_none());
    assert_eq!(
        recovered_row.2,
        agro_ops_backend::jobs::STALE_RECOVERY_ERROR
    );
    assert!(recovered_row.3);
    assert_eq!(
        mark_succeeded(&db, retry_job.id, old_owner)
            .await
            .expect("stale owner completion must execute"),
        OwnershipTransition::OwnershipLost
    );

    let new_owner = Uuid::new_v4();
    let reclaimed = claim(&db, new_owner, std::slice::from_ref(&retry_type), 1)
        .await
        .expect("recovered job must be claimable");
    assert_eq!(reclaimed[0].attempts, 2);
    assert_eq!(
        mark_failed(&db, retry_job.id, old_owner, future(), &safe_error())
            .await
            .expect("old owner failure must execute"),
        OwnershipTransition::OwnershipLost
    );
    assert!(matches!(
        mark_succeeded(&db, retry_job.id, new_owner)
            .await
            .expect("new owner completion must execute"),
        OwnershipTransition::Applied(_)
    ));

    let exhausted_type = unique_type("stale_exhausted");
    let exhausted_job = persist(&db, new_job(exhausted_type.clone(), 1, due())).await;
    claim(
        &db,
        Uuid::new_v4(),
        std::slice::from_ref(&exhausted_type),
        1,
    )
    .await
    .expect("stale exhausted job must claim");
    tokio::time::sleep(Duration::from_millis(10)).await;
    let recovered = recover_stale(&db, Duration::from_millis(1), 100)
        .await
        .expect("stale exhausted job must recover");
    assert!(recovered.exhausted >= 1);
    let state: String = sqlx::query_scalar("SELECT estado FROM jobs WHERE id = $1")
        .bind(exhausted_job.id)
        .fetch_one(&db)
        .await
        .expect("stale exhausted state must be queryable");
    assert_eq!(state, "agotado");
}

#[tokio::test]
async fn stale_recovery_racing_completion_has_exactly_one_valid_winner() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let job_type = unique_type("stale_race");
    let job = persist(&db, new_job(job_type.clone(), 2, due())).await;
    let owner = Uuid::new_v4();
    claim(&db, owner, &[job_type], 1)
        .await
        .expect("race job must claim");
    tokio::time::sleep(Duration::from_millis(10)).await;

    let (completion, recovery) = tokio::join!(
        mark_succeeded(&db, job.id, owner),
        recover_stale(&db, Duration::from_millis(1), 100),
    );
    let completion = completion.expect("racing completion must execute");
    let recovery = recovery.expect("racing recovery must execute");
    let completion_won = matches!(completion, OwnershipTransition::Applied(_));

    let final_row: (String, Option<Uuid>, Option<OffsetDateTime>) =
        sqlx::query_as("SELECT estado, bloqueado_por, completado_en FROM jobs WHERE id = $1")
            .bind(job.id)
            .fetch_one(&db)
            .await
            .expect("race result must be queryable");
    if completion_won {
        assert_eq!(final_row.0, "completado");
        assert!(final_row.2.is_some());
    } else {
        assert!(recovery.pending >= 1);
        assert_eq!(final_row.0, "pendiente");
        assert!(final_row.2.is_none());
    }
    assert!(final_row.1.is_none());
}

#[tokio::test]
async fn database_constraints_and_transition_trigger_reject_invalid_states() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;

    let invalid_lock = sqlx::query(
        "INSERT INTO jobs (tipo, estado, payload, max_intentos, next_attempt_at) VALUES ('test.invalid_lock', 'ejecutando', '{}'::jsonb, 1, statement_timestamp())",
    )
    .execute(&db)
    .await
    .expect_err("running job without ownership must fail");
    assert_eq!(
        invalid_lock
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );

    let invalid_type = sqlx::query(
        "INSERT INTO jobs (tipo, payload, max_intentos, next_attempt_at) VALUES ('arbitrary description', '{}'::jsonb, 1, statement_timestamp())",
    )
    .execute(&db)
    .await
    .expect_err("noncanonical job type must fail");
    assert_eq!(
        invalid_type
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );

    let terminal_type = unique_type("terminal_guard");
    let terminal_job = persist(&db, new_job(terminal_type.clone(), 1, due())).await;
    let owner = Uuid::new_v4();
    claim(&db, owner, &[terminal_type], 1)
        .await
        .expect("terminal guard job must claim");
    mark_succeeded(&db, terminal_job.id, owner)
        .await
        .expect("terminal guard job must complete");
    let invalid_transition = sqlx::query(
        "UPDATE jobs SET estado = 'pendiente', completado_en = NULL, actualizado_en = statement_timestamp() WHERE id = $1",
    )
    .bind(terminal_job.id)
    .execute(&db)
    .await
    .expect_err("completed job must not return to pending");
    let database_error = invalid_transition
        .as_database_error()
        .expect("transition rejection must be a database error");
    assert_eq!(database_error.code().as_deref(), Some("P0001"));
    assert_eq!(database_error.message(), "agro_ops_job_transicion_invalida");

    let search_path: String = sqlx::query_scalar(
        "SELECT array_to_string(proconfig, ',') FROM pg_proc WHERE proname = 'proteger_transiciones_jobs'",
    )
    .fetch_one(&db)
    .await
    .expect("job trigger search path must be queryable");
    assert_eq!(search_path, "search_path=pg_catalog");
}
