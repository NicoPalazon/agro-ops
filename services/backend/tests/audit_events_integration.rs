use std::sync::OnceLock;

use agro_ops_backend::audit::{self, AuditActor, NewAuditEvent};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions, types::Json};
use uuid::Uuid;

const CHECK: &str = "23514";
const RAISE: &str = "P0001";
const ACTOR_ORGANIZATION_MISMATCH: &str = "agro_ops_audit_actor_organizacion_invalida";
const AUDIT_DELETE_FORBIDDEN: &str = "agro_ops_audit_historia_eliminacion_prohibida";
const AUDIT_IMMUTABLE: &str = "agro_ops_audit_historia_inmutable";
const AUDIT_TRUNCATE_FORBIDDEN: &str = "agro_ops_audit_historia_truncate_prohibido";

type AuditSnapshotRow = (
    String,
    Option<Uuid>,
    Option<Json<Value>>,
    Option<Json<Value>>,
);

struct EventInput<'a> {
    organization_id: Uuid,
    actor_type: &'a str,
    actor_user_id: Option<Uuid>,
    action: &'a str,
    entity_type: &'a str,
    entity_id: Option<Uuid>,
    reference: Option<&'a str>,
    before_state: Option<Value>,
    after_state: Option<Value>,
}

fn database_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn assert_database_error(error: sqlx::Error, sqlstate: &str, message: Option<&str>) {
    let database_error = error
        .as_database_error()
        .expect("expected a PostgreSQL database error");
    assert_eq!(database_error.code().as_deref(), Some(sqlstate));
    if let Some(message) = message {
        assert_eq!(database_error.message(), message);
    }
}

macro_rules! assert_statement_fails {
    ($tx:ident, $operation:expr, $sqlstate:expr, $message:expr) => {{
        sqlx::query("SAVEPOINT expected_failure")
            .execute(&mut *$tx)
            .await
            .expect("savepoint must be created");
        let error = $operation
            .await
            .expect_err("database statement unexpectedly succeeded");
        assert_database_error(error, $sqlstate, $message);
        sqlx::query("ROLLBACK TO SAVEPOINT expected_failure")
            .execute(&mut *$tx)
            .await
            .expect("failed statement must be rolled back");
        sqlx::query("RELEASE SAVEPOINT expected_failure")
            .execute(&mut *$tx)
            .await
            .expect("savepoint must be released");
    }};
}

async fn test_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
        .await
        .expect("PostgreSQL with migrations must be available")
}

async fn insert_organization(transaction: &mut Transaction<'_, Postgres>) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO public.organizaciones (id, nombre) VALUES ($1, $2)")
        .bind(id)
        .bind(format!("Organización de auditoría {id}"))
        .execute(&mut **transaction)
        .await
        .expect("organization fixture must insert");
    id
}

async fn insert_user(transaction: &mut Transaction<'_, Postgres>, organization_id: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO public.usuarios (id, organizacion_id, nombre_completo) VALUES ($1, $2, $3)",
    )
    .bind(id)
    .bind(organization_id)
    .bind(format!("Usuario de auditoría {id}"))
    .execute(&mut **transaction)
    .await
    .expect("user fixture must insert");
    id
}

async fn insert_event(
    transaction: &mut Transaction<'_, Postgres>,
    event: EventInput<'_>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        INSERT INTO public.audit_events (
            organizacion_id, actor_tipo, actor_usuario_id, accion, entidad_tipo,
            entidad_id, referencia, estado_anterior, estado_posterior
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id
        "#,
    )
    .bind(event.organization_id)
    .bind(event.actor_type)
    .bind(event.actor_user_id)
    .bind(event.action)
    .bind(event.entity_type)
    .bind(event.entity_id)
    .bind(event.reference)
    .bind(event.before_state.map(Json))
    .bind(event.after_state.map(Json))
    .fetch_one(&mut **transaction)
    .await
}

