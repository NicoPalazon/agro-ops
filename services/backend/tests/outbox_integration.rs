use agro_ops_backend::outbox::{
    IdempotencyKey, NewOutboxEvent, OutboxDestination, OutboxEntityType, OutboxEventType,
    OutboxStoreError, record,
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use time::OffsetDateTime;
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

async fn organization(db: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO organizaciones (nombre) VALUES ($1) RETURNING id")
        .bind(format!("Organización outbox {}", Uuid::new_v4()))
        .fetch_one(db)
        .await
        .expect("test organization must insert")
}

fn event(
    organization_id: Option<Uuid>,
    destination: &str,
    key: &str,
    event_type: &str,
    payload: Value,
) -> NewOutboxEvent {
    NewOutboxEvent {
        organization_id,
        destination: OutboxDestination::new(destination).expect("destination must be canonical"),
        event_type: OutboxEventType::new(event_type).expect("event type must be canonical"),
        entity_type: Some(
            OutboxEntityType::new("test.entity").expect("entity type must be canonical"),
        ),
        entity_id: Some(Uuid::new_v4()),
        reference: Some("REF-2026-0001".to_owned()),
        idempotency_key: IdempotencyKey::new(key).expect("idempotency key must be valid"),
        payload,
        delivery_max_attempts: 3,
        delivery_next_attempt_at: OffsetDateTime::now_utc(),
    }
}

async fn commit_record(db: &PgPool, event: NewOutboxEvent) -> (Uuid, Uuid) {
    let mut transaction = db.begin().await.expect("transaction must begin");
    let recorded = record(&mut transaction, event)
        .await
        .expect("outbox event must record");
    transaction.commit().await.expect("transaction must commit");
    (recorded.event_id, recorded.job_id)
}

async fn counts(db: &PgPool, event_id: Uuid, job_id: Uuid) -> (i64, i64, i64) {
    let events = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM outbox_events WHERE id = $1")
        .bind(event_id)
        .fetch_one(db)
        .await
        .expect("event count must load");
    let jobs = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(db)
        .await
        .expect("job count must load");
    let links = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM outbox_job_links WHERE outbox_event_id = $1 AND job_id = $2",
    )
    .bind(event_id)
    .bind(job_id)
    .fetch_one(db)
    .await
    .expect("link count must load");
    (events, jobs, links)
}

#[tokio::test]
async fn record_creates_one_event_job_and_link_in_the_caller_transaction() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let key = format!("record-{}", Uuid::new_v4());
    let input = event(
        Some(organization_id),
        "test_adapter",
        &key,
        "test.evento_creado",
        json!({"campo": "valor"}),
    );
    let mut transaction = db.begin().await.expect("transaction must begin");
    let recorded = record(&mut transaction, input)
        .await
        .expect("record must use caller transaction");

    let transaction_counts: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*) FROM outbox_events WHERE id = $1),
            (SELECT COUNT(*) FROM jobs WHERE id = $2),
            (SELECT COUNT(*) FROM outbox_job_links WHERE outbox_event_id = $1 AND job_id = $2)
        "#,
    )
    .bind(recorded.event_id)
    .bind(recorded.job_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("transactional rows must be visible to their caller");
    assert_eq!(transaction_counts, (1, 1, 1));
    let job: (String, Value) = sqlx::query_as("SELECT tipo, payload FROM jobs WHERE id = $1")
        .bind(recorded.job_id)
        .fetch_one(&mut *transaction)
        .await
        .expect("linked job must load");
    assert_eq!(job.0, "outbox.dispatch.test_adapter");
    assert_eq!(job.1, json!({}));

    transaction.rollback().await.expect("rollback must succeed");
    assert_eq!(
        counts(&db, recorded.event_id, recorded.job_id).await,
        (0, 0, 0)
    );
}

