use agro_ops_backend::external_references::{
    ExternalEntityType, ExternalId, ExternalReferenceStoreError, ExternalSystem, LastSyncStatus,
    NewExternalReference, SyncMetadata, SyncVersion, list_for_entity, register,
    resolve_by_external_identity, update_sync_metadata,
};
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
        .bind(format!(
            "Organización referencias externas {}",
            Uuid::new_v4()
        ))
        .fetch_one(db)
        .await
        .expect("test organization must insert")
}

fn reference(
    organization_id: Option<Uuid>,
    system: ExternalSystem,
    external_id: impl Into<String>,
    entity_type: &str,
    entity_id: Uuid,
) -> NewExternalReference {
    NewExternalReference {
        organization_id,
        system,
        external_id: ExternalId::new(external_id).expect("external id must be valid"),
        entity_type: ExternalEntityType::new(entity_type).expect("entity type must be valid"),
        entity_id,
        sync_version: Some(SyncVersion::new("revision-1").expect("sync version must be valid")),
        last_sync_status: LastSyncStatus::Pendiente,
        last_synced_at: None,
    }
}

async fn commit_registration(
    db: &PgPool,
    reference: NewExternalReference,
) -> agro_ops_backend::external_references::RegisteredExternalReference {
    let mut transaction = db.begin().await.expect("transaction must begin");
    let recorded = register(&mut transaction, reference)
        .await
        .expect("reference must register");
    transaction.commit().await.expect("transaction must commit");
    recorded
}

#[tokio::test]
async fn registration_is_transactional_and_resolves_opaque_external_identity() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let entity_id = Uuid::new_v4();
    let input = reference(
        Some(organization_id),
        ExternalSystem::finnegans(),
        "provider/item?revision=00042",
        "stock.movimiento",
        entity_id,
    );

    let mut transaction = db.begin().await.expect("transaction must begin");
    let registered = register(&mut transaction, input.clone())
        .await
        .expect("reference must register through caller transaction");
    assert!(registered.created);
    let visible_inside: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM external_references WHERE id = $1")
            .bind(registered.reference.id)
            .fetch_one(&mut *transaction)
            .await
            .expect("caller transaction can see its reference");
    assert_eq!(visible_inside, 1);
    transaction.rollback().await.expect("rollback must succeed");

    let visible_after_rollback: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM external_references WHERE id = $1")
            .bind(registered.reference.id)
            .fetch_one(&db)
            .await
            .expect("reference count must load");
    assert_eq!(visible_after_rollback, 0);

    let committed = commit_registration(&db, input).await;
    let resolved = resolve_by_external_identity(
        &db,
        Some(organization_id),
        &ExternalSystem::finnegans(),
        &ExternalId::new("provider/item?revision=00042").expect("opaque id must be accepted"),
    )
    .await
    .expect("reference must resolve")
    .expect("reference must exist");
    assert_eq!(resolved.id, committed.reference.id);
    assert_eq!(resolved.entity_id, entity_id);
    assert_eq!(
        resolved.external_id.as_str(),
        "provider/item?revision=00042"
    );
}

#[tokio::test]
async fn identical_registration_is_idempotent_but_relinking_is_a_typed_conflict() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let entity_id = Uuid::new_v4();
    let input = reference(
        Some(organization_id),
        ExternalSystem::finnegans(),
        format!("invoice/{}", Uuid::new_v4()),
        "comercial.comprobante",
        entity_id,
    );
    let first = commit_registration(&db, input.clone()).await;
    let repeated = commit_registration(&db, input.clone()).await;
    assert!(first.created);
    assert!(!repeated.created);
    assert_eq!(first.reference.id, repeated.reference.id);

    let mut conflicting = input;
    conflicting.entity_id = Uuid::new_v4();
    let mut transaction = db.begin().await.expect("transaction must begin");
    assert!(matches!(
        register(&mut transaction, conflicting).await,
        Err(ExternalReferenceStoreError::IdentityConflict)
    ));
    transaction.rollback().await.expect("rollback must succeed");

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM external_references WHERE organizacion_id = $1",
    )
    .bind(organization_id)
    .fetch_one(&db)
    .await
    .expect("reference count must load");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn scopes_systems_and_global_identity_are_independent_with_null_safe_uniqueness() {
    let db = test_pool().await;
    let first_organization = organization(&db).await;
    let second_organization = organization(&db).await;
    let external_id = format!("opaque-{}", Uuid::new_v4());

    let first = commit_registration(
        &db,
        reference(
            Some(first_organization),
            ExternalSystem::finnegans(),
            &external_id,
            "stock.movimiento",
            Uuid::new_v4(),
        ),
    )
    .await;
    let second_scope = commit_registration(
        &db,
        reference(
            Some(second_organization),
            ExternalSystem::finnegans(),
            &external_id,
            "stock.movimiento",
            Uuid::new_v4(),
        ),
    )
    .await;
    let second_system = commit_registration(
        &db,
        reference(
            Some(first_organization),
            ExternalSystem::arca(),
            &external_id,
            "stock.movimiento",
            Uuid::new_v4(),
        ),
    )
    .await;
    assert_ne!(first.reference.id, second_scope.reference.id);
    assert_ne!(first.reference.id, second_system.reference.id);

    let global_id = format!("global-{}", Uuid::new_v4());
    let global = reference(
        None,
        ExternalSystem::arca(),
        &global_id,
        "infraestructura.configuracion",
        Uuid::new_v4(),
    );
    let global_first = commit_registration(&db, global.clone()).await;
    let global_repeat = commit_registration(&db, global.clone()).await;
    assert_eq!(global_first.reference.id, global_repeat.reference.id);

    let mut global_conflict = global;
    global_conflict.entity_id = Uuid::new_v4();
    let mut transaction = db.begin().await.expect("transaction must begin");
    assert!(matches!(
        register(&mut transaction, global_conflict).await,
        Err(ExternalReferenceStoreError::IdentityConflict)
    ));
    transaction.rollback().await.expect("rollback must succeed");
}

