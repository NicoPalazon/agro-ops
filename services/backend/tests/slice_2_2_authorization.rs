use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use agro_ops_backend::{
    AppState, app,
    auth::{AccessTokenVerifier, AuthenticatedUser, VerifyAccessTokenError},
    authorization::{self, ResolveAuthorizationError},
    service_heartbeats::{WORKER_SERVICE_NAME, record_service_heartbeat},
};
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

const VALID_TOKEN: &str = "valid-test-access-token";

#[derive(Clone)]
enum VerifierOutcome {
    Subject(String),
    Unavailable,
}

struct TestAccessTokenVerifier {
    outcome: VerifierOutcome,
}

#[async_trait]
impl AccessTokenVerifier for TestAccessTokenVerifier {
    async fn verify(
        &self,
        access_token: &str,
    ) -> Result<AuthenticatedUser, VerifyAccessTokenError> {
        if access_token != VALID_TOKEN {
            return Err(VerifyAccessTokenError::Invalid);
        }

        match &self.outcome {
            VerifierOutcome::Subject(id) => Ok(AuthenticatedUser { id: id.clone() }),
            VerifierOutcome::Unavailable => Err(VerifyAccessTokenError::Unavailable),
        }
    }
}

#[derive(Clone, Copy)]
struct Window {
    start_offset: &'static str,
    end_offset: Option<&'static str>,
}

const CURRENT: Window = Window {
    start_offset: "-1 hour",
    end_offset: None,
};
const EXPIRED: Window = Window {
    start_offset: "-2 hours",
    end_offset: Some("-1 hour"),
};
const FUTURE: Window = Window {
    start_offset: "+1 hour",
    end_offset: Some("+2 hours"),
};

struct FixtureOptions {
    identity_window: Window,
    user_active: bool,
    organization_active: bool,
    role_active: bool,
    user_role_window: Window,
    role_permission_window: Window,
    grant_permission: bool,
    role_name: &'static str,
    duplicate_permission_role: bool,
}

impl Default for FixtureOptions {
    fn default() -> Self {
        Self {
            identity_window: CURRENT,
            user_active: true,
            organization_active: true,
            role_active: true,
            user_role_window: CURRENT,
            role_permission_window: CURRENT,
            grant_permission: true,
            role_name: "Rol de prueba",
            duplicate_permission_role: false,
        }
    }
}

struct Fixture {
    subject: Uuid,
    user_id: Uuid,
    organization_id: Uuid,
    permission_code: String,
}

fn database_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
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

fn unavailable_pool() -> PgPool {
    PgPoolOptions::new()
        .acquire_timeout(Duration::from_millis(50))
        .connect_lazy("postgres://agro_ops:agro_ops@127.0.0.1:1/agro_ops")
        .expect("unavailable test database URL must be valid")
}

async fn insert_windowed_identity(db: &PgPool, user_id: Uuid, subject: Uuid, window: Window) {
    sqlx::query(
        r#"
        INSERT INTO public.identidades_autenticacion_externas
            (usuario_id, proveedor, sujeto_proveedor, vinculada_en, desvinculada_en)
        VALUES (
            $1,
            'supabase',
            $2,
            CURRENT_TIMESTAMP + $3::interval,
            CASE
                WHEN $4::text IS NULL THEN NULL
                ELSE CURRENT_TIMESTAMP + $4::interval
            END
        )
        "#,
    )
    .bind(user_id)
    .bind(subject)
    .bind(window.start_offset)
    .bind(window.end_offset)
    .execute(db)
    .await
    .expect("test external identity must insert");
}

