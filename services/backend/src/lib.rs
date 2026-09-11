use std::{sync::Arc, time::Instant};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, MatchedPath, Multipart, Path, Query, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use sqlx::PgPool;
use tracing::{Instrument, error, info, info_span, warn};
use utoipa::{Modify, OpenApi, ToSchema};
use uuid::Uuid;

pub mod access_administration;
pub mod access_provisioning;
pub mod audit;
pub mod auth;
pub mod authorization;
pub mod config;
pub mod documents;
pub mod external_references;
pub mod idempotency;
pub mod jobs;
pub mod outbox;
pub mod service_heartbeats;
pub mod shutdown;
pub mod supabase_admin;
pub mod supabase_storage;
pub mod telemetry;
pub mod territory;
pub mod worker;

use service_heartbeats::{ServiceStatus, ServiceStatusReport, WORKER_SERVICE_NAME};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub auth: Arc<dyn auth::AccessTokenVerifier>,
    pub external_identity_admin: Arc<dyn supabase_admin::ExternalIdentityAdmin>,
    pub document_storage: Arc<dyn documents::DocumentStorage>,
    pub document_settings: documents::DocumentSettings,
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
pub(crate) enum RequestAccessError {
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

#[derive(Debug)]
enum JobsRequestError {
    Access(RequestAccessError),
    Database(sqlx::Error),
}

#[derive(Debug)]
enum OutboxRequestError {
    Access(RequestAccessError),
    Store(outbox::OutboxStoreError),
}

#[derive(Debug)]
enum AuditRequestError {
    Access(RequestAccessError),
    Database(sqlx::Error),
}

#[derive(Debug)]
enum IdempotencyRequestError {
    Access(RequestAccessError),
    Database(sqlx::Error),
}

#[derive(Debug)]
enum DocumentRequestError {
    Access(RequestAccessError),
    BadRequest,
    PayloadTooLarge,
    NotFound,
    StorageUnavailable,
    Database(sqlx::Error),
}

impl From<RequestAccessError> for DocumentRequestError {
    fn from(error: RequestAccessError) -> Self {
        Self::Access(error)
    }
}

impl IntoResponse for DocumentRequestError {
    fn into_response(self) -> Response {
        match self {
            Self::Access(error) => error.into_response(),
            Self::BadRequest => (
                StatusCode::BAD_REQUEST,
                Json(ApiErrorResponse {
                    error: "documento_invalido",
                    mensaje: "El archivo enviado no es válido.",
                }),
            )
                .into_response(),
            Self::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(ApiErrorResponse {
                    error: "documento_demasiado_grande",
                    mensaje: "El archivo supera el tamaño máximo permitido.",
                }),
            )
                .into_response(),
            Self::NotFound => StatusCode::NOT_FOUND.into_response(),
            Self::StorageUnavailable => StatusCode::SERVICE_UNAVAILABLE.into_response(),
            Self::Database(error) => {
                warn!(%error, "document metadata operation failed");
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
        }
    }
}

impl From<RequestAccessError> for OutboxRequestError {
    fn from(error: RequestAccessError) -> Self {
        Self::Access(error)
    }
}

impl IntoResponse for OutboxRequestError {
    fn into_response(self) -> Response {
        match self {
            Self::Access(error) => error.into_response(),
            Self::Store(error) => {
                warn!(%error, "failed to read outbox diagnostics");
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
        }
    }
}

impl From<RequestAccessError> for JobsRequestError {
    fn from(error: RequestAccessError) -> Self {
        Self::Access(error)
    }
}

impl IntoResponse for JobsRequestError {
    fn into_response(self) -> Response {
        match self {
            Self::Access(error) => error.into_response(),
            Self::Database(error) => {
                warn!(%error, "failed to read job diagnostics");
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
        }
    }
}

impl From<RequestAccessError> for AuditRequestError {
    fn from(error: RequestAccessError) -> Self {
        Self::Access(error)
    }
}

impl IntoResponse for AuditRequestError {
    fn into_response(self) -> Response {
        match self {
            Self::Access(error) => error.into_response(),
            Self::Database(error) => {
                warn!(%error, "failed to read audit diagnostics");
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
        }
    }
}

impl From<RequestAccessError> for IdempotencyRequestError {
    fn from(error: RequestAccessError) -> Self {
        Self::Access(error)
    }
}

impl IntoResponse for IdempotencyRequestError {
    fn into_response(self) -> Response {
        match self {
            Self::Access(error) => error.into_response(),
            Self::Database(error) => {
                warn!(%error, "failed to read idempotency diagnostics");
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
        }
    }
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
        description = "HTTP contract for Agro Ops public health and protected transversal endpoints."
    ),
    paths(health, ready, version, me, internal_worker_status, internal_system_status, internal_jobs, internal_outbox, internal_audit, internal_idempotencia, create_document, document_metadata, document_download, territory::api::list_establecimientos, territory::api::get_establecimiento, territory::api::list_campanas, territory::api::get_campana, territory::api::list_unidades_operativas, territory::api::list_usos_unidad_operativa, territory::api::preview_senasa_polygon, territory::api::confirm_senasa_geographic_source, territory::api::list_geographic_sources, territory::api::set_canonical_source_contributors),
    components(schemas(StatusResponse, VersionResponse, MeResponse, SystemStatusResponse, ServiceStatus, ServiceStatusReport, jobs::JobState, jobs::JobDiagnostic, jobs::JobsDiagnosticsResponse, outbox::OutboxDiagnostic, outbox::OutboxDiagnosticsResponse, audit::AuditDiagnostic, audit::AuditDiagnosticsResponse, idempotency::IdempotencyDiagnostic, idempotency::IdempotencyDiagnosticsResponse, documents::DocumentMetadata, documents::CreatedDocument, documents::DocumentDownloadResponse, territory::api::EstablishmentResponse, territory::api::BasePlotResponse, territory::api::ExternalReferenceResponse, territory::api::GeoJsonMultiPolygonResponse, territory::api::CampaignResponse, territory::api::OperationalUnitResponse, territory::api::TerritorialUseAssignmentResponse, territory::api::SenasaPolygonPreviewRequest, territory::api::SenasaPolygonPreviewResponse, territory::api::TerritorialValidationErrorResponse, territory::api::ConfirmSenasaGeographicSourceRequest, territory::api::GeographicSourceResponse, territory::api::ConfirmedGeographicSourceResponse, territory::api::SetCanonicalSourceContributorsRequest, territory::api::CanonicalSourceGroupingResponse)),
    modifiers(&SupabaseBearerSecurity)
)]
struct ApiDoc;

pub fn app(state: AppState) -> Router {
    let document_body_limit = state
        .document_settings
        .max_upload_bytes()
        .saturating_add(128 * 1024);
    Router::new()
        .merge(territory::api::routes())
        .route("/documentos", post(create_document))
        .route("/documentos/{document_id}", get(document_metadata))
        .route("/documentos/{document_id}/descarga", get(document_download))
        .route_layer(DefaultBodyLimit::max(document_body_limit))
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
        .route("/internal/jobs", get(internal_jobs))
        .route("/internal/outbox", get(internal_outbox))
        .route("/internal/audit", get(internal_audit))
        .route("/internal/idempotencia", get(internal_idempotencia))
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
    let user = access_administration::create_user(
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

/// Lists bounded, payload-free PostgreSQL queue diagnostics.
#[utoipa::path(
    get,
    path = "/internal/jobs",
    params(jobs::JobDiagnosticsQuery),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Bounded job diagnostics without payloads.", body = jobs::JobsDiagnosticsResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The authenticated actor does not have access to the Internal Console."),
        (status = 503, description = "Authorization or job diagnostics are unavailable.")
    )
)]
async fn internal_jobs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<jobs::JobDiagnosticsQuery>,
) -> Result<Json<jobs::JobsDiagnosticsResponse>, JobsRequestError> {
    let _authorization_context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONSOLA_TECNICA_VER,
    )
    .await?;

    jobs::list_diagnostics(&state.db, &query)
        .await
        .map(Json)
        .map_err(|error| match error {
            jobs::JobStoreError::Database(error) => JobsRequestError::Database(error),
            jobs::JobStoreError::InvalidBatchSize | jobs::JobStoreError::InvalidStaleThreshold => {
                unreachable!("diagnostic bounds are normalized before querying")
            }
        })
}

