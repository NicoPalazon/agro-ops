use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

use agro_ops_backend::documents::storage_key;

async fn test_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
        .await
        .expect("PostgreSQL with migrations must be available")
}

async fn organization(db: &PgPool) -> Uuid {
    sqlx::query_scalar("INSERT INTO organizaciones (nombre) VALUES ($1) RETURNING id")
        .bind(format!("Organización documentos {}", Uuid::new_v4()))
        .fetch_one(db)
        .await
        .expect("organization must insert")
}

async fn user(db: &PgPool, organization_id: Uuid) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
    )
    .bind(organization_id)
    .bind(format!("Usuario documentos {}", Uuid::new_v4()))
    .fetch_one(db)
    .await
    .expect("user must insert")
}

async fn insert_document(
    db: &PgPool,
    organization_id: Uuid,
    creator_id: Uuid,
    sha256: &[u8],
    document_id: Uuid,
    key: String,
) -> Uuid {
    sqlx::query_scalar(
        r#"
        INSERT INTO documentos (
            id, organizacion_id, nombre_original, tipo_mime, tamano_bytes,
            sha256, storage_bucket, storage_key, creado_por
        )
        VALUES ($1, $2, 'factura.pdf', 'application/pdf', 42, $3, 'documentos_privados', $4, $5)
        RETURNING id
        "#,
    )
    .bind(document_id)
    .bind(organization_id)
    .bind(sha256)
    .bind(key)
    .bind(creator_id)
    .fetch_one(db)
    .await
    .expect("valid document must insert")
}

#[tokio::test]
async fn document_metadata_enforces_identity_foreign_keys_and_material_constraints() {
    let db = test_pool().await;
    let first_organization = organization(&db).await;
    let second_organization = organization(&db).await;
    let first_user = user(&db, first_organization).await;
    let second_user = user(&db, second_organization).await;
    let document_id = Uuid::new_v4();
    insert_document(
        &db,
        first_organization,
        first_user,
        &[7; 32],
        document_id,
        storage_key(first_organization, document_id),
    )
    .await;
    let row: (Uuid, i64, Vec<u8>, String) = sqlx::query_as(
        "SELECT id, tamano_bytes, sha256, storage_bucket FROM documentos WHERE id = $1",
    )
    .bind(document_id)
    .fetch_one(&db)
    .await
    .expect("stored document must load");
    assert_eq!(
        row,
        (
            document_id,
            42,
            vec![7; 32],
            "documentos_privados".to_owned()
        )
    );

    let invalid_statements = [
        r#"INSERT INTO documentos (id, organizacion_id, nombre_original, tipo_mime, tamano_bytes, sha256, storage_bucket, storage_key, creado_por) VALUES ('00000000-0000-0000-0000-000000000002', '00000000-0000-0000-0000-000000000001', 'x', 'text/plain', 1, decode(repeat('00', 32), 'hex'), 'documentos_privados', 'organizaciones/00000000-0000-0000-0000-000000000001/documentos/00000000-0000-0000-0000-000000000002/contenido', '00000000-0000-0000-0000-000000000003')"#,
        r#"INSERT INTO documentos (id, organizacion_id, nombre_original, tipo_mime, tamano_bytes, sha256, storage_bucket, storage_key, creado_por) VALUES ($4, $1, 'x', 'text/plain', 0, decode(repeat('00', 32), 'hex'), 'documentos_privados', $2, $3)"#,
        r#"INSERT INTO documentos (id, organizacion_id, nombre_original, tipo_mime, tamano_bytes, sha256, storage_bucket, storage_key, creado_por) VALUES ($4, $1, 'x', 'text/plain', 1, decode(repeat('00', 31), 'hex'), 'documentos_privados', $2, $3)"#,
    ];
    let foreign_key_error = sqlx::query(invalid_statements[0])
        .execute(&db)
        .await
        .expect_err("missing organization and user must fail");
    assert_eq!(
        foreign_key_error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503")
    );
    let missing_creator_document_id = Uuid::new_v4();
    let missing_creator = sqlx::query(
        r#"
        INSERT INTO documentos (id, organizacion_id, nombre_original, tipo_mime, tamano_bytes, sha256, storage_bucket, storage_key, creado_por)
        VALUES ($4, $1, 'x', 'text/plain', 1, decode(repeat('00', 32), 'hex'), 'documentos_privados', $2, $3)
        "#,
    )
    .bind(first_organization)
    .bind(storage_key(first_organization, missing_creator_document_id))
    .bind(Uuid::new_v4())
    .bind(missing_creator_document_id)
    .execute(&db)
    .await
    .expect_err("missing creator must fail its restrictive FK");
    assert_eq!(
        missing_creator
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503")
    );
    for statement in invalid_statements.into_iter().skip(1) {
        let invalid_document_id = Uuid::new_v4();
        let error = sqlx::query(statement)
            .bind(first_organization)
            .bind(storage_key(first_organization, invalid_document_id))
            .bind(first_user)
            .bind(invalid_document_id)
            .execute(&db)
            .await
            .expect_err("invalid document material value must fail");
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23514")
        );
    }

    let mismatch_document_id = Uuid::new_v4();
    let mismatch = sqlx::query(
        r#"
        INSERT INTO documentos (id, organizacion_id, nombre_original, tipo_mime, tamano_bytes, sha256, storage_bucket, storage_key, creado_por)
        VALUES ($4, $1, 'x', 'text/plain', 1, decode(repeat('00', 32), 'hex'), 'documentos_privados', $2, $3)
        "#,
    )
    .bind(first_organization)
    .bind(storage_key(first_organization, mismatch_document_id))
    .bind(second_user)
    .bind(mismatch_document_id)
    .execute(&db)
    .await
    .expect_err("cross-organization creator must fail");
    let database_error = mismatch
        .as_database_error()
        .expect("database error expected");
    assert_eq!(database_error.code().as_deref(), Some("P0001"));
    assert_eq!(
        database_error.message(),
        "agro_ops_documento_creador_organizacion_invalida"
    );

    let mismatched_document_id = Uuid::new_v4();
    let mismatched_storage_identity = sqlx::query(
        r#"
        INSERT INTO documentos (
            id, organizacion_id, nombre_original, tipo_mime, tamano_bytes,
            sha256, storage_bucket, storage_key, creado_por
        )
        VALUES ($1, $2, 'x', 'text/plain', 1, decode(repeat('00', 32), 'hex'),
                'documentos_privados', $3, $4)
        "#,
    )
    .bind(mismatched_document_id)
    .bind(first_organization)
    .bind(storage_key(second_organization, mismatched_document_id))
    .bind(first_user)
    .execute(&db)
    .await
    .expect_err("storage identity must match the document row");
    assert_eq!(
        mismatched_storage_identity
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );
}

