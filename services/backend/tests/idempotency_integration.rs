use std::sync::Arc;

use agro_ops_backend::idempotency::{
    IdempotencyDecision, IdempotencyError, IdempotencyKey, IdempotencyRequest, OperationCode,
    RequestFingerprint, SafeIdempotencyResult, begin, complete,
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::sync::Barrier;
use uuid::Uuid;

async fn test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for PostgreSQL tests");
    PgPoolOptions::new()
        .max_connections(12)
        .connect(&database_url)
        .await
        .expect("PostgreSQL must be available with migrations applied")
}

async fn organization(db: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO organizaciones (nombre) VALUES ($1) RETURNING id")
        .bind(format!("Organización idempotencia {}", Uuid::new_v4()))
        .fetch_one(db)
        .await
        .expect("test organization must insert")
}

fn request(
    organization_id: Option<Uuid>,
    operation: &str,
    key: impl Into<String>,
    semantic_input: &[u8],
) -> IdempotencyRequest {
    IdempotencyRequest {
        organization_id,
        operation: OperationCode::new(operation).expect("operation must be canonical"),
        key: IdempotencyKey::new(key).expect("key must be valid"),
        request_fingerprint: RequestFingerprint::sha256(semantic_input),
    }
}

fn safe_result(value: Value) -> SafeIdempotencyResult {
    SafeIdempotencyResult::new(value).expect("result must be safe and bounded")
}

async fn record_count(
    db: &PgPool,
    organization_id: Option<Uuid>,
    operation: &str,
    key: &str,
) -> i64 {
    sqlx::query_scalar(
        r#"
        SELECT COUNT(*)::bigint
        FROM idempotency_records
        WHERE organizacion_id IS NOT DISTINCT FROM $1
          AND operacion = $2
          AND idempotency_key = $3
        "#,
    )
    .bind(organization_id)
    .bind(operation)
    .bind(key)
    .fetch_one(db)
    .await
    .expect("record count must load")
}

#[tokio::test]
async fn begin_complete_replay_and_conflict_follow_the_stored_fingerprint() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let key = format!("command-{}", Uuid::new_v4());
    let original = request(
        Some(organization_id),
        "test.registrar_consecuencia",
        &key,
        br#"{"cantidad":1}"#,
    );
    let expected_result = json!({"consecuencia_id": Uuid::new_v4()});

    let mut transaction = db.begin().await.expect("transaction must begin");
    let pending = match begin(&mut transaction, original.clone())
        .await
        .expect("first begin must succeed")
    {
        IdempotencyDecision::Proceed(pending) => pending,
        IdempotencyDecision::Replay(_) => panic!("first execution must proceed"),
    };
    let completed = complete(
        &mut transaction,
        pending,
        safe_result(expected_result.clone()),
    )
    .await
    .expect("completion must persist");
    assert_eq!(completed.result.as_value(), &expected_result);
    transaction.commit().await.expect("transaction must commit");

    let mut replay_transaction = db.begin().await.expect("transaction must begin");
    let replayed = match begin(&mut replay_transaction, original.clone())
        .await
        .expect("identical retry must resolve")
    {
        IdempotencyDecision::Replay(stored) => stored,
        IdempotencyDecision::Proceed(_) => panic!("identical retry must replay"),
    };
    assert_eq!(replayed.record_id, completed.record_id);
    assert_eq!(replayed.result.as_value(), &expected_result);
    replay_transaction
        .commit()
        .await
        .expect("replay transaction must commit without changes");

    let incompatible = request(
        Some(organization_id),
        "test.registrar_consecuencia",
        &key,
        br#"{"cantidad":2}"#,
    );
    let mut conflict_transaction = db.begin().await.expect("transaction must begin");
    assert!(matches!(
        begin(&mut conflict_transaction, incompatible).await,
        Err(IdempotencyError::Conflict(_))
    ));
    let still_usable: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&mut *conflict_transaction)
        .await
        .expect("normal conflict resolution must not abort the transaction");
    assert_eq!(still_usable, 1);
    conflict_transaction
        .rollback()
        .await
        .expect("conflict transaction must roll back");
}

