use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, OnceLock},
};

use agro_ops_backend::{
    AppState, app,
    auth::{AccessTokenVerifier, AuthenticatedUser, VerifyAccessTokenError},
    documents::{DocumentSettings, DocumentStorage, DocumentStorageError, PrivateDownloadAccess},
    supabase_admin::UnavailableExternalIdentityAdmin,
};
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

const TOKEN_PREFIX: &str = "document-test-token:";

struct TestVerifier;

#[async_trait]
impl AccessTokenVerifier for TestVerifier {
    async fn verify(&self, token: &str) -> Result<AuthenticatedUser, VerifyAccessTokenError> {
        let subject = token
            .strip_prefix(TOKEN_PREFIX)
            .filter(|value| Uuid::parse_str(value).is_ok())
            .ok_or(VerifyAccessTokenError::Invalid)?;
        Ok(AuthenticatedUser {
            id: subject.to_owned(),
        })
    }
}

#[derive(Default)]
struct FakeStorageState {
    puts: Vec<(String, String, String, Vec<u8>)>,
    deletes: Vec<(String, String)>,
    fail_put: bool,
    fail_sign: bool,
    delete_outcomes: VecDeque<Result<(), DocumentStorageError>>,
}

#[derive(Clone, Default)]
struct FakeStorage {
    state: Arc<Mutex<FakeStorageState>>,
}

#[async_trait]
impl DocumentStorage for FakeStorage {
    async fn put(
        &self,
        bucket: &str,
        key: &str,
        mime_type: &str,
        content: &[u8],
    ) -> Result<(), DocumentStorageError> {
        let mut state = self
            .state
            .lock()
            .expect("fake storage lock must be available");
        if state.fail_put {
            return Err(DocumentStorageError::Unavailable);
        }
        state.puts.push((
            bucket.to_owned(),
            key.to_owned(),
            mime_type.to_owned(),
            content.to_vec(),
        ));
        Ok(())
    }

    async fn create_download_access(
        &self,
        _bucket: &str,
        _key: &str,
    ) -> Result<PrivateDownloadAccess, DocumentStorageError> {
        if self
            .state
            .lock()
            .expect("fake storage lock must be available")
            .fail_sign
        {
            return Err(DocumentStorageError::Unavailable);
        }
        Ok(PrivateDownloadAccess {
            url: "https://private.example.test/signed-download?token=short-lived".to_owned(),
            expires_in_seconds: 60,
        })
    }

    async fn delete(&self, bucket: &str, key: &str) -> Result<(), DocumentStorageError> {
        let mut state = self
            .state
            .lock()
            .expect("fake storage lock must be available");
        state.deletes.push((bucket.to_owned(), key.to_owned()));
        state.delete_outcomes.pop_front().unwrap_or(Ok(()))
    }
}

async fn test_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
        .await
        .expect("PostgreSQL with migrations must be available")
}

fn database_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn principal(db: &PgPool) -> (Uuid, Uuid, Uuid) {
    let organization_id =
        sqlx::query_scalar("INSERT INTO organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(format!("Organización API documentos {}", Uuid::new_v4()))
            .fetch_one(db)
            .await
            .expect("organization must insert");
    let user_id = sqlx::query_scalar(
        "INSERT INTO usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
    )
    .bind(organization_id)
    .bind(format!("Usuario API documentos {}", Uuid::new_v4()))
    .fetch_one(db)
    .await
    .expect("user must insert");
    let subject = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, 'supabase', $2)",
    )
    .bind(user_id)
    .bind(subject)
    .execute(db)
    .await
    .expect("identity must insert");
    (organization_id, user_id, subject)
}

fn state(db: PgPool, storage: Arc<FakeStorage>) -> AppState {
    AppState {
        db,
        auth: Arc::new(TestVerifier),
        external_identity_admin: Arc::new(UnavailableExternalIdentityAdmin),
        document_storage: storage,
        document_settings: DocumentSettings::new("documentos_privados", 4),
    }
}

fn token(subject: Uuid) -> String {
    format!("{TOKEN_PREFIX}{subject}")
}

fn upload_request(bytes: &[u8], authorization: Option<String>) -> Request<Body> {
    let boundary = "document-boundary";
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"archivo\"; filename=\"comprobante.pdf\"\r\nContent-Type: application/pdf\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let mut request = Request::builder().method("POST").uri("/documentos").header(
        "content-type",
        format!("multipart/form-data; boundary={boundary}"),
    );
    if let Some(authorization) = authorization {
        request = request.header("authorization", format!("Bearer {authorization}"));
    }
    request
        .body(Body::from(body))
        .expect("upload request must build")
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body must read"),
    )
    .expect("response must be JSON")
}

