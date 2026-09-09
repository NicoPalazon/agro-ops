use std::{sync::Arc, time::Instant};

use axum::{
    Json, Router,
    extract::{MatchedPath, Path, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use sqlx::PgPool;
use tracing::{Instrument, error, info, info_span, warn};
use utoipa::{Modify, OpenApi, ToSchema};
use uuid::Uuid;

pub mod access_administration;
pub mod access_provisioning;
pub mod auth;
pub mod authorization;
pub mod config;
pub mod service_heartbeats;
pub mod shutdown;
pub mod supabase_admin;
pub mod telemetry;
pub mod worker;

use service_heartbeats::{ServiceStatus, ServiceStatusReport, WORKER_SERVICE_NAME};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub auth: Arc<dyn auth::AccessTokenVerifier>,
    pub external_identity_admin: Arc<dyn supabase_admin::ExternalIdentityAdmin>,
}

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");
const CORRELATION_ID_HEADER: HeaderName = HeaderName::from_static("x-correlation-id");
const MAX_CORRELATION_ID_LENGTH: usize = 128;

#[derive(Serialize, ToSchema)]
struct StatusResponse {
    status: &'static str,
}

#[derive(Serialize, ToSchema)]
struct VersionResponse {
    service: &'static str,
    version: &'static str,
}

#[derive(Serialize, ToSchema)]
struct SystemStatusResponse {
    api: StatusResponse,
    database: StatusResponse,
    worker: ServiceStatusReport,
    version: VersionResponse,
}

#[derive(Serialize, ToSchema)]
struct MeResponse {
    usuario_id: String,
    organizacion_id: String,
    permisos: Vec<String>,
}

#[derive(Serialize)]
struct ApiErrorResponse {
    error: &'static str,
    mensaje: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum RequestAccessError {
    AuthenticationFailed,
    AuthenticationUnavailable,
    PrincipalDenied,
    PermissionDenied,
    AuthorizationDatabaseUnavailable,
    AuthorizationInvariantViolation,
}

impl IntoResponse for RequestAccessError {
    fn into_response(self) -> Response {
        match self {
            Self::AuthenticationFailed => StatusCode::UNAUTHORIZED,
            Self::AuthenticationUnavailable | Self::AuthorizationDatabaseUnavailable => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            Self::PrincipalDenied | Self::PermissionDenied => StatusCode::FORBIDDEN,
            Self::AuthorizationInvariantViolation => StatusCode::INTERNAL_SERVER_ERROR,
        }
        .into_response()
    }
}

#[derive(Clone, Copy, Debug)]
enum AdministrationRequestError {
    Access(RequestAccessError),
    Administration(access_administration::AccessAdministrationError),
}

impl From<RequestAccessError> for AdministrationRequestError {
    fn from(error: RequestAccessError) -> Self {
        Self::Access(error)
    }
}

impl From<access_administration::AccessAdministrationError> for AdministrationRequestError {
    fn from(error: access_administration::AccessAdministrationError) -> Self {
        Self::Administration(error)
    }
}

impl IntoResponse for AdministrationRequestError {
    fn into_response(self) -> Response {
        use access_administration::AccessAdministrationError as Error;

        match self {
            Self::Access(error) => error.into_response(),
            Self::Administration(error) => {
                let (status, code, message) = match error {
                    Error::InvalidInput => (
                        StatusCode::BAD_REQUEST,
                        "entrada_invalida",
                        "Los datos enviados no son válidos.",
                    ),
                    Error::NotFound => (
                        StatusCode::NOT_FOUND,
                        "no_encontrado",
                        "El recurso solicitado no existe.",
                    ),
                    Error::Conflict => (
                        StatusCode::CONFLICT,
                        "conflicto_acceso",
                        "La operación entra en conflicto con el estado de acceso actual.",
                    ),
                    Error::ExternalRejected => (
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "supabase_rechazo_operacion",
                        "Supabase no pudo crear o resolver ese usuario.",
                    ),
                    Error::ExternalUnavailable | Error::DatabaseUnavailable => (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "administracion_no_disponible",
                        "La administración de accesos no está disponible temporalmente.",
                    ),
                };
                (
                    status,
                    Json(ApiErrorResponse {
                        error: code,
                        mensaje: message,
                    }),
                )
                    .into_response()
            }
        }
    }
}

struct SupabaseBearerSecurity;

impl Modify for SupabaseBearerSecurity {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "supabaseBearer",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .build(),
                ),
            );
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Agro Ops API",
        version = env!("CARGO_PKG_VERSION"),
        description = "HTTP contract for Agro Ops backend health and system status endpoints."
    ),
    paths(health, ready, version, me, internal_worker_status, internal_system_status),
    components(schemas(StatusResponse, VersionResponse, MeResponse, SystemStatusResponse, ServiceStatus, ServiceStatusReport)),
    modifiers(&SupabaseBearerSecurity)
)]
struct ApiDoc;

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/version", get(version))
        .route("/me", get(me))
        .route(
            "/configuracion/usuarios",
            get(configuration_users).post(configuration_create_user),
        )
        .route(
            "/configuracion/usuarios/{user_id}",
            axum::routing::patch(configuration_update_user),
        )
        .route(
            "/configuracion/roles",
            get(configuration_roles).post(configuration_create_role),
        )
        .route(
            "/configuracion/roles/{role_id}",
            axum::routing::patch(configuration_update_role),
        )
        .route("/internal/worker/status", get(internal_worker_status))
        .route("/internal/system-status", get(internal_system_status))
        .route("/openapi.json", get(openapi_json))
        .layer(middleware::from_fn(request_tracing))
        .with_state(state)
}