#[tokio::test]
async fn operation_organization_and_global_scopes_are_independent_and_null_safe() {
    let db = test_pool().await;
    let first_organization = organization(&db).await;
    let second_organization = organization(&db).await;
    let key = format!("scope-{}", Uuid::new_v4());

    for scoped_request in [
        request(
            Some(first_organization),
            "test.operacion_uno",
            &key,
            b"same-input",
        ),
        request(
            Some(first_organization),
            "test.operacion_dos",
            &key,
            b"same-input",
        ),
        request(
            Some(second_organization),
            "test.operacion_uno",
            &key,
            b"same-input",
        ),
    ] {
        let mut transaction = db.begin().await.expect("transaction must begin");
        let pending = match begin(&mut transaction, scoped_request)
            .await
            .expect("independent scope must resolve")
        {
            IdempotencyDecision::Proceed(pending) => pending,
            IdempotencyDecision::Replay(_) => panic!("independent scope must proceed"),
        };
        complete(&mut transaction, pending, safe_result(json!({"ok": true})))
            .await
            .expect("independent completion must persist");
        transaction.commit().await.expect("transaction must commit");
    }

    let global = request(None, "test.operacion_global", &key, b"global-input");
    let mut first_global_transaction = db.begin().await.expect("transaction must begin");
    let pending = match begin(&mut first_global_transaction, global.clone())
        .await
        .expect("global begin must resolve")
    {
        IdempotencyDecision::Proceed(pending) => pending,
        IdempotencyDecision::Replay(_) => panic!("first global execution must proceed"),
    };
    let completed = complete(
        &mut first_global_transaction,
        pending,
        safe_result(json!({"global": true})),
    )
    .await
    .expect("global completion must persist");
    first_global_transaction
        .commit()
        .await
        .expect("global transaction must commit");

    let mut replay_transaction = db.begin().await.expect("transaction must begin");
    let replayed = match begin(&mut replay_transaction, global)
        .await
        .expect("global retry must resolve")
    {
        IdempotencyDecision::Replay(stored) => stored,
        IdempotencyDecision::Proceed(_) => panic!("global retry must replay"),
    };
    assert_eq!(replayed.record_id, completed.record_id);
    replay_transaction
        .rollback()
        .await
        .expect("replay transaction must release its lock");
    assert_eq!(
        record_count(&db, None, "test.operacion_global", &key).await,
        1
    );
}