async fn insert_windowed_user_role(db: &PgPool, user_id: Uuid, role_id: Uuid, window: Window) {
    sqlx::query(
        r#"
        INSERT INTO public.usuarios_roles (id, usuario_id, rol_id, vigente_desde, vigente_hasta)
        VALUES (
            $1,
            $2,
            $3,
            CURRENT_TIMESTAMP + $4::interval,
            CASE
                WHEN $5::text IS NULL THEN NULL
                ELSE CURRENT_TIMESTAMP + $5::interval
            END
        )
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(role_id)
    .bind(window.start_offset)
    .bind(window.end_offset)
    .execute(db)
    .await
    .expect("test user-role episode must insert");
}

async fn insert_windowed_role_permission(
    db: &PgPool,
    role_id: Uuid,
    permission_id: Uuid,
    window: Window,
) {
    sqlx::query(
        r#"
        INSERT INTO public.roles_permisos (id, rol_id, permiso_id, vigente_desde, vigente_hasta)
        VALUES (
            $1,
            $2,
            $3,
            CURRENT_TIMESTAMP + $4::interval,
            CASE
                WHEN $5::text IS NULL THEN NULL
                ELSE CURRENT_TIMESTAMP + $5::interval
            END
        )
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(role_id)
    .bind(permission_id)
    .bind(window.start_offset)
    .bind(window.end_offset)
    .execute(db)
    .await
    .expect("test role-permission episode must insert");
}

async fn create_fixture(db: &PgPool, options: FixtureOptions) -> Fixture {
    let tag = Uuid::new_v4().simple().to_string();
    let subject = Uuid::new_v4();
    let organization_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let role_id = Uuid::new_v4();
    let permission_code = authorization::permission_codes::CONSOLA_TECNICA_VER.to_owned();

    sqlx::query("INSERT INTO public.organizaciones (id, nombre, activa) VALUES ($1, $2, $3)")
        .bind(organization_id)
        .bind(format!("Organizacion autorizacion {tag}"))
        .bind(options.organization_active)
        .execute(db)
        .await
        .expect("test organization must insert");
    sqlx::query("INSERT INTO public.usuarios (id, organizacion_id, nombre_completo, activo) VALUES ($1, $2, $3, $4)")
        .bind(user_id)
        .bind(organization_id)
        .bind(format!("Usuario autorizacion {tag}"))
        .bind(options.user_active)
        .execute(db)
        .await
        .expect("test user must insert");
    insert_windowed_identity(db, user_id, subject, options.identity_window).await;
    sqlx::query(
        "INSERT INTO public.roles (id, organizacion_id, nombre, activo) VALUES ($1, $2, $3, $4)",
    )
    .bind(role_id)
    .bind(organization_id)
    .bind(options.role_name)
    .bind(options.role_active)
    .execute(db)
    .await
    .expect("test role must insert");

    let permission_id = sqlx::query_scalar("SELECT id FROM public.permisos WHERE codigo = $1")
        .bind(&permission_code)
        .fetch_one(db)
        .await
        .expect("canonical technical-console permission must exist");

    insert_windowed_user_role(db, user_id, role_id, options.user_role_window).await;
    if options.grant_permission {
        insert_windowed_role_permission(db, role_id, permission_id, options.role_permission_window)
            .await;
    }
    if options.duplicate_permission_role {
        let duplicate_role_id = Uuid::new_v4();
        sqlx::query("INSERT INTO public.roles (id, organizacion_id, nombre) VALUES ($1, $2, $3)")
            .bind(duplicate_role_id)
            .bind(organization_id)
            .bind(format!("Rol duplicado {tag}"))
            .execute(db)
            .await
            .expect("duplicate test role must insert");
        insert_windowed_user_role(db, user_id, duplicate_role_id, CURRENT).await;
        insert_windowed_role_permission(db, duplicate_role_id, permission_id, CURRENT).await;
    }

    Fixture {
        subject,
        user_id,
        organization_id,
        permission_code,
    }
}

fn subject_state(db: PgPool, subject: impl ToString) -> AppState {
    AppState {
        db,
        auth: Arc::new(TestAccessTokenVerifier {
            outcome: VerifierOutcome::Subject(subject.to_string()),
        }),
    }
}

fn unavailable_verifier_state(db: PgPool) -> AppState {
    AppState {
        db,
        auth: Arc::new(TestAccessTokenVerifier {
            outcome: VerifierOutcome::Unavailable,
        }),
    }
}

async fn send_request(
    state: AppState,
    path: &str,
    authorization: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::builder().uri(path);
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    app(state)
        .oneshot(
            request
                .body(Body::empty())
                .expect("test request must be valid"),
        )
        .await
        .expect("application request must complete")
}

async fn response_body(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), 128 * 1024)
        .await
        .expect("response body must be readable");
    String::from_utf8(bytes.to_vec()).expect("response body must be UTF-8")
}

fn assert_request_identity_headers(response: &axum::response::Response) {
    assert!(response.headers().contains_key("x-request-id"));
    assert!(response.headers().contains_key("x-correlation-id"));
}