#[tokio::test]
async fn synchronization_metadata_updates_without_rewriting_identity() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let entity_id = Uuid::new_v4();
    let registered = commit_registration(
        &db,
        reference(
            Some(organization_id),
            ExternalSystem::arca(),
            format!("record/{}", Uuid::new_v4()),
            "documento",
            entity_id,
        ),
    )
    .await;

    let sync_time = OffsetDateTime::now_utc();
    let mut transaction = db.begin().await.expect("transaction must begin");
    let updated = update_sync_metadata(
        &mut transaction,
        registered.reference.id,
        SyncMetadata {
            sync_version: Some(SyncVersion::new("etag-2").expect("version must be valid")),
            last_sync_status: LastSyncStatus::Sincronizado,
            last_synced_at: Some(sync_time),
        },
    )
    .await
    .expect("metadata must update");
    transaction.commit().await.expect("transaction must commit");
    assert_eq!(updated.entity_id, entity_id);
    assert_eq!(updated.system, ExternalSystem::arca());
    assert_eq!(
        updated.sync_version.expect("version must exist").as_str(),
        "etag-2"
    );
    assert_eq!(updated.last_sync_status, LastSyncStatus::Sincronizado);

    let raw_identity_update =
        sqlx::query("UPDATE external_references SET entidad_id = $2 WHERE id = $1")
            .bind(registered.reference.id)
            .bind(Uuid::new_v4())
            .execute(&db)
            .await;
    assert!(raw_identity_update.is_err());

    let listed = list_for_entity(
        &db,
        Some(organization_id),
        &ExternalEntityType::new("documento").expect("entity type must be valid"),
        entity_id,
    )
    .await
    .expect("entity references must list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, registered.reference.id);
}

#[tokio::test]
async fn database_constraints_and_restrictive_organization_history_are_enforced() {
    let db = test_pool().await;
    let organization_id = organization(&db).await;
    let valid_entity_id = Uuid::new_v4();

    for (system, external_id, entity_type, status) in [
        ("Finnegans", "valid", "documento", "pendiente"),
        ("finnegans", "   ", "documento", "pendiente"),
        ("finnegans", "valid", "Documento", "pendiente"),
        ("finnegans", "valid", "documento", "unknown"),
        ("finnegans", &"x".repeat(257), "documento", "pendiente"),
    ] {
        let result = sqlx::query(
            r#"
            INSERT INTO external_references (
                organizacion_id, sistema_externo, external_id, entidad_tipo, entidad_id,
                last_sync_status
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(organization_id)
        .bind(system)
        .bind(external_id)
        .bind(entity_type)
        .bind(valid_entity_id)
        .bind(status)
        .execute(&db)
        .await;
        assert!(
            result.is_err(),
            "invalid external-reference row must be rejected"
        );
    }

    let missing_organization = sqlx::query(
        r#"
        INSERT INTO external_references (
            organizacion_id, sistema_externo, external_id, entidad_tipo, entidad_id,
            last_sync_status
        )
        VALUES ($1, 'finnegans', 'missing-org', 'documento', $2, 'pendiente')
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(valid_entity_id)
    .execute(&db)
    .await;
    assert!(missing_organization.is_err());

    let preserved = commit_registration(
        &db,
        reference(
            Some(organization_id),
            ExternalSystem::finnegans(),
            format!("preserve-history-{}", Uuid::new_v4()),
            "documento",
            valid_entity_id,
        ),
    )
    .await;
    let delete_reference = sqlx::query("DELETE FROM external_references WHERE id = $1")
        .bind(preserved.reference.id)
        .execute(&db)
        .await
        .expect_err("external identity history must not be deleted");
    assert_eq!(
        delete_reference
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("P0001")
    );
    let truncate_references = sqlx::query("TRUNCATE TABLE external_references")
        .execute(&db)
        .await
        .expect_err("referenced external references must not be truncated");
    assert_eq!(
        truncate_references
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("0A000")
    );

    // Including the direct FK dependent table lets PostgreSQL reach the ExternalReference
    // trigger; the exact message excludes fuentes_geograficas' own truncate trigger.
    let truncate_references_with_geographic_sources =
        sqlx::query("TRUNCATE TABLE external_references, fuentes_geograficas")
            .execute(&db)
            .await
            .expect_err("external identity history must not be truncated");
    let truncate_error = truncate_references_with_geographic_sources
        .as_database_error()
        .expect("truncate failure must be a database error");
    assert_eq!(truncate_error.code().as_deref(), Some("P0001"));
    assert_eq!(
        truncate_error.message(),
        "agro_ops_external_reference_truncate_prohibido"
    );
    let delete_organization = sqlx::query("DELETE FROM organizaciones WHERE id = $1")
        .bind(organization_id)
        .execute(&db)
        .await;
    assert!(delete_organization.is_err());
}