#[tokio::test]
async fn rollback_removes_business_effect_and_completion_then_allows_retry() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let key = format!("rollback-{}", Uuid::new_v4());
    let marker = format!("idempotency rollback {}", Uuid::new_v4());
    let command = request(
        Some(organization_id),
        "test.crear_rol",
        &key,
        b"create-one-role",
    );

    let mut transaction = db.begin().await.expect("transaction must begin");
    let pending = match begin(&mut transaction, command.clone())
        .await
        .expect("first execution must resolve")
    {
        IdempotencyDecision::Proceed(pending) => pending,
        IdempotencyDecision::Replay(_) => panic!("first execution must proceed"),
    };
    let rolled_back_role: Uuid = sqlx::query_scalar(
        "INSERT INTO roles (organizacion_id, nombre, descripcion) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(organization_id)
    .bind(format!("Rol rollback {}", Uuid::new_v4()))
    .bind(&marker)
    .fetch_one(&mut *transaction)
    .await
    .expect("test business effect must insert");
    complete(
        &mut transaction,
        pending,
        safe_result(json!({"rol_id": rolled_back_role})),
    )
    .await
    .expect("completion must exist inside transaction");
    transaction
        .rollback()
        .await
        .expect("transaction must roll back");

    let effects_after_rollback: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM roles WHERE descripcion = $1")
            .bind(&marker)
            .fetch_one(&db)
            .await
            .expect("business effect count must load");
    assert_eq!(effects_after_rollback, 0);
    assert_eq!(
        record_count(&db, Some(organization_id), "test.crear_rol", &key).await,
        0
    );

    let mut retry_transaction = db.begin().await.expect("transaction must begin");
    let pending = match begin(&mut retry_transaction, command)
        .await
        .expect("retry must resolve")
    {
        IdempotencyDecision::Proceed(pending) => pending,
        IdempotencyDecision::Replay(_) => panic!("retry after rollback must proceed"),
    };
    let committed_role: Uuid = sqlx::query_scalar(
        "INSERT INTO roles (organizacion_id, nombre, descripcion) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(organization_id)
    .bind(format!("Rol retry {}", Uuid::new_v4()))
    .bind(&marker)
    .fetch_one(&mut *retry_transaction)
    .await
    .expect("retried business effect must insert");
    complete(
        &mut retry_transaction,
        pending,
        safe_result(json!({"rol_id": committed_role})),
    )
    .await
    .expect("retried completion must persist");
    retry_transaction
        .commit()
        .await
        .expect("retry transaction must commit");
    assert_eq!(
        record_count(&db, Some(organization_id), "test.crear_rol", &key).await,
        1
    );
}

#[derive(Debug, PartialEq, Eq)]
enum ConcurrentOutcome {
    Executed(Uuid),
    Replayed(Uuid),
    Conflict,
}

async fn execute_concurrent_command(
    db: PgPool,
    organization_id: Uuid,
    command: IdempotencyRequest,
    marker: String,
    barrier: Arc<Barrier>,
) -> ConcurrentOutcome {
    let mut transaction = db.begin().await.expect("transaction must begin");
    barrier.wait().await;
    match begin(&mut transaction, command).await {
        Ok(IdempotencyDecision::Proceed(pending)) => {
            let role_id: Uuid = sqlx::query_scalar(
                "INSERT INTO roles (organizacion_id, nombre, descripcion) VALUES ($1, $2, $3) RETURNING id",
            )
            .bind(organization_id)
            .bind(format!("Rol concurrente {}", Uuid::new_v4()))
            .bind(&marker)
            .fetch_one(&mut *transaction)
            .await
            .expect("test business effect must insert");
            complete(
                &mut transaction,
                pending,
                safe_result(json!({"rol_id": role_id})),
            )
            .await
            .expect("completion must persist");
            transaction.commit().await.expect("transaction must commit");
            ConcurrentOutcome::Executed(role_id)
        }
        Ok(IdempotencyDecision::Replay(stored)) => {
            let role_id = stored
                .result
                .as_value()
                .get("rol_id")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
                .expect("stored result must contain the role id");
            transaction
                .commit()
                .await
                .expect("replay must commit without changes");
            ConcurrentOutcome::Replayed(role_id)
        }
        Err(IdempotencyError::Conflict(_)) => {
            let still_usable: i32 = sqlx::query_scalar("SELECT 1")
                .fetch_one(&mut *transaction)
                .await
                .expect("typed conflict must not abort the transaction");
            assert_eq!(still_usable, 1);
            transaction
                .rollback()
                .await
                .expect("transaction must roll back");
            ConcurrentOutcome::Conflict
        }
        Err(error) => panic!("unexpected idempotency error: {error}"),
    }
}

#[tokio::test]
async fn concurrent_identical_requests_execute_one_effect_and_replay_one_result() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let key = format!("concurrent-same-{}", Uuid::new_v4());
    let command = request(
        Some(organization_id),
        "test.comando_concurrente",
        &key,
        b"same-semantic-input",
    );
    let marker = format!("idempotency identical {}", Uuid::new_v4());
    let barrier = Arc::new(Barrier::new(2));

    let first = tokio::spawn(execute_concurrent_command(
        db.clone(),
        organization_id,
        command.clone(),
        marker.clone(),
        barrier.clone(),
    ));
    let second = tokio::spawn(execute_concurrent_command(
        db.clone(),
        organization_id,
        command,
        marker.clone(),
        barrier,
    ));
    let (first, second) = tokio::join!(first, second);
    let outcomes = [
        first.expect("first task must finish"),
        second.expect("second task must finish"),
    ];

    let executed = outcomes
        .iter()
        .find_map(|outcome| match outcome {
            ConcurrentOutcome::Executed(id) => Some(*id),
            _ => None,
        })
        .expect("one caller must execute");
    let replayed = outcomes
        .iter()
        .find_map(|outcome| match outcome {
            ConcurrentOutcome::Replayed(id) => Some(*id),
            _ => None,
        })
        .expect("one caller must replay");
    assert_eq!(executed, replayed);
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ConcurrentOutcome::Executed(_)))
            .count(),
        1
    );
    let effects: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM roles WHERE descripcion = $1")
            .bind(&marker)
            .fetch_one(&db)
            .await
            .expect("business effect count must load");
    assert_eq!(effects, 1);
    assert_eq!(
        record_count(&db, Some(organization_id), "test.comando_concurrente", &key).await,
        1
    );
}

#[tokio::test]
async fn concurrent_conflicting_requests_commit_one_effect_and_return_one_conflict() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let key = format!("concurrent-conflict-{}", Uuid::new_v4());
    let first_command = request(
        Some(organization_id),
        "test.comando_conflictivo",
        &key,
        b"semantic-input-a",
    );
    let second_command = request(
        Some(organization_id),
        "test.comando_conflictivo",
        &key,
        b"semantic-input-b",
    );
    let marker = format!("idempotency conflicting {}", Uuid::new_v4());
    let barrier = Arc::new(Barrier::new(2));

    let first = tokio::spawn(execute_concurrent_command(
        db.clone(),
        organization_id,
        first_command,
        marker.clone(),
        barrier.clone(),
    ));
    let second = tokio::spawn(execute_concurrent_command(
        db.clone(),
        organization_id,
        second_command,
        marker.clone(),
        barrier,
    ));
    let (first, second) = tokio::join!(first, second);
    let outcomes = [
        first.expect("first task must finish"),
        second.expect("second task must finish"),
    ];
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ConcurrentOutcome::Executed(_)))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, ConcurrentOutcome::Conflict))
            .count(),
        1
    );
    let effects: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM roles WHERE descripcion = $1")
            .bind(&marker)
            .fetch_one(&db)
            .await
            .expect("business effect count must load");
    assert_eq!(effects, 1);
    assert_eq!(
        record_count(&db, Some(organization_id), "test.comando_conflictivo", &key).await,
        1
    );
}