#[tokio::test]
async fn resolve_context_honors_identity_user_and_organization_lifecycle() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let active = create_fixture(&db, FixtureOptions::default()).await;

    let context = authorization::resolve_context(&db, active.subject)
        .await
        .expect("active principal must resolve");
    assert_eq!(context.user_id, active.user_id);
    assert_eq!(context.organization_id, active.organization_id);

    for options in [
        FixtureOptions {
            identity_window: EXPIRED,
            ..FixtureOptions::default()
        },
        FixtureOptions {
            identity_window: FUTURE,
            ..FixtureOptions::default()
        },
        FixtureOptions {
            user_active: false,
            ..FixtureOptions::default()
        },
        FixtureOptions {
            organization_active: false,
            ..FixtureOptions::default()
        },
    ] {
        let fixture = create_fixture(&db, options).await;
        assert!(matches!(
            authorization::resolve_context(&db, fixture.subject).await,
            Err(ResolveAuthorizationError::PrincipalUnavailable)
        ));
    }

    assert!(matches!(
        authorization::resolve_context(&db, Uuid::new_v4()).await,
        Err(ResolveAuthorizationError::PrincipalUnavailable)
    ));
}

#[tokio::test]
async fn resolve_context_honors_temporal_grants_activity_and_permission_deduplication() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let current = create_fixture(&db, FixtureOptions::default()).await;
    let current_context = authorization::resolve_context(&db, current.subject)
        .await
        .expect("current fixture must resolve");
    assert!(current_context.has_permission(&current.permission_code));

    for options in [
        FixtureOptions {
            user_role_window: EXPIRED,
            ..FixtureOptions::default()
        },
        FixtureOptions {
            user_role_window: FUTURE,
            ..FixtureOptions::default()
        },
        FixtureOptions {
            role_permission_window: EXPIRED,
            ..FixtureOptions::default()
        },
        FixtureOptions {
            role_permission_window: FUTURE,
            ..FixtureOptions::default()
        },
        FixtureOptions {
            role_active: false,
            ..FixtureOptions::default()
        },
    ] {
        let fixture = create_fixture(&db, options).await;
        let context = authorization::resolve_context(&db, fixture.subject)
            .await
            .expect("active principal without an effective grant must resolve");
        assert!(!context.has_permission(&fixture.permission_code));
    }

    let inactive_permission = create_fixture(&db, FixtureOptions::default()).await;
    sqlx::query("UPDATE public.permisos SET activo = false WHERE codigo = $1")
        .bind(&inactive_permission.permission_code)
        .execute(&db)
        .await
        .expect("canonical permission must be temporarily deactivated");
    let resolution = authorization::resolve_context(&db, inactive_permission.subject).await;
    sqlx::query("UPDATE public.permisos SET activo = true WHERE codigo = $1")
        .bind(&inactive_permission.permission_code)
        .execute(&db)
        .await
        .expect("canonical permission must be restored");
    let context = resolution.expect("active principal with an inactive permission must resolve");
    assert!(!context.has_permission(&inactive_permission.permission_code));

    let duplicate = create_fixture(
        &db,
        FixtureOptions {
            duplicate_permission_role: true,
            ..FixtureOptions::default()
        },
    )
    .await;
    let duplicate_context = authorization::resolve_context(&db, duplicate.subject)
        .await
        .expect("duplicate grant fixture must resolve");
    assert_eq!(
        duplicate_context
            .permission_codes()
            .iter()
            .filter(|code| code.as_str() == duplicate.permission_code)
            .count(),
        1
    );
}