#[tokio::test]
async fn identical_retries_reuse_one_event_job_and_link_while_conflicts_are_typed() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let key = format!("retry-{}", Uuid::new_v4());
    let original = event(
        Some(organization_id),
        "test_adapter",
        &key,
        "test.evento_creado",
        json!({"a": 1, "b": 2}),
    );
    let expected_entity_id = original.entity_id;
    let (event_id, job_id) = commit_record(&db, original.clone()).await;

    for _ in 0..5 {
        let recorded = commit_record(&db, original.clone()).await;
        assert_eq!(recorded, (event_id, job_id));
    }
    assert_eq!(counts(&db, event_id, job_id).await, (1, 1, 1));
    let totals: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*) FROM outbox_events WHERE organizacion_id = $1 AND destino = 'test_adapter' AND idempotency_key = $2),
            (SELECT COUNT(*) FROM jobs WHERE id = $3),
            (SELECT COUNT(*) FROM outbox_job_links WHERE outbox_event_id = $4)
        "#,
    )
    .bind(organization_id)
    .bind(&key)
    .bind(job_id)
    .bind(event_id)
    .fetch_one(&db)
    .await
    .expect("idempotency totals must load");
    assert_eq!(totals, (1, 1, 1));

    let mut different_payload = event(
        Some(organization_id),
        "test_adapter",
        &key,
        "test.evento_creado",
        json!({"a": 999}),
    );
    different_payload.entity_id = expected_entity_id;
    let mut transaction = db.begin().await.expect("transaction must begin");
    assert!(matches!(
        record(&mut transaction, different_payload).await,
        Err(OutboxStoreError::IdempotencyConflict)
    ));
    transaction.rollback().await.expect("rollback must succeed");

    let mut different_type = event(
        Some(organization_id),
        "test_adapter",
        &key,
        "test.otro_evento",
        json!({"a": 1, "b": 2}),
    );
    different_type.entity_id = expected_entity_id;
    let mut transaction = db.begin().await.expect("transaction must begin");
    assert!(matches!(
        record(&mut transaction, different_type).await,
        Err(OutboxStoreError::IdempotencyConflict)
    ));
    transaction.rollback().await.expect("rollback must succeed");
    assert_eq!(counts(&db, event_id, job_id).await, (1, 1, 1));
}

#[tokio::test]
async fn idempotency_is_isolated_by_organization_and_enforced_for_global_events() {
    let db = test_pool().await;
    let first_organization = organization(&db).await;
    let second_organization = organization(&db).await;
    let key = format!("scope-{}", Uuid::new_v4());
    let first = event(
        Some(first_organization),
        "scope_adapter",
        &key,
        "test.scope_event",
        json!({"scope": 1}),
    );
    let mut second = first.clone();
    second.organization_id = Some(second_organization);
    let first_recorded = commit_record(&db, first).await;
    let second_recorded = commit_record(&db, second).await;
    assert_ne!(first_recorded, second_recorded);

    let global_key = format!("global-{}", Uuid::new_v4());
    let global = event(
        None,
        "scope_adapter",
        &global_key,
        "test.global_event",
        json!({"global": true}),
    );
    let global_entity = global.entity_id;
    let first_global = commit_record(&db, global.clone()).await;
    assert_eq!(commit_record(&db, global).await, first_global);

    let mut conflict = event(
        None,
        "scope_adapter",
        &global_key,
        "test.global_event",
        json!({"global": false}),
    );
    conflict.entity_id = global_entity;
    let mut transaction = db.begin().await.expect("transaction must begin");
    assert!(matches!(
        record(&mut transaction, conflict).await,
        Err(OutboxStoreError::IdempotencyConflict)
    ));
    transaction.rollback().await.expect("rollback must succeed");
    assert_eq!(counts(&db, first_global.0, first_global.1).await, (1, 1, 1));
}

#[tokio::test]
async fn concurrent_identical_records_resolve_to_one_database_coordinated_effect() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let key = format!("concurrent-{}", Uuid::new_v4());
    let input = event(
        Some(organization_id),
        "concurrent_adapter",
        &key,
        "test.concurrent_event",
        json!({"same": true}),
    );
    let first_db = db.clone();
    let second_db = db.clone();
    let first_input = input.clone();
    let first = tokio::spawn(async move { commit_record(&first_db, first_input).await });
    let second = tokio::spawn(async move { commit_record(&second_db, input).await });
    let first = first.await.expect("first caller must complete");
    let second = second.await.expect("second caller must complete");

    assert_eq!(first, second);
    assert_eq!(counts(&db, first.0, first.1).await, (1, 1, 1));
}

#[tokio::test]
async fn concurrent_conflicting_records_leave_one_effect_and_one_typed_conflict() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let key = format!("concurrent-conflict-{}", Uuid::new_v4());
    let first = event(
        Some(organization_id),
        "race_adapter",
        &key,
        "test.concurrent_event",
        json!({"winner": 1}),
    );
    let mut second = first.clone();
    second.payload = json!({"winner": 2});
    let first_db = db.clone();
    let second_db = db.clone();
    let first = tokio::spawn(async move {
        let mut transaction = first_db.begin().await.expect("transaction must begin");
        let result = record(&mut transaction, first).await;
        match &result {
            Ok(_) => transaction.commit().await.expect("winner must commit"),
            Err(_) => transaction.rollback().await.expect("loser must roll back"),
        }
        result
    });
    let second = tokio::spawn(async move {
        let mut transaction = second_db.begin().await.expect("transaction must begin");
        let result = record(&mut transaction, second).await;
        match &result {
            Ok(_) => transaction.commit().await.expect("winner must commit"),
            Err(_) => transaction.rollback().await.expect("loser must roll back"),
        }
        result
    });
    let results = [
        first.await.expect("first caller must finish"),
        second.await.expect("second caller must finish"),
    ];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(OutboxStoreError::IdempotencyConflict)))
            .count(),
        1
    );
    let recorded = results
        .into_iter()
        .find_map(Result::ok)
        .expect("one winner");
    assert_eq!(
        counts(&db, recorded.event_id, recorded.job_id).await,
        (1, 1, 1)
    );
}