/// Lists bounded, payload-free transactional outbox diagnostics.
#[utoipa::path(
    get,
    path = "/internal/outbox",
    params(outbox::OutboxDiagnosticsQuery),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Bounded outbox and linked delivery job diagnostics without payloads.", body = outbox::OutboxDiagnosticsResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The authenticated actor does not have access to the Internal Console."),
        (status = 503, description = "Authorization or outbox diagnostics are unavailable.")
    )
)]
async fn internal_outbox(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<outbox::OutboxDiagnosticsQuery>,
) -> Result<Json<outbox::OutboxDiagnosticsResponse>, OutboxRequestError> {
    let _authorization_context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONSOLA_TECNICA_VER,
    )
    .await?;

    outbox::list_diagnostics(&state.db, &query)
        .await
        .map(Json)
        .map_err(OutboxRequestError::Store)
}

/// Lists bounded, organization-scoped audit metadata without snapshots.
#[utoipa::path(
    get,
    path = "/internal/audit",
    params(audit::AuditDiagnosticsQuery),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Bounded organization-scoped audit metadata without snapshots.", body = audit::AuditDiagnosticsResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The authenticated actor does not have access to the Internal Console."),
        (status = 503, description = "Authorization or audit diagnostics are unavailable.")
    )
)]
async fn internal_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<audit::AuditDiagnosticsQuery>,
) -> Result<Json<audit::AuditDiagnosticsResponse>, AuditRequestError> {
    let authorization_context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONSOLA_TECNICA_VER,
    )
    .await?;

    audit::list_diagnostics(&state.db, authorization_context.organization_id, &query)
        .await
        .map(Json)
        .map_err(AuditRequestError::Database)
}

