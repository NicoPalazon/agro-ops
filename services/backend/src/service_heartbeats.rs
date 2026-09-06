use serde::Serialize;
use sqlx::PgPool;
use utoipa::ToSchema;

pub const WORKER_SERVICE_NAME: &str = "worker";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    Healthy,
    Stale,
    Unavailable,
}

#[derive(Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct ServiceStatusReport {
    pub service: String,
    pub status: ServiceStatus,
    pub last_seen_at: Option<String>,
}

impl ServiceStatusReport {
    pub fn unavailable(service: &str) -> Self {
        Self {
            service: service.to_string(),
            status: ServiceStatus::Unavailable,
            last_seen_at: None,
        }
    }
}

pub async fn record_service_heartbeat(db: &PgPool, service_name: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO service_heartbeats (service_name, last_seen_at)
        VALUES ($1, CURRENT_TIMESTAMP)
        ON CONFLICT (service_name)
        DO UPDATE SET last_seen_at = CURRENT_TIMESTAMP
        "#,
    )
    .bind(service_name)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn service_status(
    db: &PgPool,
    service_name: &str,
) -> Result<ServiceStatusReport, sqlx::Error> {
    let heartbeat = sqlx::query_as::<_, (bool, String)>(
        r#"
        SELECT
            last_seen_at >= CURRENT_TIMESTAMP - INTERVAL '15 seconds' AS is_healthy,
            to_char(
                last_seen_at AT TIME ZONE 'UTC',
                'YYYY-MM-DD"T"HH24:MI:SS.US"Z"'
            ) AS last_seen_at
        FROM service_heartbeats
        WHERE service_name = $1
        "#,
    )
    .bind(service_name)
    .fetch_optional(db)
    .await?;

    Ok(match heartbeat {
        Some((is_healthy, last_seen_at)) => ServiceStatusReport {
            service: service_name.to_string(),
            status: if is_healthy {
                ServiceStatus::Healthy
            } else {
                ServiceStatus::Stale
            },
            last_seen_at: Some(last_seen_at),
        },
        None => ServiceStatusReport::unavailable(service_name),
    })
}

pub async fn worker_status(db: &PgPool) -> Result<ServiceStatusReport, sqlx::Error> {
    service_status(db, WORKER_SERVICE_NAME).await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use sqlx::postgres::PgPoolOptions;

    use super::*;

    static SERVICE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for PostgreSQL tests");

        PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("PostgreSQL must be available for integration tests")
    }

    fn unique_service_name(test_name: &str) -> String {
        let sequence = SERVICE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        format!("test-{test_name}-{}-{sequence}", std::process::id())
    }

    async fn remove_service(db: &PgPool, service_name: &str) {
        sqlx::query("DELETE FROM service_heartbeats WHERE service_name = $1")
            .bind(service_name)
            .execute(db)
            .await
            .expect("test heartbeat cleanup must succeed");
    }

    #[tokio::test]
    async fn inserts_worker_heartbeat() {
        let db = test_pool().await;
        let service_name = unique_service_name("insert");

        record_service_heartbeat(&db, &service_name)
            .await
            .expect("heartbeat insert must succeed");

        let (row_count, is_recent) = sqlx::query_as::<_, (i64, bool)>(
            r#"
            SELECT
                COUNT(*)::BIGINT,
                bool_and(last_seen_at >= CURRENT_TIMESTAMP - INTERVAL '5 seconds')
            FROM service_heartbeats
            WHERE service_name = $1
            "#,
        )
        .bind(&service_name)
        .fetch_one(&db)
        .await
        .expect("inserted heartbeat must be readable");

        assert_eq!(row_count, 1);
        assert!(is_recent);
        remove_service(&db, &service_name).await;
    }

    #[tokio::test]
    async fn updates_worker_heartbeat_with_upsert() {
        let db = test_pool().await;
        let service_name = unique_service_name("upsert");

        record_service_heartbeat(&db, &service_name)
            .await
            .expect("initial heartbeat insert must succeed");

        sqlx::query(
            "UPDATE service_heartbeats SET last_seen_at = CURRENT_TIMESTAMP - INTERVAL '1 hour' WHERE service_name = $1",
        )
        .bind(&service_name)
        .execute(&db)
        .await
        .expect("test heartbeat must become stale");

        record_service_heartbeat(&db, &service_name)
            .await
            .expect("heartbeat upsert must succeed");

        let (row_count, is_recent) = sqlx::query_as::<_, (i64, bool)>(
            r#"
            SELECT
                COUNT(*)::BIGINT,
                bool_and(last_seen_at >= CURRENT_TIMESTAMP - INTERVAL '5 seconds')
            FROM service_heartbeats
            WHERE service_name = $1
            "#,
        )
        .bind(&service_name)
        .fetch_one(&db)
        .await
        .expect("updated heartbeat must be readable");

        assert_eq!(row_count, 1);
        assert!(is_recent);
        remove_service(&db, &service_name).await;
    }

    #[tokio::test]
    async fn reports_healthy_status_for_recent_heartbeat() {
        let db = test_pool().await;
        let service_name = unique_service_name("healthy");

        record_service_heartbeat(&db, &service_name)
            .await
            .expect("heartbeat insert must succeed");

        let report = service_status(&db, &service_name)
            .await
            .expect("heartbeat status must be readable");

        assert_eq!(report.status, ServiceStatus::Healthy);
        assert!(report.last_seen_at.is_some());
        remove_service(&db, &service_name).await;
    }

    #[tokio::test]
    async fn reports_stale_status_for_old_heartbeat() {
        let db = test_pool().await;
        let service_name = unique_service_name("stale");

        sqlx::query(
            r#"
            INSERT INTO service_heartbeats (service_name, last_seen_at)
            VALUES ($1, CURRENT_TIMESTAMP - INTERVAL '16 seconds')
            "#,
        )
        .bind(&service_name)
        .execute(&db)
        .await
        .expect("stale heartbeat insert must succeed");

        let report = service_status(&db, &service_name)
            .await
            .expect("heartbeat status must be readable");

        assert_eq!(report.status, ServiceStatus::Stale);
        assert!(report.last_seen_at.is_some());
        remove_service(&db, &service_name).await;
    }

    #[tokio::test]
    async fn reports_unavailable_when_heartbeat_is_missing() {
        let db = test_pool().await;
        let service_name = unique_service_name("missing");

        let report = service_status(&db, &service_name)
            .await
            .expect("missing heartbeat lookup must succeed");

        assert_eq!(
            report,
            ServiceStatusReport {
                service: service_name,
                status: ServiceStatus::Unavailable,
                last_seen_at: None,
            }
        );
    }
}