async fn request_tracing(request: Request, next: Next) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let correlation_id =
        correlation_id(request.headers()).unwrap_or_else(|| Uuid::new_v4().to_string());
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let route = request.extensions().get::<MatchedPath>().map_or_else(
        || path.clone(),
        |matched_path| matched_path.as_str().to_owned(),
    );
    let started_at = Instant::now();
    let span = info_span!(
        "http_request",
        service = "api",
        request_id = %request_id,
        correlation_id = %correlation_id,
        method = %method,
        path = %path,
        route = %route,
        status = tracing::field::Empty,
        latency_ms = tracing::field::Empty,
    );

    info!(parent: &span, "request started");
    let mut response = next.run(request).instrument(span.clone()).await;
    let latency_ms = started_at.elapsed().as_millis() as u64;
    span.record("status", response.status().as_u16());
    span.record("latency_ms", latency_ms);
    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        HeaderValue::from_str(&request_id).expect("UUID request ID must be a valid header value"),
    );
    response.headers_mut().insert(
        CORRELATION_ID_HEADER,
        HeaderValue::from_str(&correlation_id)
            .expect("validated correlation ID must be a valid header value"),
    );
    info!(parent: &span, "request completed");

    response
}

fn correlation_id(headers: &HeaderMap) -> Option<String> {
    let correlation_id = headers.get(&CORRELATION_ID_HEADER)?.to_str().ok()?;

    (1..=MAX_CORRELATION_ID_LENGTH)
        .contains(&correlation_id.len())
        .then_some(correlation_id)
        .filter(|value| value.bytes().all(|byte| byte.is_ascii_graphic()))
        .map(str::to_owned)
}

/// Reports whether the API process is running.
#[utoipa::path(
    get,
    path = "/health",
    params(
        ("x-correlation-id" = Option<String>, Header, description = "Optional correlation identifier for a wider logical operation.")
    ),
    responses(
        (status = 200, description = "API is healthy.", body = StatusResponse,
            headers(
                ("x-request-id" = String, description = "Server-generated identifier for this HTTP request."),
                ("x-correlation-id" = String, description = "Correlation identifier supplied by the caller or generated by the server.")
            )
        )
    )
)]
async fn health() -> Json<StatusResponse> {
    Json(StatusResponse { status: "ok" })
}

/// Reports whether PostgreSQL is available to the API.
#[utoipa::path(
    get,
    path = "/ready",
    params(
        ("x-correlation-id" = Option<String>, Header, description = "Optional correlation identifier for a wider logical operation.")
    ),
    responses(
        (status = 200, description = "Database is available.", body = StatusResponse,
            headers(
                ("x-request-id" = String, description = "Server-generated identifier for this HTTP request."),
                ("x-correlation-id" = String, description = "Correlation identifier supplied by the caller or generated by the server.")
            )
        ),
        (status = 503, description = "Database is unavailable.", body = StatusResponse,
            headers(
                ("x-request-id" = String, description = "Server-generated identifier for this HTTP request."),
                ("x-correlation-id" = String, description = "Correlation identifier supplied by the caller or generated by the server.")
            )
        )
    )
)]
async fn ready(State(state): State<AppState>) -> (StatusCode, Json<StatusResponse>) {
    let status = database_status(&state.db).await;
    let status_code = if status.status == "ready" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (status_code, Json(status))
}

async fn database_status(db: &PgPool) -> StatusResponse {
    match sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(db).await {
        Ok(_) => StatusResponse { status: "ready" },
        Err(_) => StatusResponse {
            status: "not_ready",
        },
    }
}

/// Returns the running backend package identity and version.
#[utoipa::path(
    get,
    path = "/version",
    params(
        ("x-correlation-id" = Option<String>, Header, description = "Optional correlation identifier for a wider logical operation.")
    ),
    responses(
        (status = 200, description = "Backend version.", body = VersionResponse,
            headers(
                ("x-request-id" = String, description = "Server-generated identifier for this HTTP request."),
                ("x-correlation-id" = String, description = "Correlation identifier supplied by the caller or generated by the server.")
            )
        )
    )
)]
async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        service: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[utoipa::path(
    get,
    path = "/me",
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Current Agro Ops authorization context.", body = MeResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The Supabase identity is not enabled in Agro Ops."),
        (status = 503, description = "Authentication or authorization is unavailable.")
    )
)]
async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, RequestAccessError> {
    let context = resolve_request_context(&state, &headers).await?;
    Ok(Json(MeResponse {
        usuario_id: context.user_id.to_string(),
        organizacion_id: context.organization_id.to_string(),
        permisos: context.permission_codes().iter().cloned().collect(),
    }))
}

async fn configuration_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<access_administration::UsersResponse>, AdministrationRequestError> {
    let context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONFIGURACION_ADMINISTRAR,
    )
    .await?;
    Ok(Json(
        access_administration::list_users(
            &state.db,
            state.external_identity_admin.as_ref(),
            &context,
        )
        .await?,
    ))
}