#[tokio::test]
async fn database_rejects_outbox_mutation_truncation_invalid_values_and_organization_deletion() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let key = format!("immutable-{}", Uuid::new_v4());
    let recorded = commit_record(
        &db,
        event(
            Some(organization_id),
            "immutable_adapter",
            &key,
            "test.immutable_event",
            json!({"immutable": true}),
        ),
    )
    .await;

    for statement in [
        "UPDATE outbox_events SET payload = '{}'::jsonb WHERE id = $1",
        "DELETE FROM outbox_events WHERE id = $1",
        "UPDATE outbox_job_links SET creado_en = statement_timestamp() WHERE outbox_event_id = $1",
        "DELETE FROM outbox_job_links WHERE outbox_event_id = $1",
    ] {
        let error = sqlx::query(statement)
            .bind(recorded.0)
            .execute(&db)
            .await
            .expect_err("immutable outbox history mutation must fail");
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("P0001")
        );
    }
    for statement in [
        "TRUNCATE TABLE outbox_events CASCADE",
        "TRUNCATE TABLE outbox_job_links",
    ] {
        let error = sqlx::query(statement)
            .execute(&db)
            .await
            .expect_err("outbox truncation must fail");
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("P0001")
        );
    }

    let delete_organization = sqlx::query("DELETE FROM organizaciones WHERE id = $1")
        .bind(organization_id)
        .execute(&db)
        .await
        .expect_err("organization history FK must be restrictive");
    assert_eq!(
        delete_organization
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503")
    );

    let invalid_cases = [
        ("Bad Destination", "test.valid_event", Some("entity"), "key"),
        ("valid", "Not canonical", Some("entity"), "key"),
        ("valid", "test.valid_event", Some("Bad Entity"), "key"),
        ("valid", "test.valid_event", Some("entity"), " "),
    ];
    for (destination, event_type, entity_type, idempotency_key) in invalid_cases {
        let error = sqlx::query(
            r#"
            INSERT INTO outbox_events
                (destino, evento_tipo, entidad_tipo, referencia, idempotency_key, payload)
            VALUES ($1, $2, $3, 'REF', $4, '{}'::jsonb)
            "#,
        )
        .bind(destination)
        .bind(event_type)
        .bind(entity_type)
        .bind(idempotency_key)
        .execute(&db)
        .await
        .expect_err("invalid canonical or bounded field must fail");
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23514")
        );
    }
    let oversized_reference = "r".repeat(257);
    let oversized_key = "k".repeat(257);
    for (reference, key) in [
        (" ", "valid-key"),
        (oversized_reference.as_str(), "valid-key"),
        ("REF", oversized_key.as_str()),
    ] {
        let result = sqlx::query(
            "INSERT INTO outbox_events (destino, evento_tipo, referencia, idempotency_key, payload) VALUES ('valid', 'test.valid_event', $1, $2, '{}'::jsonb)",
        )
        .bind(reference)
        .bind(key)
        .execute(&db)
        .await;
        assert!(
            result.is_err(),
            "invalid reference/idempotency bound must fail"
        );
    }

    for (destination, event_type, entity_type) in [
        ("d".repeat(65), "test.valid_event".to_owned(), None),
        (
            "valid".to_owned(),
            format!("test.{}", "e".repeat(124)),
            None,
        ),
        (
            "valid".to_owned(),
            "test.valid_event".to_owned(),
            Some("e".repeat(129)),
        ),
    ] {
        let result = sqlx::query(
            "INSERT INTO outbox_events (destino, evento_tipo, entidad_tipo, idempotency_key, payload) VALUES ($1, $2, $3, $4, '{}'::jsonb)",
        )
        .bind(destination)
        .bind(event_type)
        .bind(entity_type)
        .bind(format!("bounded-{}", Uuid::new_v4()))
        .execute(&db)
        .await;
        assert!(
            result.is_err(),
            "bounded canonical code must reject overflow"
        );
    }
}