#[tokio::test]
async fn inserts_user_and_system_audit_events_with_exact_json_snapshots() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let mut transaction = db.begin().await.expect("transaction must begin");
    let organization_id = insert_organization(&mut transaction).await;
    let user_id = insert_user(&mut transaction, organization_id).await;
    let entity_id = Uuid::new_v4();
    let before_state = json!({"activo": true, "roles": ["operador"]});
    let after_state = json!({"activo": false, "roles": ["operador"]});

    let user_event_id = insert_event(
        &mut transaction,
        EventInput {
            organization_id,
            actor_type: "usuario",
            actor_user_id: Some(user_id),
            action: "usuario.deshabilitado",
            entity_type: "usuario",
            entity_id: Some(entity_id),
            reference: None,
            before_state: Some(before_state.clone()),
            after_state: Some(after_state.clone()),
        },
    )
    .await
    .expect("user audit event must insert");
    let system_event_id = insert_event(
        &mut transaction,
        EventInput {
            organization_id,
            actor_type: "sistema",
            actor_user_id: None,
            action: "sistema.reconciliado",
            entity_type: "sistema",
            entity_id: None,
            reference: Some("reconciliacion:nocturna"),
            before_state: None,
            after_state: Some(json!({"resultado": "completo"})),
        },
    )
    .await
    .expect("system audit event must insert");

    let row: AuditSnapshotRow = sqlx::query_as(
        "SELECT actor_tipo, actor_usuario_id, estado_anterior, estado_posterior FROM public.audit_events WHERE id = $1",
    )
    .bind(user_event_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("user audit event must be readable");
    assert_eq!(row.0, "usuario");
    assert_eq!(row.1, Some(user_id));
    assert_eq!(row.2.map(|value| value.0), Some(before_state));
    assert_eq!(row.3.map(|value| value.0), Some(after_state));
    let system_actor: (String, Option<Uuid>) = sqlx::query_as(
        "SELECT actor_tipo, actor_usuario_id FROM public.audit_events WHERE id = $1",
    )
    .bind(system_event_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("system audit event must be readable");
    assert_eq!(system_actor, ("sistema".to_owned(), None));

    transaction
        .rollback()
        .await
        .expect("fixture must roll back");
}

#[tokio::test]
async fn rejects_ambiguous_or_cross_organization_audit_actors() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let mut transaction = db.begin().await.expect("transaction must begin");
    let first_organization_id = insert_organization(&mut transaction).await;
    let second_organization_id = insert_organization(&mut transaction).await;
    let first_user_id = insert_user(&mut transaction, first_organization_id).await;

    assert_statement_fails!(
        transaction,
        insert_event(
            &mut transaction,
            EventInput {
                organization_id: first_organization_id,
                actor_type: "usuario",
                actor_user_id: None,
                action: "usuario.creado",
                entity_type: "usuario",
                entity_id: None,
                reference: None,
                before_state: None,
                after_state: None,
            },
        ),
        CHECK,
        None
    );
    assert_statement_fails!(
        transaction,
        insert_event(
            &mut transaction,
            EventInput {
                organization_id: first_organization_id,
                actor_type: "sistema",
                actor_user_id: Some(first_user_id),
                action: "sistema.reconciliado",
                entity_type: "sistema",
                entity_id: None,
                reference: None,
                before_state: None,
                after_state: None,
            },
        ),
        CHECK,
        None
    );
    assert_statement_fails!(
        transaction,
        insert_event(
            &mut transaction,
            EventInput {
                organization_id: second_organization_id,
                actor_type: "usuario",
                actor_user_id: Some(first_user_id),
                action: "usuario.creado",
                entity_type: "usuario",
                entity_id: None,
                reference: None,
                before_state: None,
                after_state: None,
            },
        ),
        RAISE,
        Some(ACTOR_ORGANIZATION_MISMATCH)
    );

    transaction
        .rollback()
        .await
        .expect("fixture must roll back");
}