async fn configuration_create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<access_administration::CreateUserRequest>,
) -> Result<(StatusCode, Json<access_administration::UserSummary>), AdministrationRequestError> {
    let context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONFIGURACION_ADMINISTRAR,
    )
    .await?;
    let user = access_administration::create_or_enable_user(
        &state.db,
        state.external_identity_admin.as_ref(),
        &context,
        &request,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(user)))
}

async fn configuration_update_user(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<access_administration::UpdateUserRequest>,
) -> Result<Json<access_administration::UserSummary>, AdministrationRequestError> {
    let context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONFIGURACION_ADMINISTRAR,
    )
    .await?;
    Ok(Json(
        access_administration::update_user(
            &state.db,
            state.external_identity_admin.as_ref(),
            &context,
            user_id,
            &request,
        )
        .await?,
    ))
}

async fn configuration_roles(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<access_administration::RolesResponse>, AdministrationRequestError> {
    let context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONFIGURACION_ADMINISTRAR,
    )
    .await?;
    Ok(Json(
        access_administration::list_roles(&state.db, &context).await?,
    ))
}

async fn configuration_create_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<access_administration::CreateRoleRequest>,
) -> Result<(StatusCode, Json<access_administration::RoleSummary>), AdministrationRequestError> {
    let context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONFIGURACION_ADMINISTRAR,
    )
    .await?;
    let role = access_administration::create_role(&state.db, &context, &request).await?;
    Ok((StatusCode::CREATED, Json(role)))
}

async fn configuration_update_role(
    State(state): State<AppState>,
    Path(role_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<access_administration::UpdateRoleRequest>,
) -> Result<Json<access_administration::RoleSummary>, AdministrationRequestError> {
    let context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONFIGURACION_ADMINISTRAR,
    )
    .await?;
    Ok(Json(
        access_administration::update_role(&state.db, &context, role_id, &request).await?,
    ))
}

/// Reports the persisted worker heartbeat status.
#[utoipa::path(
    get,
    path = "/internal/worker/status",
    params(
        ("x-correlation-id" = Option<String>, Header, description = "Optional correlation identifier for a wider logical operation.")
    ),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Worker status was read successfully.", body = ServiceStatusReport,
            headers(
                ("x-request-id" = String, description = "Server-generated identifier for this HTTP request."),
                ("x-correlation-id" = String, description = "Correlation identifier supplied by the caller or generated by the server.")
            )
        ),
        (status = 503, description = "Authentication or authorization dependency failures return an empty body. If authorization succeeds but the worker status cannot be read, the optional application/json body is a ServiceStatusReport.", body = Option<ServiceStatusReport>,
            headers(
                ("x-request-id" = String, description = "Server-generated identifier for this HTTP request."),
                ("x-correlation-id" = String, description = "Correlation identifier supplied by the caller or generated by the server.")
            )
        ),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The authenticated actor does not have access to the Internal Console.")
    )
)]
async fn internal_worker_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ServiceStatusReport>), RequestAccessError> {
    let _authorization_context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONSOLA_TECNICA_VER,
    )
    .await?;

    match service_heartbeats::worker_status(&state.db).await {
        Ok(report) => Ok((StatusCode::OK, Json(report))),
        Err(error) => {
            warn!(%error, "failed to read worker heartbeat");
            Ok((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ServiceStatusReport::unavailable(WORKER_SERVICE_NAME)),
            ))
        }
    }
}

/// Reports the API, database, worker, and version status used by the Internal
/// Console. This is deliberately private because it combines operational data.
#[utoipa::path(
    get,
    path = "/internal/system-status",
    params(
        ("x-correlation-id" = Option<String>, Header, description = "Optional correlation identifier for a wider logical operation.")
    ),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "System status was read successfully, including degraded dependencies.", body = SystemStatusResponse,
            headers(
                ("x-request-id" = String, description = "Server-generated identifier for this HTTP request."),
                ("x-correlation-id" = String, description = "Correlation identifier supplied by the caller or generated by the server.")
            )
        ),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The authenticated actor does not have access to the Internal Console."),
        (status = 503, description = "Authorization is unavailable.")
    )
)]
async fn internal_system_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SystemStatusResponse>, RequestAccessError> {
    let _authorization_context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONSOLA_TECNICA_VER,
    )
    .await?;

    let database = database_status(&state.db).await;
    let worker = match service_heartbeats::worker_status(&state.db).await {
        Ok(report) => report,
        Err(error) => {
            warn!(%error, "failed to read worker heartbeat");
            ServiceStatusReport::unavailable(WORKER_SERVICE_NAME)
        }
    };

    Ok(Json(SystemStatusResponse {
        api: StatusResponse { status: "ok" },
        database,
        worker,
        version: VersionResponse {
            service: env!("CARGO_PKG_NAME"),
            version: env!("CARGO_PKG_VERSION"),
        },
    }))
}

