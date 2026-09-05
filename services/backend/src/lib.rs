use std::{sync::Arc, time::Instant};

use axum::{
    Json, Router,
    extract::{MatchedPath, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::get,
};
use serde::Serialize;
use sqlx::PgPool;
use tracing::{Instrument, info, info_span, warn};
use utoipa::{Modify, OpenApi, ToSchema};
use uuid::Uuid;

pub mod auth;
pub mod config;
pub mod service_heartbeats;
pub mod shutdown;
pub mod telemetry;
pub mod worker;

use service_heartbeats::{ServiceStatus, ServiceStatusReport, WORKER_SERVICE_NAME};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub auth: Arc<dyn auth::AccessTokenVerifier>,
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
    paths(health, ready, version, internal_worker_status, internal_system_status),
    components(schemas(StatusResponse, VersionResponse, SystemStatusResponse, ServiceStatus, ServiceStatusReport)),
    modifiers(&SupabaseBearerSecurity)
)]
struct ApiDoc;

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/version", get(version))
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
        (status = 503, description = "Worker status could not be read.", body = ServiceStatusReport,
            headers(
                ("x-request-id" = String, description = "Server-generated identifier for this HTTP request."),
                ("x-correlation-id" = String, description = "Correlation identifier supplied by the caller or generated by the server.")
            )
        ),
        (status = 401, description = "A valid Supabase user access token is required.")
    )
)]
async fn internal_worker_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<ServiceStatusReport>), StatusCode> {
    require_authenticated_user(&state, &headers).await?;

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
        (status = 401, description = "A valid Supabase user access token is required.")
    )
)]
async fn internal_system_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SystemStatusResponse>, StatusCode> {
    require_authenticated_user(&state, &headers).await?;

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

async fn require_authenticated_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), StatusCode> {
    let access_token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let _user = state
        .auth
        .verify(access_token)
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    Ok(())
}

async fn openapi_json(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<utoipa::openapi::OpenApi>, StatusCode> {
    require_authenticated_user(&state, &headers).await?;
    Ok(Json(ApiDoc::openapi()))
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    struct TestAccessTokenVerifier;

    #[async_trait]
    impl auth::AccessTokenVerifier for TestAccessTokenVerifier {
        async fn verify(
            &self,
            access_token: &str,
        ) -> Result<auth::AuthenticatedUser, auth::VerifyAccessTokenError> {
            (access_token == "valid-test-access-token")
                .then_some(auth::AuthenticatedUser {
                    id: "test-user".to_owned(),
                })
                .ok_or(auth::VerifyAccessTokenError::Invalid)
        }
    }

    fn test_auth() -> Arc<dyn auth::AccessTokenVerifier> {
        Arc::new(TestAccessTokenVerifier)
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
        }
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
        let response = app(unavailable_state())
            .oneshot(
                Request::builder()
                    .uri("/openapi.json")
                    .header("authorization", "Bearer valid-test-access-token")
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
            document["paths"]["/internal/worker/status"]["get"]["security"][0]["supabaseBearer"],
            serde_json::json!([])
        );
        assert_eq!(
            document["paths"]["/internal/system-status"]["get"]["security"][0]["supabaseBearer"],
            serde_json::json!([])
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
    async fn internal_worker_status_has_machine_readable_unavailable_response() {
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
        assert_eq!(
            response_body(response).await,
            r#"{"service":"worker","status":"unavailable","last_seen_at":null}"#
        );
    }

    #[tokio::test]
    async fn internal_worker_status_has_machine_readable_healthy_response() {
        let state = available_state().await;
        service_heartbeats::record_service_heartbeat(&state.db, WORKER_SERVICE_NAME)
            .await
            .expect("worker heartbeat must be writable");

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/internal/worker/status")
                    .header("authorization", "Bearer valid-test-access-token")
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
        let state = available_state().await;
        service_heartbeats::record_service_heartbeat(&state.db, WORKER_SERVICE_NAME)
            .await
            .expect("worker heartbeat must be writable");

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/internal/system-status")
                    .header("authorization", "Bearer valid-test-access-token")
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
        let state = available_state().await;
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
                    .header("authorization", "Bearer valid-test-access-token")
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
        let state = available_state().await;
        sqlx::query("DELETE FROM service_heartbeats WHERE service_name = $1")
            .bind(WORKER_SERVICE_NAME)
            .execute(&state.db)
            .await
            .expect("worker heartbeat must be removable");

        let response = app(state)
            .oneshot(
                Request::builder()
                    .uri("/internal/system-status")
                    .header("authorization", "Bearer valid-test-access-token")
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
    async fn internal_system_status_reports_degraded_database_and_unavailable_worker() {
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

        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_str(&response_body(response).await)
            .expect("system status response must be valid JSON");
        assert_eq!(body["database"]["status"], "not_ready");
        assert_eq!(body["worker"]["status"], "unavailable");
        assert!(body["worker"]["last_seen_at"].is_null());
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
}