#[tokio::test]
async fn duplicate_content_lookup_is_scoped_and_storage_keys_follow_document_identity() {
    let db = test_pool().await;
    let first_organization = organization(&db).await;
    let second_organization = organization(&db).await;
    let first_user = user(&db, first_organization).await;
    let second_user = user(&db, second_organization).await;
    let hash = [19; 32];
    let first_document_id = Uuid::new_v4();
    let first_key = storage_key(first_organization, first_document_id);
    let first_document = insert_document(
        &db,
        first_organization,
        first_user,
        &hash,
        first_document_id,
        first_key.clone(),
    )
    .await;
    let second_document_id = Uuid::new_v4();
    let second_document = insert_document(
        &db,
        second_organization,
        second_user,
        &hash,
        second_document_id,
        storage_key(second_organization, second_document_id),
    )
    .await;
    let first_matches: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM documentos WHERE organizacion_id = $1 AND sha256 = $2 ORDER BY id",
    )
    .bind(first_organization)
    .bind(&hash[..])
    .fetch_all(&db)
    .await
    .expect("duplicate lookup must work");
    assert_eq!(first_matches, vec![first_document]);
    assert_ne!(first_document, second_document);
    let persisted_key: String =
        sqlx::query_scalar("SELECT storage_key FROM documentos WHERE id = $1")
            .bind(first_document)
            .fetch_one(&db)
            .await
            .expect("storage key must remain queryable");
    assert_eq!(persisted_key, first_key);

    let organization_delete = sqlx::query("DELETE FROM organizaciones WHERE id = $1")
        .bind(first_organization)
        .execute(&db)
        .await
        .expect_err("document history must keep organization restrictive");
    assert_eq!(
        organization_delete
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23503")
    );
}