#[tokio::test]
async fn preserves_multiple_independent_immutable_audit_facts() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let mut transaction = db.begin().await.expect("transaction must begin");
    let organization_id = insert_organization(&mut transaction).await;
    let first_event_id = insert_event(
        &mut transaction,
        EventInput {
            organization_id,
            actor_type: "sistema",
            actor_user_id: None,
            action: "sistema.iniciado",
            entity_type: "proceso",
            entity_id: None,
            reference: Some("primero"),
            before_state: None,
            after_state: Some(json!({"numero": 1})),
        },
    )
    .await
    .expect("first audit fact must insert");
    let second_event_id = insert_event(
        &mut transaction,
        EventInput {
            organization_id,
            actor_type: "sistema",
            actor_user_id: None,
            action: "sistema.finalizado",
            entity_type: "proceso",
            entity_id: None,
            reference: Some("segundo"),
            before_state: None,
            after_state: Some(json!({"numero": 2})),
        },
    )
    .await
    .expect("second audit fact must insert");

    assert_statement_fails!(
        transaction,
        sqlx::query("UPDATE public.audit_events SET referencia = 'alterada' WHERE id = $1")
            .bind(first_event_id)
            .execute(&mut *transaction),
        RAISE,
        Some(AUDIT_IMMUTABLE)
    );
    assert_statement_fails!(
        transaction,
        sqlx::query("DELETE FROM public.audit_events WHERE id = $1")
            .bind(first_event_id)
            .execute(&mut *transaction),
        RAISE,
        Some(AUDIT_DELETE_FORBIDDEN)
    );
    assert_statement_fails!(
        transaction,
        sqlx::query("TRUNCATE public.audit_events").execute(&mut *transaction),
        RAISE,
        Some(AUDIT_TRUNCATE_FORBIDDEN)
    );

    let facts: Vec<(Uuid, Option<String>, Option<Json<Value>>)> = sqlx::query_as(
        "SELECT id, referencia, estado_posterior FROM public.audit_events WHERE id IN ($1, $2) ORDER BY referencia",
    )
    .bind(first_event_id)
    .bind(second_event_id)
    .fetch_all(&mut *transaction)
    .await
    .expect("immutable audit facts must remain queryable");
    assert_eq!(facts.len(), 2);
    assert_eq!(
        facts[0],
        (
            first_event_id,
            Some("primero".to_owned()),
            Some(Json(json!({"numero": 1})))
        )
    );
    assert_eq!(
        facts[1],
        (
            second_event_id,
            Some("segundo".to_owned()),
            Some(Json(json!({"numero": 2})))
        )
    );

    transaction
        .rollback()
        .await
        .expect("fixture must roll back");
}

#[tokio::test]
async fn audit_writer_uses_the_callers_transaction_and_rolls_back_with_it() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let mut transaction = db.begin().await.expect("transaction must begin");
    let organization_id = insert_organization(&mut transaction).await;
    let user_id = insert_user(&mut transaction, organization_id).await;

    let recorded = audit::record(
        &mut transaction,
        &NewAuditEvent {
            organization_id,
            actor: AuditActor::Usuario(user_id),
            action: "usuario.roles_actualizados",
            entity_type: "usuario",
            entity_id: Some(user_id),
            reference: None,
            before_state: Some(json!({"roles": ["operador"]})),
            after_state: Some(json!({"roles": ["administrador"]})),
        },
    )
    .await
    .expect("audit writer must append within caller transaction");
    let visible_in_transaction: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM public.audit_events WHERE id = $1)")
            .bind(recorded.id)
            .fetch_one(&mut *transaction)
            .await
            .expect("audit event must be visible in caller transaction");
    assert!(visible_in_transaction);

    transaction
        .rollback()
        .await
        .expect("caller transaction must roll back");
    let persisted_after_rollback: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM public.audit_events WHERE id = $1)")
            .bind(recorded.id)
            .fetch_one(&db)
            .await
            .expect("rolled-back audit event must be queryable");
    assert!(!persisted_after_rollback);
}