#[tokio::test]
async fn database_constraints_immutability_and_restrictive_history_are_enforced() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let valid_key = format!("constraints-{}", Uuid::new_v4());
    let valid_hash = vec![7_u8; 32];

    let invalid_rows = [
        (
            "Not.Canonical",
            valid_key.as_str(),
            valid_hash.clone(),
            json!({"ok": true}),
        ),
        ("test.valid", "   ", valid_hash.clone(), json!({"ok": true})),
        (
            "test.valid",
            valid_key.as_str(),
            vec![7_u8; 31],
            json!({"ok": true}),
        ),
        (
            "test.valid",
            valid_key.as_str(),
            valid_hash.clone(),
            json!({"value": "x".repeat(33_000)}),
        ),
    ];
    for (operation, key, fingerprint, result) in invalid_rows {
        let insert = sqlx::query(
            r#"
            INSERT INTO idempotency_records (
                organizacion_id, operacion, idempotency_key, request_sha256, resultado
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(organization_id)
        .bind(operation)
        .bind(key)
        .bind(fingerprint)
        .bind(result)
        .execute(&db)
        .await;
        assert!(insert.is_err(), "invalid completed record must be rejected");
    }
    assert!(SafeIdempotencyResult::new(json!({"value": "x".repeat(33_000)})).is_err());

    let command = request(
        Some(organization_id),
        "test.proteger_historia",
        &valid_key,
        b"immutable",
    );
    let mut transaction = db.begin().await.expect("transaction must begin");
    let pending = match begin(&mut transaction, command)
        .await
        .expect("begin must resolve")
    {
        IdempotencyDecision::Proceed(pending) => pending,
        IdempotencyDecision::Replay(_) => panic!("new command must proceed"),
    };
    let completed = complete(&mut transaction, pending, safe_result(json!({"ok": true})))
        .await
        .expect("completion must persist");
    transaction.commit().await.expect("transaction must commit");

    assert!(
        sqlx::query("UPDATE idempotency_records SET resultado = '{}'::jsonb WHERE id = $1")
            .bind(completed.record_id)
            .execute(&db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM idempotency_records WHERE id = $1")
            .bind(completed.record_id)
            .execute(&db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("TRUNCATE idempotency_records")
            .execute(&db)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM organizaciones WHERE id = $1")
            .bind(organization_id)
            .execute(&db)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn advisory_lock_is_released_on_rollback_and_commit() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let key = format!("lock-release-{}", Uuid::new_v4());
    let command = request(
        Some(organization_id),
        "test.liberar_lock",
        &key,
        b"lock-input",
    );

    let mut owner = db.begin().await.expect("owner transaction must begin");
    assert!(matches!(
        begin(&mut owner, command.clone()).await,
        Ok(IdempotencyDecision::Proceed(_))
    ));

    let blocked_db = db.clone();
    let blocked_command = command.clone();
    let blocked = tokio::spawn(async move {
        let mut transaction = blocked_db.begin().await.expect("waiter must begin");
        let decision = begin(&mut transaction, blocked_command)
            .await
            .expect("waiter must eventually resolve");
        transaction.rollback().await.expect("waiter must roll back");
        matches!(decision, IdempotencyDecision::Proceed(_))
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !blocked.is_finished(),
        "waiter must block on the database lock"
    );
    owner
        .rollback()
        .await
        .expect("owner rollback must release lock");
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), blocked)
            .await
            .expect("waiter must unblock after rollback")
            .expect("waiter task must finish")
    );

    let mut committed_owner = db.begin().await.expect("owner transaction must begin");
    let pending = match begin(&mut committed_owner, command.clone())
        .await
        .expect("command must resolve")
    {
        IdempotencyDecision::Proceed(pending) => pending,
        IdempotencyDecision::Replay(_) => panic!("command has not completed yet"),
    };
    complete(
        &mut committed_owner,
        pending,
        safe_result(json!({"committed": true})),
    )
    .await
    .expect("completion must persist");

    let replay_db = db.clone();
    let replay_command = command;
    let replay = tokio::spawn(async move {
        let mut transaction = replay_db.begin().await.expect("waiter must begin");
        let decision = begin(&mut transaction, replay_command)
            .await
            .expect("waiter must eventually resolve");
        transaction.commit().await.expect("waiter must commit");
        matches!(decision, IdempotencyDecision::Replay(_))
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !replay.is_finished(),
        "waiter must block until owner commits"
    );
    committed_owner
        .commit()
        .await
        .expect("owner commit must release lock");
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(2), replay)
            .await
            .expect("waiter must unblock after commit")
            .expect("waiter task must finish")
    );
}