async fn authenticate_request(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<auth::AuthenticatedUser, RequestAccessError> {
    let access_token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or(RequestAccessError::AuthenticationFailed)?;
    state
        .auth
        .verify(access_token)
        .await
        .map_err(|error| match error {
            auth::VerifyAccessTokenError::Invalid => RequestAccessError::AuthenticationFailed,
            auth::VerifyAccessTokenError::Unavailable => {
                warn!("Supabase authentication verification is unavailable");
                RequestAccessError::AuthenticationUnavailable
            }
        })
}

async fn authorize_request(
    state: &AppState,
    headers: &HeaderMap,
    required_permission: &'static str,
) -> Result<authorization::AuthorizationContext, RequestAccessError> {
    let context = resolve_request_context(state, headers).await?;

    context
        .require_permission(required_permission)
        .map_err(|_| RequestAccessError::PermissionDenied)?;

    Ok(context)
}

async fn resolve_request_context(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<authorization::AuthorizationContext, RequestAccessError> {
    let authenticated_user = authenticate_request(state, headers).await?;
    let supabase_subject = Uuid::parse_str(&authenticated_user.id).map_err(|_| {
        warn!("Supabase authentication returned a non-UUID subject");
        RequestAccessError::AuthenticationFailed
    })?;
    let context = authorization::resolve_context(&state.db, supabase_subject)
        .await
        .map_err(|error| match error {
            authorization::ResolveAuthorizationError::PrincipalUnavailable => {
                RequestAccessError::PrincipalDenied
            }
            authorization::ResolveAuthorizationError::InvariantViolation => {
                error!("authorization identity cardinality invariant violated");
                RequestAccessError::AuthorizationInvariantViolation
            }
            authorization::ResolveAuthorizationError::DatabaseUnavailable(error) => {
                warn!(%error, "PostgreSQL authorization resolution is unavailable");
                RequestAccessError::AuthorizationDatabaseUnavailable
            }
        })?;

    Ok(context)
}

async fn openapi_json(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<utoipa::openapi::OpenApi>, RequestAccessError> {
    resolve_request_context(&state, &headers).await?;
    Ok(Json(ApiDoc::openapi()))
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use async_trait::async_trait;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode},
    };
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    struct TestAccessTokenVerifier;

    struct FixedExternalIdentityAdmin {
        subject: Uuid,
    }

    const VALID_TOKEN_PREFIX: &str = "valid-test-access-token:";

    fn worker_heartbeat_test_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[async_trait]
    impl auth::AccessTokenVerifier for TestAccessTokenVerifier {
        async fn verify(
            &self,
            access_token: &str,
        ) -> Result<auth::AuthenticatedUser, auth::VerifyAccessTokenError> {
            if access_token == "valid-test-access-token" {
                return Ok(auth::AuthenticatedUser {
                    id: Uuid::nil().to_string(),
                });
            }

            let subject = access_token
                .strip_prefix(VALID_TOKEN_PREFIX)
                .filter(|subject| Uuid::parse_str(subject).is_ok())
                .ok_or(auth::VerifyAccessTokenError::Invalid)?;
            Ok(auth::AuthenticatedUser {
                id: subject.to_owned(),
            })
        }
    }

    #[async_trait]
    impl supabase_admin::ExternalIdentityAdmin for FixedExternalIdentityAdmin {
        async fn resolve_or_invite(
            &self,
            _email: &str,
            _full_name: &str,
        ) -> Result<supabase_admin::ExternalAuthUser, supabase_admin::ExternalIdentityAdminError>
        {
            Ok(supabase_admin::ExternalAuthUser {
                subject: self.subject,
            })
        }

        async fn correos_electronicos_por_sujeto(
            &self,
            subjects: &[Uuid],
        ) -> Result<
            std::collections::HashMap<Uuid, String>,
            supabase_admin::ExternalIdentityAdminError,
        > {
            Ok(subjects
                .iter()
                .map(|subject| (*subject, format!("{subject}@example.com")))
                .collect())
        }
    }

    fn test_auth() -> Arc<dyn auth::AccessTokenVerifier> {
        Arc::new(TestAccessTokenVerifier)
    }

    fn valid_token(subject: Uuid) -> String {
        format!("{VALID_TOKEN_PREFIX}{subject}")
    }

    async fn available_state() -> AppState {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for PostgreSQL tests");
        let db = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("PostgreSQL must be available for integration tests");

        AppState {
            db,
            auth: test_auth(),
            external_identity_admin: Arc::new(supabase_admin::UnavailableExternalIdentityAdmin),
        }
    }

    fn unavailable_state() -> AppState {
        let db = PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(50))
            .connect_lazy("postgres://agro_ops:agro_ops@127.0.0.1:1/agro_ops")
            .expect("unavailable test database URL must be valid");

        AppState {
            db,
            auth: test_auth(),
            external_identity_admin: Arc::new(supabase_admin::UnavailableExternalIdentityAdmin),
        }
    }

    async fn provision_internal_console_access(db: &PgPool, subject: Uuid) {
        provision_internal_access(
            db,
            subject,
            authorization::permission_codes::CONSOLA_TECNICA_VER,
        )
        .await;
    }

    async fn provision_internal_access(db: &PgPool, subject: Uuid, permission_code: &str) {
        let organization_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();
        let role_id = Uuid::new_v4();
        let role_assignment_id = Uuid::new_v4();
        let permission_grant_id = Uuid::new_v4();
        let unique_name = Uuid::new_v4().to_string();

        sqlx::query("INSERT INTO public.organizaciones (id, nombre) VALUES ($1, $2)")
            .bind(organization_id)
            .bind(format!("Organizacion test {unique_name}"))
            .execute(db)
            .await
            .expect("test organization must insert");
        sqlx::query("INSERT INTO public.usuarios (id, organizacion_id, nombre_completo) VALUES ($1, $2, $3)")
            .bind(user_id)
            .bind(organization_id)
            .bind(format!("Usuario test {unique_name}"))
            .execute(db)
            .await
            .expect("test user must insert");
        sqlx::query("INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, 'supabase', $2)")
            .bind(user_id)
            .bind(subject)
            .execute(db)
            .await
            .expect("test external identity must insert");
        sqlx::query("INSERT INTO public.roles (id, organizacion_id, nombre) VALUES ($1, $2, $3)")
            .bind(role_id)
            .bind(organization_id)
            .bind(format!("Rol test {unique_name}"))
            .execute(db)
            .await
            .expect("test role must insert");
        sqlx::query("INSERT INTO public.usuarios_roles (id, usuario_id, rol_id, vigente_desde) VALUES ($1, $2, $3, CURRENT_TIMESTAMP - INTERVAL '1 minute')")
            .bind(role_assignment_id)
            .bind(user_id)
            .bind(role_id)
            .execute(db)
            .await
            .expect("test role assignment must insert");
        sqlx::query("INSERT INTO public.roles_permisos (id, rol_id, permiso_id, vigente_desde) SELECT $1, $2, id, CURRENT_TIMESTAMP - INTERVAL '1 minute' FROM public.permisos WHERE codigo = $3")
            .bind(permission_grant_id)
            .bind(role_id)
            .bind(permission_code)
            .execute(db)
            .await
            .expect("test permission grant must insert");
    }

    async fn response_body(response: axum::response::Response) -> String {
        let bytes = to_bytes(response.into_body(), 128 * 1024)
            .await
            .expect("response body must be readable");
        String::from_utf8(bytes.to_vec()).expect("response body must be UTF-8")
    }

    #[tokio::test]
    async fn health_remains_available_without_database() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("health request must be valid"),
            )
            .await
            .expect("health request must succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_body(response).await, r#"{"status":"ok"}"#);
    }

    #[tokio::test]
    async fn version_returns_package_metadata_and_request_identity_headers() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/version")
                    .header("x-correlation-id", "version-test-correlation")
                    .body(Body::empty())
                    .expect("version request must be valid"),
            )
            .await
            .expect("version request must succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert!(
            Uuid::parse_str(
                response.headers()["x-request-id"]
                    .to_str()
                    .expect("request ID must be valid ASCII")
            )
            .is_ok()
        );
        assert_eq!(
            response.headers()["x-correlation-id"],
            HeaderValue::from_static("version-test-correlation")
        );

        let body: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("version response must be valid JSON");
        assert_eq!(body["service"], env!("CARGO_PKG_NAME"));
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn openapi_document_exposes_current_api_contract_and_request_identity_headers() {
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;
        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .header("x-correlation-id", "openapi-test-correlation")
                    .body(Body::empty())
                    .expect("OpenAPI request must be valid"),
            )
            .await
            .expect("OpenAPI request must succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert!(
            Uuid::parse_str(
                response.headers()["x-request-id"]
                    .to_str()
                    .expect("request ID must be valid ASCII")
            )
            .is_ok()
        );
        assert_eq!(
            response.headers()["x-correlation-id"],
            HeaderValue::from_static("openapi-test-correlation")
        );

        let document: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("OpenAPI response must be valid JSON");
        assert!(
            document["openapi"]
                .as_str()
                .is_some_and(|version| version.starts_with("3."))
        );
        assert_eq!(document["info"]["title"], "Agro Ops API");
        assert_eq!(document["info"]["version"], env!("CARGO_PKG_VERSION"));

        for path in [
            "/health",
            "/ready",
            "/version",
            "/me",
            "/internal/worker/status",
            "/internal/system-status",
        ] {
            assert!(document["paths"][path]["get"].is_object());
        }

        assert!(document["components"]["schemas"]["StatusResponse"].is_object());
        assert!(document["components"]["schemas"]["VersionResponse"].is_object());
        assert!(document["components"]["schemas"]["ServiceStatusReport"].is_object());
        assert_eq!(
            document["components"]["securitySchemes"]["supabaseBearer"]["type"],
            "http"
        );
        assert_eq!(
            document["components"]["securitySchemes"]["supabaseBearer"]["scheme"],
            "bearer"
        );
        assert_eq!(
            document["components"]["securitySchemes"]["supabaseBearer"]["bearerFormat"],
            "JWT"
        );
        assert_eq!(
            document["paths"]["/me"]["get"]["security"][0]["supabaseBearer"],
            serde_json::json!([])
        );
        assert_eq!(
            document["paths"]["/internal/worker/status"]["get"]["security"][0]["supabaseBearer"],
            serde_json::json!([])
        );
        assert_eq!(
            document["paths"]["/internal/system-status"]["get"]["security"][0]["supabaseBearer"],
            serde_json::json!([])
        );
        for path in ["/internal/worker/status", "/internal/system-status"] {
            assert!(document["paths"][path]["get"]["responses"]["401"].is_object());
            assert!(document["paths"][path]["get"]["responses"]["403"].is_object());
            assert!(document["paths"][path]["get"]["responses"]["503"].is_object());
        }
        let worker_status_503 =
            &document["paths"]["/internal/worker/status"]["get"]["responses"]["503"];
        let worker_status_503_description = worker_status_503["description"]
            .as_str()
            .expect("worker status 503 description must be present");
        assert!(worker_status_503_description.contains("empty body"));
        assert!(worker_status_503_description.contains("optional application/json body"));
        assert!(worker_status_503_description.contains("ServiceStatusReport"));
        let worker_status_503_schema = &worker_status_503["content"]["application/json"]["schema"];
        assert!(
            worker_status_503_schema["oneOf"]
                .as_array()
                .is_some_and(|schemas| schemas
                    .iter()
                    .any(|schema| schema["$ref"] == "#/components/schemas/ServiceStatusReport")),
            "worker status 503 must retain the ServiceStatusReport JSON representation"
        );
        assert!(
            worker_status_503_schema["oneOf"]
                .as_array()
                .is_some_and(|schemas| schemas.iter().any(|schema| schema["type"] == "null")),
            "worker status 503 JSON representation must be optional for empty authorization failures"
        );
        for path in ["/health", "/ready", "/version"] {
            assert!(document["paths"][path]["get"].get("security").is_none());
        }
        assert_eq!(
            document["paths"]["/health"]["get"]["responses"]["200"]["content"]["application/json"]
                ["schema"]["$ref"],
            "#/components/schemas/StatusResponse"
        );

        let health_parameters = document["paths"]["/health"]["get"]["parameters"]
            .as_array()
            .expect("health parameters must be an array");
        let correlation_parameter = health_parameters
            .iter()
            .find(|parameter| parameter["name"] == "x-correlation-id")
            .expect("correlation header must be documented");
        assert_eq!(correlation_parameter["in"], "header");
        assert_ne!(correlation_parameter["required"], true);

        let response_headers = &document["paths"]["/health"]["get"]["responses"]["200"]["headers"];
        assert!(response_headers["x-request-id"].is_object());
        assert!(response_headers["x-correlation-id"].is_object());
    }

    #[tokio::test]
    async fn health_response_contains_generated_request_and_correlation_ids() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("health request must be valid"),
            )
            .await
            .expect("health request must succeed");

        assert!(response.headers().contains_key("x-request-id"));
        assert!(response.headers().contains_key("x-correlation-id"));
        let request_id = response.headers()["x-request-id"]
            .to_str()
            .expect("generated request ID must be valid ASCII");
        let correlation_id = response.headers()["x-correlation-id"]
            .to_str()
            .expect("generated correlation ID must be valid ASCII");
        assert!(Uuid::parse_str(request_id).is_ok());
        assert!(Uuid::parse_str(correlation_id).is_ok());
    }

    #[tokio::test]
    async fn supplied_correlation_id_is_propagated_unchanged() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header("x-correlation-id", "test-correlation-123")
                    .body(Body::empty())
                    .expect("health request must be valid"),
            )
            .await
            .expect("health request must succeed");

        assert_eq!(
            response.headers()["x-correlation-id"],
            HeaderValue::from_static("test-correlation-123")
        );
        assert_ne!(
            response.headers()["x-request-id"],
            response.headers()["x-correlation-id"]
        );
    }

    #[tokio::test]
    async fn each_request_receives_a_distinct_request_id() {
        let first = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("first health request must be valid"),
            )
            .await
            .expect("first health request must succeed");
        let second = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .expect("second health request must be valid"),
            )
            .await
            .expect("second health request must succeed");

        assert_ne!(
            first.headers()["x-request-id"],
            second.headers()["x-request-id"]
        );
    }

    #[tokio::test]
    async fn invalid_or_oversized_correlation_id_is_replaced_with_a_generated_id() {
        for invalid_correlation_id in [
            "not a valid correlation ID".to_owned(),
            "a".repeat(MAX_CORRELATION_ID_LENGTH + 1),
        ] {
            let response = app(unavailable_state())
                .oneshot(
                    Request::builder()
                        .uri("/health")
                        .header(
                            HeaderName::from_static("x-correlation-id"),
                            invalid_correlation_id,
                        )
                        .body(Body::empty())
                        .expect("health request must be valid"),
                )
                .await
                .expect("health request must succeed");

            let correlation_id = response.headers()["x-correlation-id"]
                .to_str()
                .expect("generated correlation ID must be valid ASCII");
            assert!(Uuid::parse_str(correlation_id).is_ok());
        }
    }

    #[tokio::test]
    async fn ready_remains_unavailable_without_database() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .expect("ready request must be valid"),
            )
            .await
            .expect("ready request must succeed");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response_body(response).await, r#"{"status":"not_ready"}"#);
    }

    #[tokio::test]
    async fn ready_remains_available_with_database() {
        let response = app(available_state().await)
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .expect("ready request must be valid"),
            )
            .await
            .expect("ready request must succeed");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_body(response).await, r#"{"status":"ready"}"#);
    }

    #[tokio::test]
    async fn internal_worker_status_fails_closed_when_authorization_database_is_unavailable() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/internal/worker/status")
                    .header("authorization", "Bearer valid-test-access-token")
                    .body(Body::empty())
                    .expect("worker status request must be valid"),
            )
            .await
            .expect("worker status request must succeed");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response_body(response).await, "");
    }

    #[tokio::test]
    async fn internal_worker_status_has_machine_readable_healthy_response() {
        let _guard = worker_heartbeat_test_lock().lock().await;
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;
        service_heartbeats::record_service_heartbeat(&state.db, WORKER_SERVICE_NAME)
            .await
            .expect("worker heartbeat must be writable");

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/internal/worker/status")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("worker status request must be valid"),
            )
            .await
            .expect("worker status request must succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_body(response).await;
        assert!(body.contains(r#""service":"worker""#));
        assert!(body.contains(r#""status":"healthy""#));
        assert!(body.contains(r#""last_seen_at":""#));
    }

    #[tokio::test]
    async fn internal_worker_status_requires_a_valid_access_token() {
        for authorization in [
            None,
            Some("not-a-bearer-token"),
            Some("Bearer invalid-token"),
        ] {
            let mut request = Request::builder().uri("/internal/worker/status");
            if let Some(authorization) = authorization {
                request = request.header("authorization", authorization);
            }
            let response = app(unavailable_state())
                .oneshot(
                    request
                        .body(Body::empty())
                        .expect("worker status request must be valid"),
                )
                .await
                .expect("worker status request must succeed");

            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn internal_system_status_reports_healthy_api_database_and_worker() {
        let _guard = worker_heartbeat_test_lock().lock().await;
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;
        service_heartbeats::record_service_heartbeat(&state.db, WORKER_SERVICE_NAME)
            .await
            .expect("worker heartbeat must be writable");

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/internal/system-status")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("system status request must be valid"),
            )
            .await
            .expect("system status request must succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("system status response must be valid JSON");
        assert_eq!(body["api"]["status"], "ok");
        assert_eq!(body["database"]["status"], "ready");
        assert_eq!(body["worker"]["status"], "healthy");
        assert_eq!(body["version"]["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(
            body.as_object()
                .expect("system status must be an object")
                .keys()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from(["api", "database", "version", "worker"])
        );
    }

    #[tokio::test]
    async fn internal_system_status_reports_a_stale_worker() {
        let _guard = worker_heartbeat_test_lock().lock().await;
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;
        service_heartbeats::record_service_heartbeat(&state.db, WORKER_SERVICE_NAME)
            .await
            .expect("worker heartbeat must be writable");
        sqlx::query(
            "UPDATE service_heartbeats SET last_seen_at = CURRENT_TIMESTAMP - INTERVAL '16 seconds' WHERE service_name = $1",
        )
        .bind(WORKER_SERVICE_NAME)
        .execute(&state.db)
        .await
        .expect("worker heartbeat must be made stale");

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/internal/system-status")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("system status request must be valid"),
            )
            .await
            .expect("system status request must succeed");

        let body: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("system status response must be valid JSON");
        assert_eq!(body["worker"]["status"], "stale");
        assert!(body["worker"]["last_seen_at"].is_string());
    }

    #[tokio::test]
    async fn internal_system_status_reports_an_unavailable_worker_without_a_heartbeat() {
        let _guard = worker_heartbeat_test_lock().lock().await;
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;
        sqlx::query("DELETE FROM service_heartbeats WHERE service_name = $1")
            .bind(WORKER_SERVICE_NAME)
            .execute(&state.db)
            .await
            .expect("worker heartbeat must be removable");

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/internal/system-status")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("system status request must be valid"),
            )
            .await
            .expect("system status request must succeed");

        let body: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("system status response must be valid JSON");
        assert_eq!(body["database"]["status"], "ready");
        assert_eq!(body["worker"]["status"], "unavailable");
        assert!(body["worker"]["last_seen_at"].is_null());
    }

    #[tokio::test]
    async fn internal_system_status_fails_closed_when_authorization_database_is_unavailable() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/internal/system-status")
                    .header("authorization", "Bearer valid-test-access-token")
                    .body(Body::empty())
                    .expect("system status request must be valid"),
            )
            .await
            .expect("system status request must succeed");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response_body(response).await, "");
    }

    #[tokio::test]
    async fn internal_system_status_requires_a_valid_access_token() {
        for authorization in [
            None,
            Some("not-a-bearer-token"),
            Some("Bearer invalid-token"),
        ] {
            let mut request = Request::builder().uri("/internal/system-status");
            if let Some(authorization) = authorization {
                request = request.header("authorization", authorization);
            }
            let response = app(unavailable_state())
                .oneshot(
                    request
                        .body(Body::empty())
                        .expect("system status request must be valid"),
                )
                .await
                .expect("system status request must succeed");

            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn openapi_document_is_not_public() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .body(Body::empty())
                    .expect("OpenAPI request must be valid"),
            )
            .await
            .expect("OpenAPI request must succeed");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn openapi_rejects_a_valid_unprovisioned_supabase_identity() {
        let state = available_state().await;
        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .header(
                        "authorization",
                        format!("Bearer {}", valid_token(Uuid::new_v4())),
                    )
                    .body(Body::empty())
                    .expect("OpenAPI request must be valid"),
            )
            .await
            .expect("OpenAPI request must succeed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn openapi_reports_authorization_dependency_unavailability() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .header("authorization", "Bearer valid-test-access-token")
                    .body(Body::empty())
                    .expect("OpenAPI request must be valid"),
            )
            .await
            .expect("OpenAPI request must succeed");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn posting_an_already_linked_subject_returns_conflict_without_mutation() {
        let mut state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_access(
            &state.db,
            subject,
            authorization::permission_codes::CONFIGURACION_ADMINISTRAR,
        )
        .await;
        state.external_identity_admin = Arc::new(FixedExternalIdentityAdmin { subject });
        let existing: (Uuid, String) = sqlx::query_as(
            r#"
            SELECT rol.id, usuario.nombre_completo
            FROM public.identidades_autenticacion_externas AS identidad
            JOIN public.usuarios AS usuario ON usuario.id = identidad.usuario_id
            JOIN public.usuarios_roles AS usuario_rol ON usuario_rol.usuario_id = usuario.id
            JOIN public.roles AS rol ON rol.id = usuario_rol.rol_id
            WHERE identidad.proveedor = 'supabase'
              AND identidad.sujeto_proveedor = $1
              AND tstzrange(usuario_rol.vigente_desde, usuario_rol.vigente_hasta, '[)')
                  @> statement_timestamp()
            "#,
        )
        .bind(subject)
        .fetch_one(&state.db)
        .await
        .expect("existing linked administrator must be queryable");
        let body = serde_json::json!({
            "correo_electronico": "linked@example.com",
            "nombre_completo": "Mutated through POST",
            "roles_ids": [existing.0],
        });

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/configuracion/usuarios")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .expect("configuration user request must be valid"),
            )
            .await
            .expect("configuration user request must complete");

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let stored_name: String = sqlx::query_scalar(
            r#"
            SELECT usuario.nombre_completo
            FROM public.identidades_autenticacion_externas AS identidad
            JOIN public.usuarios AS usuario ON usuario.id = identidad.usuario_id
            WHERE identidad.proveedor = 'supabase' AND identidad.sujeto_proveedor = $1
            "#,
        )
        .bind(subject)
        .fetch_one(&state.db)
        .await
        .expect("linked user must remain queryable");
        assert_eq!(stored_name, existing.1);
    }

    #[tokio::test]
    async fn me_requires_a_valid_access_token() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .body(Body::empty())
                    .expect("me request must be valid"),
            )
            .await
            .expect("me request must succeed");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn me_rejects_a_valid_unprovisioned_supabase_identity() {
        let state = available_state().await;
        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header(
                        "authorization",
                        format!("Bearer {}", valid_token(Uuid::new_v4())),
                    )
                    .body(Body::empty())
                    .expect("me request must be valid"),
            )
            .await
            .expect("me request must succeed");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn me_returns_the_enabled_users_deduplicated_authorization_context() {
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("me request must be valid"),
            )
            .await
            .expect("me request must succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("me response must be valid JSON");
        assert!(Uuid::parse_str(body["usuario_id"].as_str().expect("user id")).is_ok());
        assert!(
            Uuid::parse_str(body["organizacion_id"].as_str().expect("organization id")).is_ok()
        );
        assert_eq!(
            body["permisos"],
            serde_json::json!([authorization::permission_codes::CONSOLA_TECNICA_VER])
        );
        assert!(body.get("roles").is_none());
        assert!(body.get("sujeto_proveedor").is_none());
    }

    #[tokio::test]
    async fn me_reports_authorization_dependency_unavailability() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/me")
                    .header("authorization", "Bearer valid-test-access-token")
                    .body(Body::empty())
                    .expect("me request must be valid"),
            )
            .await
            .expect("me request must succeed");

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn configuration_user_list_remains_available_without_email_enrichment() {
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_access(
            &state.db,
            subject,
            authorization::permission_codes::CONFIGURACION_ADMINISTRAR,
        )
        .await;

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/configuracion/usuarios")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("configuration user request must be valid"),
            )
            .await
            .expect("configuration user request must complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("configuration user response must be valid JSON");
        let users = body["usuarios"]
            .as_array()
            .expect("configuration users must be an array");
        assert_eq!(users.len(), 1);
        assert!(users[0]["correo_electronico"].is_null());
        assert_eq!(users[0]["activo"], true);
        assert_eq!(users[0]["roles"].as_array().map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn access_management_requires_configuration_administration_permission() {
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;

        for (method, uri, body) in [
            (Method::GET, "/configuracion/usuarios", ""),
            (
                Method::POST,
                "/configuracion/usuarios",
                r#"{"correo_electronico":"x@example.com","nombre_completo":"X","roles_ids":[]}"#,
            ),
            (
                Method::PATCH,
                "/configuracion/usuarios/00000000-0000-0000-0000-000000000001",
                r#"{"activo":false}"#,
            ),
            (Method::GET, "/configuracion/roles", ""),
            (
                Method::POST,
                "/configuracion/roles",
                r#"{"nombre":"X","permisos":[]}"#,
            ),
            (
                Method::PATCH,
                "/configuracion/roles/00000000-0000-0000-0000-000000000001",
                r#"{"activo":false}"#,
            ),
        ] {
            let response = app(state.clone())
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .method(method)
                        .header("authorization", format!("Bearer {}", valid_token(subject)))
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .expect("configuration request must be valid"),
                )
                .await
                .expect("configuration request must succeed");

            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
    }
}
