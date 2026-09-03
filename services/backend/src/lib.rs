use std::time::Instant;

use axum::{
    Json, Router,
    extract::{MatchedPath, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::get,
};
use serde::Serialize;
use sqlx::PgPool;
use tracing::{Instrument, info, info_span, warn};
use uuid::Uuid;

pub mod service_heartbeats;
pub mod telemetry;
pub mod worker;

use service_heartbeats::{ServiceStatusReport, WORKER_SERVICE_NAME};

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");
const CORRELATION_ID_HEADER: HeaderName = HeaderName::from_static("x-correlation-id");
const MAX_CORRELATION_ID_LENGTH: usize = 128;

#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct VersionResponse {
    service: &'static str,
    version: &'static str,
}

pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/version", get(version))
        .route("/internal/worker/status", get(internal_worker_status))
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

async fn health() -> Json<StatusResponse> {
    Json(StatusResponse { status: "ok" })
}

async fn ready(State(state): State<AppState>) -> (StatusCode, Json<StatusResponse>) {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
    {
        Ok(_) => (StatusCode::OK, Json(StatusResponse { status: "ready" })),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(StatusResponse {
                status: "not_ready",
            }),
        ),
    }
}

async fn version() -> Json<VersionResponse> {
    Json(VersionResponse {
        service: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn internal_worker_status(
    State(state): State<AppState>,
) -> (StatusCode, Json<ServiceStatusReport>) {
    match service_heartbeats::worker_status(&state.db).await {
        Ok(report) => (StatusCode::OK, Json(report)),
        Err(error) => {
            warn!(%error, "failed to read worker heartbeat");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ServiceStatusReport::unavailable(WORKER_SERVICE_NAME)),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    async fn available_state() -> AppState {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for PostgreSQL tests");
        let db = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("PostgreSQL must be available for integration tests");

        AppState { db }
    }

    fn unavailable_state() -> AppState {
        let db = PgPoolOptions::new()
            .acquire_timeout(std::time::Duration::from_millis(50))
            .connect_lazy("postgres://agro_ops:agro_ops@127.0.0.1:1/agro_ops")
            .expect("unavailable test database URL must be valid");

        AppState { db }
    }

    async fn response_body(response: axum::response::Response) -> String {
        let bytes = to_bytes(response.into_body(), 1024)
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
}