/// Lists bounded, organization-scoped idempotency metadata without replay results.
#[utoipa::path(
    get,
    path = "/internal/idempotencia",
    params(idempotency::IdempotencyDiagnosticsQuery),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Bounded organization-scoped idempotency metadata without replay results.", body = idempotency::IdempotencyDiagnosticsResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The authenticated actor does not have access to the Internal Console."),
        (status = 503, description = "Authorization or idempotency diagnostics are unavailable.")
    )
)]
async fn internal_idempotencia(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<idempotency::IdempotencyDiagnosticsQuery>,
) -> Result<Json<idempotency::IdempotencyDiagnosticsResponse>, IdempotencyRequestError> {
    let authorization_context = authorize_request(
        &state,
        &headers,
        authorization::permission_codes::CONSOLA_TECNICA_VER,
    )
    .await?;

    idempotency::list_diagnostics(&state.db, authorization_context.organization_id, &query)
        .await
        .map(Json)
        .map_err(IdempotencyRequestError::Database)
}

/// Stores a private document for the authenticated actor's organization.
#[utoipa::path(
    post,
    path = "/documentos",
    request_body(content = String, content_type = "multipart/form-data", description = "One `archivo` file field."),
    security(("supabaseBearer" = [])),
    responses(
        (status = 201, description = "Private document metadata was created.", body = documents::CreatedDocument),
        (status = 400, description = "The multipart file is invalid."),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The authenticated identity is not an active Agro Ops user."),
        (status = 413, description = "The file exceeds the configured maximum size."),
        (status = 503, description = "Storage or metadata persistence is unavailable.")
    )
)]
async fn create_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<documents::CreatedDocument>), DocumentRequestError> {
    let context = resolve_request_context(&state, &headers).await?;
    let upload =
        read_document_upload(&mut multipart, state.document_settings.max_upload_bytes()).await?;
    let created = documents::create(
        &state.db,
        &state.document_storage,
        &state.document_settings,
        &context,
        upload,
    )
    .await
    .map_err(map_document_error)?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// Returns safe metadata for a private document in the caller's organization.
#[utoipa::path(
    get,
    path = "/documentos/{document_id}",
    params(("document_id" = String, Path, description = "Documento UUID")),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Safe document metadata.", body = documents::DocumentMetadata),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 404, description = "The document is not in the caller organization."),
        (status = 503, description = "Metadata storage is unavailable.")
    )
)]
async fn document_metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<Uuid>,
) -> Result<Json<documents::DocumentMetadata>, DocumentRequestError> {
    let context = resolve_request_context(&state, &headers).await?;
    documents::metadata(&state.db, context.organization_id, document_id)
        .await
        .map(Json)
        .map_err(map_document_error)
}