#[tokio::test]
async fn exact_administrador_role_name_has_no_authorization_effect_without_a_permission_grant() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let fixture = create_fixture(
        &db,
        FixtureOptions {
            grant_permission: false,
            role_name: "Administrador",
            ..FixtureOptions::default()
        },
    )
    .await;

    let context = authorization::resolve_context(&db, fixture.subject)
        .await
        .expect("an active actor with an active Administrador role must resolve");
    assert!(
        !context.has_permission(&fixture.permission_code),
        "the exact role name Administrador must not grant authorization"
    );

    let response = send_request(
        subject_state(db, fixture.subject),
        "/internal/worker/status",
        Some("Bearer valid-test-access-token"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn resolve_context_reports_database_unavailability_without_permission_denial() {
    let _guard = database_test_lock().lock().await;
    let error = authorization::resolve_context(&unavailable_pool(), Uuid::new_v4())
        .await
        .expect_err("unavailable PostgreSQL must not resolve authorization");
    assert!(matches!(
        error,
        ResolveAuthorizationError::DatabaseUnavailable(_)
    ));
}

#[tokio::test]
async fn internal_endpoints_enforce_authorization_boundaries() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;

    let unauthenticated = send_request(
        subject_state(db.clone(), Uuid::new_v4()),
        "/internal/worker/status",
        None,
    )
    .await;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    assert_request_identity_headers(&unauthenticated);

    let invalid = send_request(
        subject_state(db.clone(), Uuid::new_v4()),
        "/internal/worker/status",
        Some("Bearer invalid-token"),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
    assert_request_identity_headers(&invalid);

    let verifier_unavailable = send_request(
        unavailable_verifier_state(db.clone()),
        "/internal/worker/status",
        Some("Bearer valid-test-access-token"),
    )
    .await;
    assert_eq!(
        verifier_unavailable.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_request_identity_headers(&verifier_unavailable);

    let unprovisioned = send_request(
        subject_state(db.clone(), Uuid::new_v4()),
        "/internal/worker/status",
        Some("Bearer valid-test-access-token"),
    )
    .await;
    assert_eq!(unprovisioned.status(), StatusCode::FORBIDDEN);
    assert_request_identity_headers(&unprovisioned);
    let unprovisioned_body = response_body(unprovisioned).await;

    let inactive_user = create_fixture(
        &db,
        FixtureOptions {
            user_active: false,
            ..FixtureOptions::default()
        },
    )
    .await;
    let inactive_user_response = send_request(
        subject_state(db.clone(), inactive_user.subject),
        "/internal/worker/status",
        Some("Bearer valid-test-access-token"),
    )
    .await;
    assert_eq!(inactive_user_response.status(), StatusCode::FORBIDDEN);

    let no_permission = create_fixture(
        &db,
        FixtureOptions {
            grant_permission: false,
            ..FixtureOptions::default()
        },
    )
    .await;
    let no_permission_response = send_request(
        subject_state(db.clone(), no_permission.subject),
        "/internal/worker/status",
        Some("Bearer valid-test-access-token"),
    )
    .await;
    assert_eq!(no_permission_response.status(), StatusCode::FORBIDDEN);
    assert_request_identity_headers(&no_permission_response);
    assert_eq!(
        response_body(no_permission_response).await,
        unprovisioned_body
    );

    for options in [
        FixtureOptions {
            user_role_window: EXPIRED,
            ..FixtureOptions::default()
        },
        FixtureOptions {
            role_permission_window: EXPIRED,
            ..FixtureOptions::default()
        },
    ] {
        let fixture = create_fixture(&db, options).await;
        let response = send_request(
            subject_state(db.clone(), fixture.subject),
            "/internal/worker/status",
            Some("Bearer valid-test-access-token"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    let authorized = create_fixture(&db, FixtureOptions::default()).await;
    record_service_heartbeat(&db, WORKER_SERVICE_NAME)
        .await
        .expect("worker heartbeat must be writable");
    let worker_response = send_request(
        subject_state(db.clone(), authorized.subject),
        "/internal/worker/status",
        Some("Bearer valid-test-access-token"),
    )
    .await;
    assert_eq!(worker_response.status(), StatusCode::OK);
    let system_response = send_request(
        subject_state(db.clone(), authorized.subject),
        "/internal/system-status",
        Some("Bearer valid-test-access-token"),
    )
    .await;
    assert_eq!(system_response.status(), StatusCode::OK);

    let non_uuid_subject = send_request(
        subject_state(db.clone(), "not-a-uuid"),
        "/internal/worker/status",
        Some("Bearer valid-test-access-token"),
    )
    .await;
    assert_eq!(non_uuid_subject.status(), StatusCode::UNAUTHORIZED);

    let database_unavailable = send_request(
        subject_state(unavailable_pool(), Uuid::new_v4()),
        "/internal/system-status",
        Some("Bearer valid-test-access-token"),
    )
    .await;
    assert_eq!(
        database_unavailable.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_request_identity_headers(&database_unavailable);
}