#[tokio::test]
async fn upload_requires_an_authenticated_active_agro_ops_principal_and_enforces_bounded_size() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let storage = Arc::new(FakeStorage::default());
    let anonymous = app(state(db.clone(), storage.clone()))
        .oneshot(upload_request(b"abc", None))
        .await
        .expect("anonymous response must return");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

    let unprovisioned = app(state(db.clone(), storage.clone()))
        .oneshot(upload_request(b"abc", Some(token(Uuid::new_v4()))))
        .await
        .expect("unprovisioned response must return");
    assert_eq!(unprovisioned.status(), StatusCode::FORBIDDEN);

    let (_, _, subject) = principal(&db).await;
    let oversized = app(state(db, storage))
        .oneshot(upload_request(b"12345", Some(token(subject))))
        .await
        .expect("oversized response must return");
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn successful_upload_hashes_actual_bytes_audits_and_reports_duplicate_content() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let storage = Arc::new(FakeStorage::default());
    let (organization_id, user_id, subject) = principal(&db).await;
    let bytes = b"abc";
    let first_response = app(state(db.clone(), storage.clone()))
        .oneshot(upload_request(bytes, Some(token(subject))))
        .await
        .expect("upload response must return");
    assert_eq!(first_response.status(), StatusCode::CREATED);
    let first = body_json(first_response).await;
    let expected_hash = format!("{:x}", Sha256::digest(bytes));
    assert_eq!(first["sha256"], expected_hash);
    assert_eq!(first["tamano_bytes"], 3);
    assert!(
        first["duplicate_content_of"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    let document_id = first["id"].as_str().expect("id must be present").to_owned();
    let persisted: (Uuid, Uuid, Vec<u8>) =
        sqlx::query_as("SELECT organizacion_id, creado_por, sha256 FROM documentos WHERE id = $1")
            .bind(Uuid::parse_str(&document_id).expect("document ID must be UUID"))
            .fetch_one(&db)
            .await
            .expect("document metadata must persist");
    assert_eq!(
        persisted,
        (organization_id, user_id, Sha256::digest(bytes).to_vec())
    );
    let audit: (String, Uuid, String) = sqlx::query_as(
        "SELECT accion, actor_usuario_id, entidad_tipo FROM audit_events WHERE entidad_id = $1",
    )
    .bind(Uuid::parse_str(&document_id).expect("document ID must be UUID"))
    .fetch_one(&db)
    .await
    .expect("document audit must persist");
    assert_eq!(
        audit,
        (
            "documento.creado".to_owned(),
            user_id,
            "documento".to_owned()
        )
    );
    let puts = storage
        .state
        .lock()
        .expect("fake storage lock must be available")
        .puts
        .clone();
    assert_eq!(puts.len(), 1);
    assert_eq!(puts[0].0, "documentos_privados");
    assert_eq!(puts[0].2, "application/pdf");
    assert_eq!(puts[0].3, bytes);
    assert!(puts[0].1.contains(&organization_id.to_string()));

    let duplicate_response = app(state(db, storage))
        .oneshot(upload_request(bytes, Some(token(subject))))
        .await
        .expect("duplicate upload response must return");
    assert_eq!(duplicate_response.status(), StatusCode::CREATED);
    let duplicate = body_json(duplicate_response).await;
    assert_eq!(
        duplicate["duplicate_content_of"],
        serde_json::json!([document_id])
    );
}

#[tokio::test]
async fn storage_or_postgresql_failure_never_leaves_success_metadata_and_database_failure_cleans_storage()
 {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let storage = Arc::new(FakeStorage::default());
    let (organization_id, _, subject) = principal(&db).await;
    storage
        .state
        .lock()
        .expect("fake storage lock must be available")
        .fail_put = true;
    let failed_storage = app(state(db.clone(), storage.clone()))
        .oneshot(upload_request(b"abc", Some(token(subject))))
        .await
        .expect("storage failure response must return");
    assert_eq!(failed_storage.status(), StatusCode::SERVICE_UNAVAILABLE);
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM documentos WHERE organizacion_id = $1")
            .bind(organization_id)
            .fetch_one(&db)
            .await
            .expect("count must load");
    assert_eq!(count, 0);

    storage
        .state
        .lock()
        .expect("fake storage lock must be available")
        .fail_put = false;
    sqlx::query(
        r#"
        CREATE OR REPLACE FUNCTION public.documentos_test_rechazar_insert()
        RETURNS TRIGGER LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'document test failure'; END; $$
        "#,
    )
    .execute(&db)
    .await
    .expect("test failure function must create");
    sqlx::query(
        "CREATE TRIGGER documentos_test_rechazar_insert BEFORE INSERT ON documentos FOR EACH ROW EXECUTE FUNCTION public.documentos_test_rechazar_insert()",
    )
    .execute(&db)
    .await
    .expect("test failure trigger must create");
    let failed_database = app(state(db.clone(), storage.clone()))
        .oneshot(upload_request(b"abc", Some(token(subject))))
        .await
        .expect("database failure response must return");
    assert_eq!(failed_database.status(), StatusCode::SERVICE_UNAVAILABLE);
    sqlx::query("DROP TRIGGER documentos_test_rechazar_insert ON documentos")
        .execute(&db)
        .await
        .expect("test failure trigger must drop");
    sqlx::query("DROP FUNCTION public.documentos_test_rechazar_insert()")
        .execute(&db)
        .await
        .expect("test failure function must drop");
    sqlx::query(
        r#"
        CREATE OR REPLACE FUNCTION public.documentos_test_rechazar_audit()
        RETURNS TRIGGER LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'audit test failure'; END; $$
        "#,
    )
    .execute(&db)
    .await
    .expect("audit failure function must create");
    sqlx::query(
        "CREATE TRIGGER documentos_test_rechazar_audit BEFORE INSERT ON audit_events FOR EACH ROW EXECUTE FUNCTION public.documentos_test_rechazar_audit()",
    )
    .execute(&db)
    .await
    .expect("audit failure trigger must create");
    let failed_audit = app(state(db.clone(), storage.clone()))
        .oneshot(upload_request(b"abc", Some(token(subject))))
        .await
        .expect("audit failure response must return");
    assert_eq!(failed_audit.status(), StatusCode::SERVICE_UNAVAILABLE);
    sqlx::query("DROP TRIGGER documentos_test_rechazar_audit ON audit_events")
        .execute(&db)
        .await
        .expect("audit failure trigger must drop");
    sqlx::query("DROP FUNCTION public.documentos_test_rechazar_audit()")
        .execute(&db)
        .await
        .expect("audit failure function must drop");
    let (put_count, delete_count) = {
        let state = storage
            .state
            .lock()
            .expect("fake storage lock must be available");
        (state.puts.len(), state.deletes.len())
    };
    assert_eq!(put_count, 2);
    assert_eq!(delete_count, 2);
    let documents: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM documentos WHERE organizacion_id = $1")
            .bind(organization_id)
            .fetch_one(&db)
            .await
            .expect("count must load");
    let audits: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE organizacion_id = $1 AND accion = 'documento.creado'")
        .bind(organization_id)
        .fetch_one(&db)
        .await
        .expect("count must load");
    assert_eq!((documents, audits), (0, 0));
}

#[tokio::test]
async fn metadata_and_private_download_access_are_scoped_to_the_callers_organization() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let storage = Arc::new(FakeStorage::default());
    let (_, _, owner_subject) = principal(&db).await;
    let (_, _, other_subject) = principal(&db).await;
    let uploaded = app(state(db.clone(), storage.clone()))
        .oneshot(upload_request(b"abc", Some(token(owner_subject))))
        .await
        .expect("upload response must return");
    let document_id = body_json(uploaded).await["id"]
        .as_str()
        .expect("document ID must exist")
        .to_owned();
    for path in [
        format!("/documentos/{document_id}"),
        format!("/documentos/{document_id}/descarga"),
    ] {
        let denied = app(state(db.clone(), storage.clone()))
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("authorization", format!("Bearer {}", token(other_subject)))
                    .body(Body::empty())
                    .expect("request must build"),
            )
            .await
            .expect("cross-organization response must return");
        assert_eq!(denied.status(), StatusCode::NOT_FOUND);
    }
    let download = app(state(db, storage))
        .oneshot(
            Request::builder()
                .uri(format!("/documentos/{document_id}/descarga"))
                .header("authorization", format!("Bearer {}", token(owner_subject)))
                .body(Body::empty())
                .expect("request must build"),
        )
        .await
        .expect("owner download must return");
    assert_eq!(download.status(), StatusCode::OK);
    let download = body_json(download).await;
    assert_eq!(download["expires_in_seconds"], 60);
    assert!(
        download["url"]
            .as_str()
            .is_some_and(|url| url.contains("signed-download"))
    );
    assert!(!download.to_string().contains("service-role-not-public"));
}