/// Creates short-lived private download access for a document in the caller's organization.
#[utoipa::path(
    get,
    path = "/documentos/{document_id}/descarga",
    params(("document_id" = String, Path, description = "Documento UUID")),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Short-lived private download access.", body = documents::DocumentDownloadResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 404, description = "The document is not in the caller organization."),
        (status = 503, description = "Private Storage is unavailable.")
    )
)]
async fn document_download(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(document_id): Path<Uuid>,
) -> Result<Json<documents::DocumentDownloadResponse>, DocumentRequestError> {
    let context = resolve_request_context(&state, &headers).await?;
    documents::download_access(
        &state.db,
        &state.document_storage,
        context.organization_id,
        document_id,
    )
    .await
    .map(Json)
    .map_err(map_document_error)
}

async fn read_document_upload(
    multipart: &mut Multipart,
    maximum_size: usize,
) -> Result<documents::DocumentUpload, DocumentRequestError> {
    let upload = {
        let field = multipart
            .next_field()
            .await
            .map_err(|_| DocumentRequestError::BadRequest)?
            .ok_or(DocumentRequestError::BadRequest)?;
        if field.name() != Some("archivo") {
            return Err(DocumentRequestError::BadRequest);
        }
        let filename = field
            .file_name()
            .map(str::to_owned)
            .ok_or(DocumentRequestError::BadRequest)?;
        let mime_type = field
            .content_type()
            .map(str::to_owned)
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        let mut collector = documents::DocumentUploadCollector::new(
            filename,
            mime_type,
            maximum_size,
        )
        .map_err(|error| match error {
            documents::DocumentUploadError::TooLarge => DocumentRequestError::PayloadTooLarge,
            _ => DocumentRequestError::BadRequest,
        })?;
        let mut field = field;
        while let Some(chunk) = field
            .chunk()
            .await
            .map_err(|_| DocumentRequestError::BadRequest)?
        {
            collector.push_chunk(&chunk).map_err(|error| match error {
                documents::DocumentUploadError::TooLarge => DocumentRequestError::PayloadTooLarge,
                _ => DocumentRequestError::BadRequest,
            })?;
        }
        collector.finish().map_err(|error| match error {
            documents::DocumentUploadError::TooLarge => DocumentRequestError::PayloadTooLarge,
            _ => DocumentRequestError::BadRequest,
        })?
    };
    if multipart
        .next_field()
        .await
        .map_err(|_| DocumentRequestError::BadRequest)?
        .is_some()
    {
        return Err(DocumentRequestError::BadRequest);
    }
    Ok(upload)
}

fn map_document_error(error: documents::DocumentError) -> DocumentRequestError {
    match error {
        documents::DocumentError::NotFound => DocumentRequestError::NotFound,
        documents::DocumentError::StorageUnavailable => DocumentRequestError::StorageUnavailable,
        documents::DocumentError::Database(error) => DocumentRequestError::Database(error),
    }
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

pub(crate) async fn resolve_request_context(
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
            document_storage: Arc::new(documents::UnavailableDocumentStorage),
            document_settings: documents::DocumentSettings::new("documentos_privados", 1024),
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
            document_storage: Arc::new(documents::UnavailableDocumentStorage),
            document_settings: documents::DocumentSettings::new("documentos_privados", 1024),
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
            "/internal/jobs",
            "/internal/outbox",
            "/internal/audit",
            "/internal/idempotencia",
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
        for path in [
            "/internal/worker/status",
            "/internal/system-status",
            "/internal/jobs",
            "/internal/outbox",
            "/internal/audit",
            "/internal/idempotencia",
        ] {
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
    async fn internal_jobs_requires_a_valid_access_token() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/internal/jobs")
                    .body(Body::empty())
                    .expect("jobs request must be valid"),
            )
            .await
            .expect("jobs request must complete");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn internal_jobs_rejects_authenticated_users_without_technical_permission() {
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
                    .uri("/internal/jobs")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("jobs request must be valid"),
            )
            .await
            .expect("jobs request must complete");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn internal_jobs_returns_bounded_filtered_metadata_without_payloads_or_secrets() {
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;
        let job_id = Uuid::new_v4();
        let diagnostic_job_type = "test.internal_jobs_diagnostic";
        let secret = "private-access-token-must-not-leak";

        // Remove only fixtures owned by this test. Jobs are otherwise shared
        // across the suite and the endpoint intentionally has global scope.
        sqlx::query(
            "DELETE FROM public.jobs WHERE tipo IN ('test.internal_diagnostic', 'test.internal_jobs_diagnostic')",
        )
        .execute(&state.db)
        .await
        .expect("prior jobs diagnostic fixtures must be removable");
        sqlx::query(
            r#"
            INSERT INTO public.jobs (
                id, tipo, estado, payload, intentos, max_intentos,
                next_attempt_at, actualizado_en, ultimo_error
            )
            VALUES (
                $1, $2, 'agotado',
                jsonb_build_object('authorization', $3::text), 1, 1,
                statement_timestamp(), TIMESTAMPTZ '9999-12-31 23:59:59+00', 'Resumen seguro'
            )
            "#,
        )
        .bind(job_id)
        .bind(diagnostic_job_type)
        .bind(secret)
        .execute(&state.db)
        .await
        .expect("diagnostic test job must insert");

        // Internal Jobs intentionally offers only state and bound filters. A
        // terminal fixture timestamp makes this owned row the deterministic
        // newest match without assuming the shared database has no other jobs.

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/internal/jobs?estado=agotado&limite=1")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("jobs request must be valid"),
            )
            .await
            .expect("jobs request must complete");

        assert_eq!(response.status(), StatusCode::OK);
        let raw_body = response_body(response).await;
        assert!(!raw_body.contains(secret));
        assert!(!raw_body.contains("payload"));
        assert!(!raw_body.contains("authorization"));
        let body: serde_json::Value =
            serde_json::from_str(&raw_body).expect("jobs response must be valid JSON");
        let jobs = body["jobs"].as_array().expect("jobs must be an array");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0]["id"], job_id.to_string());
        assert_eq!(jobs[0]["tipo"], diagnostic_job_type);
        assert_eq!(jobs[0]["estado"], "agotado");
        assert_eq!(jobs[0]["intentos"], 1);
        assert_eq!(jobs[0]["max_intentos"], 1);
        assert_eq!(jobs[0]["ultimo_error"], "Resumen seguro");
        assert!(jobs[0].get("payload").is_none());
        assert!(jobs[0].get("organizacion_id").is_none());
        assert!(jobs[0].get("credentials").is_none());
        assert!(jobs[0]["bloqueado_por"].is_null());
    }

    #[tokio::test]
    async fn internal_outbox_requires_authentication_and_technical_permission() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/internal/outbox")
                    .body(Body::empty())
                    .expect("outbox request must be valid"),
            )
            .await
            .expect("outbox request must complete");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

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
                    .uri("/internal/outbox")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("outbox request must be valid"),
            )
            .await
            .expect("outbox request must complete");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn internal_outbox_joins_filters_and_bounds_safe_metadata_without_payloads() {
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;
        let destination = format!("diagnostic_{}", &Uuid::new_v4().simple().to_string()[..12]);
        let secret = "private-outbox-payload-secret";
        let mut transaction = state.db.begin().await.expect("transaction must begin");
        let recorded = outbox::record(
            &mut transaction,
            outbox::NewOutboxEvent {
                organization_id: None,
                destination: outbox::OutboxDestination::new(&destination)
                    .expect("destination must be canonical"),
                event_type: outbox::OutboxEventType::new("test.internal_diagnostic")
                    .expect("event type must be canonical"),
                entity_type: Some(
                    outbox::OutboxEntityType::new("test.entity")
                        .expect("entity type must be canonical"),
                ),
                entity_id: Some(Uuid::new_v4()),
                reference: Some("OUTBOX-DIAGNOSTIC".to_owned()),
                idempotency_key: outbox::IdempotencyKey::new(format!(
                    "diagnostic-{}",
                    Uuid::new_v4()
                ))
                .expect("idempotency key must be valid"),
                payload: serde_json::json!({"authorization": secret}),
                delivery_max_attempts: 1,
                delivery_next_attempt_at: time::OffsetDateTime::now_utc(),
            },
        )
        .await
        .expect("diagnostic event must record");
        transaction.commit().await.expect("transaction must commit");

        let worker_id = Uuid::new_v4();
        jobs::claim(
            &state.db,
            worker_id,
            &[outbox::OutboxDestination::new(&destination)
                .expect("destination must be canonical")
                .delivery_job_type()],
            1,
        )
        .await
        .expect("diagnostic delivery must claim");
        jobs::mark_failed(
            &state.db,
            recorded.job_id,
            worker_id,
            time::OffsetDateTime::now_utc(),
            &jobs::SafeErrorSummary::new("Resumen seguro").expect("error summary must be safe"),
        )
        .await
        .expect("diagnostic delivery must exhaust");

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/internal/outbox?estado=agotado&destino={destination}&evento_tipo=test.internal_diagnostic&limite=1"
                    ))
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("outbox request must be valid"),
            )
            .await
            .expect("outbox request must complete");

        assert_eq!(response.status(), StatusCode::OK);
        let raw_body = response_body(response).await;
        assert!(!raw_body.contains(secret));
        assert!(!raw_body.contains("payload"));
        assert!(!raw_body.contains("authorization"));
        assert!(!raw_body.contains("credentials"));
        let body: serde_json::Value =
            serde_json::from_str(&raw_body).expect("outbox response must be valid JSON");
        let events = body["eventos"]
            .as_array()
            .expect("eventos must be an array");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], recorded.event_id.to_string());
        assert_eq!(events[0]["destino"], destination);
        assert_eq!(events[0]["evento_tipo"], "test.internal_diagnostic");
        assert_eq!(events[0]["estado"], "agotado");
        assert_eq!(events[0]["intentos"], 1);
        assert_eq!(events[0]["max_intentos"], 1);
        assert_eq!(events[0]["ultimo_error"], "Resumen seguro");
        assert!(events[0].get("payload").is_none());
    }

    #[tokio::test]
    async fn internal_audit_requires_authentication_and_technical_permission() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/internal/audit")
                    .body(Body::empty())
                    .expect("audit request must be valid"),
            )
            .await
            .expect("audit request must complete");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

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
                    .uri("/internal/audit")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("audit request must be valid"),
            )
            .await
            .expect("audit request must complete");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn internal_audit_returns_bounded_filtered_organization_scoped_metadata() {
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;
        let (organization_id, actor_user_id): (Uuid, Uuid) = sqlx::query_as(
            "SELECT u.organizacion_id, u.id FROM public.usuarios AS u INNER JOIN public.identidades_autenticacion_externas AS i ON i.usuario_id = u.id WHERE i.proveedor = 'supabase' AND i.sujeto_proveedor = $1",
        )
        .bind(subject)
        .fetch_one(&state.db)
        .await
        .expect("technical actor must exist");
        let older_id = Uuid::new_v4();
        let newer_id = Uuid::new_v4();
        let secret = "private-audit-snapshot-secret";
        for (id, action, occurred_at) in [
            (
                older_id,
                "diagnostico.anterior",
                time::OffsetDateTime::now_utc() - time::Duration::minutes(2),
            ),
            (
                newer_id,
                "diagnostico.reciente",
                time::OffsetDateTime::now_utc() - time::Duration::minutes(1),
            ),
        ] {
            sqlx::query(
                "INSERT INTO public.audit_events (id, organizacion_id, actor_tipo, actor_usuario_id, accion, entidad_tipo, entidad_id, referencia, estado_anterior, estado_posterior, ocurrido_en) VALUES ($1, $2, 'usuario', $3, $4, 'diagnostico_entidad', $5, 'AUDIT-DIAGNOSTIC', jsonb_build_object('authorization', $6::text), jsonb_build_object('authorization', $6::text), $7)",
            )
            .bind(id)
            .bind(organization_id)
            .bind(actor_user_id)
            .bind(action)
            .bind(Uuid::new_v4())
            .bind(secret)
            .bind(occurred_at)
            .execute(&state.db)
            .await
            .expect("audit diagnostic event must insert");
        }

        let other_subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, other_subject).await;
        let (other_organization_id, other_actor_user_id): (Uuid, Uuid) = sqlx::query_as(
            "SELECT u.organizacion_id, u.id FROM public.usuarios AS u INNER JOIN public.identidades_autenticacion_externas AS i ON i.usuario_id = u.id WHERE i.proveedor = 'supabase' AND i.sujeto_proveedor = $1",
        )
        .bind(other_subject)
        .fetch_one(&state.db)
        .await
        .expect("other technical actor must exist");
        sqlx::query(
            "INSERT INTO public.audit_events (organizacion_id, actor_tipo, actor_usuario_id, accion, entidad_tipo) VALUES ($1, 'usuario', $2, 'diagnostico.reciente', 'diagnostico_entidad')",
        )
        .bind(other_organization_id)
        .bind(other_actor_user_id)
        .execute(&state.db)
        .await
        .expect("other organization audit event must insert");

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .uri("/internal/audit?entidad_tipo=diagnostico_entidad&limite=50")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("audit request must be valid"),
            )
            .await
            .expect("audit request must complete");

        assert_eq!(response.status(), StatusCode::OK);
        let raw_body = response_body(response).await;
        assert!(!raw_body.contains(secret));
        assert!(!raw_body.contains("authorization"));
        let body: serde_json::Value =
            serde_json::from_str(&raw_body).expect("audit response must be valid JSON");
        let events = body["eventos"]
            .as_array()
            .expect("eventos must be an array");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["id"], newer_id.to_string());
        assert_eq!(events[1]["id"], older_id.to_string());
        assert_eq!(events[0]["accion"], "diagnostico.reciente");
        assert_eq!(events[0]["actor_usuario_id"], actor_user_id.to_string());
        assert_eq!(events[0]["tiene_estado_anterior"], true);
        assert_eq!(events[0]["tiene_estado_posterior"], true);
        assert!(events[0].get("estado_anterior").is_none());
        assert!(events[0].get("estado_posterior").is_none());
        assert!(events[0].get("organizacion_id").is_none());

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/internal/audit?entidad_tipo=diagnostico_entidad&actor_usuario_id={actor_user_id}&limite=1"
                    ))
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("bounded audit request must be valid"),
            )
            .await
            .expect("bounded audit request must complete");
        let body: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("bounded audit response must be valid JSON");
        assert_eq!(
            body["eventos"]
                .as_array()
                .expect("eventos must be an array")
                .len(),
            1
        );

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/internal/audit?accion=diagnostico.anterior&limite=50")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("filtered audit request must be valid"),
            )
            .await
            .expect("filtered audit request must complete");
        let body: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("filtered audit response must be valid JSON");
        let events = body["eventos"]
            .as_array()
            .expect("eventos must be an array");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["id"], older_id.to_string());
    }

    #[tokio::test]
    async fn internal_idempotency_requires_authentication_and_technical_permission() {
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/internal/idempotencia")
                    .body(Body::empty())
                    .expect("idempotency request must be valid"),
            )
            .await
            .expect("idempotency request must complete");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

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
                    .uri("/internal/idempotencia")
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("idempotency request must be valid"),
            )
            .await
            .expect("idempotency request must complete");
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn internal_idempotency_returns_bounded_filtered_organization_scoped_metadata() {
        let state = available_state().await;
        let subject = Uuid::new_v4();
        provision_internal_console_access(&state.db, subject).await;
        let organization_id: Uuid = sqlx::query_scalar(
            "SELECT u.organizacion_id FROM public.usuarios AS u INNER JOIN public.identidades_autenticacion_externas AS i ON i.usuario_id = u.id WHERE i.proveedor = 'supabase' AND i.sujeto_proveedor = $1",
        )
        .bind(subject)
        .fetch_one(&state.db)
        .await
        .expect("technical organization must exist");
        let secret = "private-idempotency-result-secret";
        let diagnostic_suffix = Uuid::new_v4().simple().to_string();
        let operation = format!("diagnostico.comando_{diagnostic_suffix}");
        let older_key = format!("diagnostic-old-key-{diagnostic_suffix}");
        let newer_key = format!("diagnostic-new-key-{diagnostic_suffix}");
        let global_key = format!("global-diagnostic-key-{diagnostic_suffix}");
        let older_id = Uuid::new_v4();
        let newer_id = Uuid::new_v4();
        for (id, key, completed_at, fingerprint) in [
            (
                older_id,
                older_key.as_str(),
                time::OffsetDateTime::now_utc() - time::Duration::minutes(2),
                vec![17_u8; 32],
            ),
            (
                newer_id,
                newer_key.as_str(),
                time::OffsetDateTime::now_utc() - time::Duration::minutes(1),
                vec![42_u8; 32],
            ),
        ] {
            sqlx::query(
                "INSERT INTO public.idempotency_records (id, organizacion_id, operacion, idempotency_key, request_sha256, resultado, completado_en) VALUES ($1, $2, $3, $4, $5, jsonb_build_object('secret', $6::text), $7)",
            )
            .bind(id)
            .bind(organization_id)
            .bind(&operation)
            .bind(key)
            .bind(fingerprint)
            .bind(secret)
            .bind(completed_at)
            .execute(&state.db)
            .await
            .expect("idempotency diagnostic record must insert");
        }
        sqlx::query(
            "INSERT INTO public.idempotency_records (organizacion_id, operacion, idempotency_key, request_sha256, resultado) VALUES (NULL, $1, $2, $3, jsonb_build_object('secret', $4::text))",
        )
        .bind(&operation)
        .bind(&global_key)
        .bind(vec![99_u8; 32])
        .bind(secret)
        .execute(&state.db)
        .await
        .expect("global idempotency record must insert");

        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/internal/idempotencia?operacion={operation}&limite=1"
                    ))
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("idempotency request must be valid"),
            )
            .await
            .expect("idempotency request must complete");

        assert_eq!(response.status(), StatusCode::OK);
        let raw_body = response_body(response).await;
        assert!(!raw_body.contains(secret));
        assert!(!raw_body.contains(&global_key));
        let body: serde_json::Value =
            serde_json::from_str(&raw_body).expect("idempotency response must be valid JSON");
        let records = body["registros"]
            .as_array()
            .expect("registros must be an array");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["id"], newer_id.to_string());
        assert_eq!(records[0]["request_sha256"], "2a".repeat(32));
        assert!(
            records[0]["resultado_bytes"]
                .as_i64()
                .is_some_and(|size| size > 0)
        );
        assert!(records[0].get("resultado").is_none());
        assert!(records[0].get("organizacion_id").is_none());

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/internal/idempotencia?operacion={operation}&idempotency_key={older_key}&limite=50"
                    ))
                    .header("authorization", format!("Bearer {}", valid_token(subject)))
                    .body(Body::empty())
                    .expect("idempotency request must be valid"),
            )
            .await
            .expect("idempotency key filter request must complete");
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("filtered idempotency response must be valid JSON");
        let records = body["registros"]
            .as_array()
            .expect("registros must be an array");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["id"], older_id.to_string());
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
        let caller_user_id: Uuid = sqlx::query_scalar(
            "SELECT usuario_id FROM public.identidades_autenticacion_externas WHERE proveedor = 'supabase' AND sujeto_proveedor = $1",
        )
        .bind(subject)
        .fetch_one(&state.db)
        .await
        .expect("unauthorized caller must be queryable");
        let audits_before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM public.audit_events WHERE actor_usuario_id = $1",
        )
        .bind(caller_user_id)
        .fetch_one(&state.db)
        .await
        .expect("audit count must be queryable");

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
        let audits_after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::bigint FROM public.audit_events WHERE actor_usuario_id = $1",
        )
        .bind(caller_user_id)
        .fetch_one(&state.db)
        .await
        .expect("audit count must be queryable");
        assert_eq!(audits_after, audits_before);
    }
}
