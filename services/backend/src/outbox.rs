use std::{fmt, sync::Arc};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    jobs::{self, JobState, JobType, NewJob},
    worker::{DuplicateJobType, JobDispatcher, JobExecutionResult, JobHandler},
};

pub const MAX_DIAGNOSTIC_LIMIT: u16 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxDestination(String);

impl OutboxDestination {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidOutboxCode> {
        let value = value.into();
        if value.len() > 64 || !is_canonical_part(&value) {
            return Err(InvalidOutboxCode);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn delivery_job_type(&self) -> JobType {
        JobType::new(format!("outbox.dispatch.{}", self.0))
            .expect("a validated outbox destination always yields a valid job type")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxEventType(String);

impl OutboxEventType {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidOutboxCode> {
        let value = value.into();
        if value.len() > 128 || !is_dotted_canonical_code(&value) {
            return Err(InvalidOutboxCode);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxEntityType(String);

impl OutboxEntityType {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidOutboxCode> {
        let value = value.into();
        if value.len() > 128 || !is_canonical_code(&value) {
            return Err(InvalidOutboxCode);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidOutboxText> {
        let value = value.into();
        if value.trim() != value || value.is_empty() || value.chars().count() > 256 {
            return Err(InvalidOutboxText);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidOutboxCode;

impl fmt::Display for InvalidOutboxCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("outbox code must be bounded and canonical")
    }
}

impl std::error::Error for InvalidOutboxCode {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidOutboxText;

impl fmt::Display for InvalidOutboxText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("outbox text must be trimmed, nonblank, and at most 256 characters")
    }
}

impl std::error::Error for InvalidOutboxText {}

fn is_canonical_part(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn is_canonical_code(value: &str) -> bool {
    !value.is_empty() && value.split('.').all(is_canonical_part)
}

fn is_dotted_canonical_code(value: &str) -> bool {
    value.contains('.') && is_canonical_code(value)
}

/// Immutable integration envelope to persist with a caller-owned transaction.
///
/// Payloads must be explicitly constructed and must never contain credentials,
/// authorization headers, private keys, connection strings, or access tokens.
#[derive(Clone, Debug)]
pub struct NewOutboxEvent {
    pub organization_id: Option<Uuid>,
    pub destination: OutboxDestination,
    pub event_type: OutboxEventType,
    pub entity_type: Option<OutboxEntityType>,
    pub entity_id: Option<Uuid>,
    pub reference: Option<String>,
    pub idempotency_key: IdempotencyKey,
    pub payload: Value,
    pub delivery_max_attempts: i32,
    pub delivery_next_attempt_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedOutboxEvent {
    pub event_id: Uuid,
    pub job_id: Uuid,
}

#[derive(Debug)]
pub enum OutboxStoreError {
    IdempotencyConflict,
    InvalidReference,
    InvariantViolation,
    Jobs(jobs::JobStoreError),
    Database(sqlx::Error),
}

impl fmt::Display for OutboxStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdempotencyConflict => formatter.write_str(
                "outbox idempotency key already belongs to a different immutable envelope",
            ),
            Self::InvalidReference => formatter.write_str("outbox reference is invalid"),
            Self::InvariantViolation => {
                formatter.write_str("outbox event/job link invariant failed")
            }
            Self::Jobs(_) => formatter.write_str("outbox delivery job operation failed"),
            Self::Database(_) => formatter.write_str("PostgreSQL outbox operation failed"),
        }
    }
}

impl std::error::Error for OutboxStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Jobs(error) => Some(error),
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for OutboxStoreError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Debug)]
struct StoredEvent {
    id: Uuid,
    organization_id: Option<Uuid>,
    destination: String,
    event_type: String,
    entity_type: Option<String>,
    entity_id: Option<Uuid>,
    reference: Option<String>,
    idempotency_key: String,
    payload: Value,
}

/// Records one immutable outbox event and exactly one delivery job atomically.
///
/// This function only uses the caller-owned PostgreSQL transaction: it never
/// acquires another connection, starts a nested transaction, or commits. A
/// duplicate equivalent envelope reuses the original event and job. Retries
/// therefore always retain the same event and external idempotency key.
pub async fn record(
    transaction: &mut Transaction<'_, Postgres>,
    event: NewOutboxEvent,
) -> Result<RecordedOutboxEvent, OutboxStoreError> {
    validate_reference(event.reference.as_deref())?;

    let inserted = sqlx::query(
        r#"
        INSERT INTO public.outbox_events (
            organizacion_id, destino, evento_tipo, entidad_tipo, entidad_id,
            referencia, idempotency_key, payload
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (organizacion_id, destino, idempotency_key) DO NOTHING
        RETURNING id, organizacion_id, destino, evento_tipo, entidad_tipo,
                  entidad_id, referencia, idempotency_key, payload
        "#,
    )
    .bind(event.organization_id)
    .bind(event.destination.as_str())
    .bind(event.event_type.as_str())
    .bind(event.entity_type.as_ref().map(OutboxEntityType::as_str))
    .bind(event.entity_id)
    .bind(event.reference.as_deref())
    .bind(event.idempotency_key.as_str())
    .bind(&event.payload)
    .fetch_optional(&mut **transaction)
    .await?;

    if let Some(row) = inserted {
        let stored = stored_event_from_row(row)?;
        let job = jobs::enqueue(
            transaction,
            NewJob {
                organization_id: event.organization_id,
                job_type: event.destination.delivery_job_type(),
                payload: json!({}),
                max_attempts: event.delivery_max_attempts,
                initial_next_attempt_at: event.delivery_next_attempt_at,
            },
        )
        .await
        .map_err(OutboxStoreError::Jobs)?;
        sqlx::query(
            "INSERT INTO public.outbox_job_links (outbox_event_id, job_id) VALUES ($1, $2)",
        )
        .bind(stored.id)
        .bind(job.id)
        .execute(&mut **transaction)
        .await?;
        return Ok(RecordedOutboxEvent {
            event_id: stored.id,
            job_id: job.id,
        });
    }

    let row = sqlx::query(
        r#"
        SELECT e.id, e.organizacion_id, e.destino, e.evento_tipo, e.entidad_tipo,
               e.entidad_id, e.referencia, e.idempotency_key, e.payload, l.job_id
        FROM public.outbox_events AS e
        INNER JOIN public.outbox_job_links AS l ON l.outbox_event_id = e.id
        WHERE e.organizacion_id IS NOT DISTINCT FROM $1
          AND e.destino = $2
          AND e.idempotency_key = $3
        "#,
    )
    .bind(event.organization_id)
    .bind(event.destination.as_str())
    .bind(event.idempotency_key.as_str())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(OutboxStoreError::InvariantViolation)?;
    let job_id: Uuid = row.try_get("job_id")?;
    let stored = stored_event_from_row(row)?;

    if !same_envelope(&stored, &event) {
        return Err(OutboxStoreError::IdempotencyConflict);
    }

    Ok(RecordedOutboxEvent {
        event_id: stored.id,
        job_id,
    })
}

fn validate_reference(reference: Option<&str>) -> Result<(), OutboxStoreError> {
    if reference.is_some_and(|value| {
        value.trim() != value || value.is_empty() || value.chars().count() > 256
    }) {
        return Err(OutboxStoreError::InvalidReference);
    }
    Ok(())
}

fn same_envelope(stored: &StoredEvent, requested: &NewOutboxEvent) -> bool {
    stored.organization_id == requested.organization_id
        && stored.destination == requested.destination.as_str()
        && stored.event_type == requested.event_type.as_str()
        && stored.entity_type.as_deref()
            == requested.entity_type.as_ref().map(OutboxEntityType::as_str)
        && stored.entity_id == requested.entity_id
        && stored.reference == requested.reference
        && stored.idempotency_key == requested.idempotency_key.as_str()
        && stored.payload == requested.payload
}

fn stored_event_from_row(row: PgRow) -> Result<StoredEvent, sqlx::Error> {
    Ok(StoredEvent {
        id: row.try_get("id")?,
        organization_id: row.try_get("organizacion_id")?,
        destination: row.try_get("destino")?,
        event_type: row.try_get("evento_tipo")?,
        entity_type: row.try_get("entidad_tipo")?,
        entity_id: row.try_get("entidad_id")?,
        reference: row.try_get("referencia")?,
        idempotency_key: row.try_get("idempotency_key")?,
        payload: row.try_get("payload")?,
    })
}

/// Immutable data supplied to an external destination adapter.
#[derive(Clone, Debug, PartialEq)]
pub struct OutboxDeliveryEnvelope {
    pub event_id: Uuid,
    pub organization_id: Option<Uuid>,
    pub destination: OutboxDestination,
    pub event_type: OutboxEventType,
    pub entity_type: Option<OutboxEntityType>,
    pub entity_id: Option<Uuid>,
    pub reference: Option<String>,
    pub idempotency_key: IdempotencyKey,
    pub payload: Value,
    pub occurred_at: OffsetDateTime,
}

/// Destination boundary for at-least-once outbox delivery.
///
/// A remote effect can succeed before the worker persists job completion. Every
/// adapter must therefore submit `envelope.idempotency_key` (or the same stable
/// external reference) on every attempt; the core cannot guarantee remote
/// exactly-once behavior.
#[async_trait]
pub trait OutboxDeliveryAdapter: Send + Sync {
    fn destination(&self) -> &OutboxDestination;

    async fn deliver(&self, envelope: &OutboxDeliveryEnvelope) -> JobExecutionResult;
}

pub struct OutboxJobHandler {
    db: PgPool,
    job_type: JobType,
    adapter: Arc<dyn OutboxDeliveryAdapter>,
}

impl OutboxJobHandler {
    pub fn new(db: PgPool, adapter: Arc<dyn OutboxDeliveryAdapter>) -> Self {
        let job_type = adapter.destination().delivery_job_type();
        Self {
            db,
            job_type,
            adapter,
        }
    }
}

#[async_trait]
impl JobHandler for OutboxJobHandler {
    fn job_type(&self) -> &JobType {
        &self.job_type
    }

    async fn execute(&self, job: &jobs::ClaimedJob) -> JobExecutionResult {
        let envelope = match load_delivery_envelope(&self.db, job.id).await {
            Ok(envelope) if envelope.destination == *self.adapter.destination() => envelope,
            Ok(_) => return retry_for_load_failure(),
            Err(_) => return retry_for_load_failure(),
        };
        self.adapter.deliver(&envelope).await
    }
}

pub fn register_delivery_adapter(
    dispatcher: &mut JobDispatcher,
    db: PgPool,
    adapter: Arc<dyn OutboxDeliveryAdapter>,
) -> Result<(), DuplicateJobType> {
    dispatcher.register(Arc::new(OutboxJobHandler::new(db, adapter)))
}

fn retry_for_load_failure() -> JobExecutionResult {
    JobExecutionResult::Retry {
        next_attempt_at: OffsetDateTime::now_utc() + time::Duration::minutes(1),
        error: jobs::SafeErrorSummary::new("No se pudo cargar el evento outbox vinculado.")
            .expect("static outbox error summary is safe"),
    }
}

pub async fn load_delivery_envelope(
    db: &PgPool,
    job_id: Uuid,
) -> Result<OutboxDeliveryEnvelope, OutboxStoreError> {
    let row = sqlx::query(
        r#"
        SELECT e.id, e.organizacion_id, e.destino, e.evento_tipo, e.entidad_tipo,
               e.entidad_id, e.referencia, e.idempotency_key, e.payload, e.ocurrido_en
        FROM public.outbox_job_links AS l
        INNER JOIN public.outbox_events AS e ON e.id = l.outbox_event_id
        WHERE l.job_id = $1
        "#,
    )
    .bind(job_id)
    .fetch_optional(db)
    .await?
    .ok_or(OutboxStoreError::InvariantViolation)?;

    let destination: String = row.try_get("destino")?;
    let event_type: String = row.try_get("evento_tipo")?;
    let entity_type: Option<String> = row.try_get("entidad_tipo")?;
    let idempotency_key: String = row.try_get("idempotency_key")?;
    Ok(OutboxDeliveryEnvelope {
        event_id: row.try_get("id")?,
        organization_id: row.try_get("organizacion_id")?,
        destination: OutboxDestination::new(destination)
            .map_err(|_| OutboxStoreError::InvariantViolation)?,
        event_type: OutboxEventType::new(event_type)
            .map_err(|_| OutboxStoreError::InvariantViolation)?,
        entity_type: entity_type
            .map(OutboxEntityType::new)
            .transpose()
            .map_err(|_| OutboxStoreError::InvariantViolation)?,
        entity_id: row.try_get("entidad_id")?,
        reference: row.try_get("referencia")?,
        idempotency_key: IdempotencyKey::new(idempotency_key)
            .map_err(|_| OutboxStoreError::InvariantViolation)?,
        payload: row.try_get("payload")?,
        occurred_at: row.try_get("ocurrido_en")?,
    })
}

#[derive(Clone, Debug, Default, Deserialize, IntoParams)]
pub struct OutboxDiagnosticsQuery {
    pub estado: Option<JobState>,
    pub destino: Option<String>,
    pub evento_tipo: Option<String>,
    pub limite: Option<u16>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OutboxDiagnostic {
    pub id: String,
    pub destino: String,
    pub evento_tipo: String,
    pub entidad_tipo: Option<String>,
    pub entidad_id: Option<String>,
    pub referencia: Option<String>,
    pub idempotency_key: String,
    pub ocurrido_en: String,
    pub creado_en: String,
    pub estado: JobState,
    pub intentos: i32,
    pub max_intentos: i32,
    pub next_attempt_at: String,
    pub bloqueado_en: Option<String>,
    pub bloqueado_por: Option<String>,
    pub actualizado_en: String,
    pub completado_en: Option<String>,
    pub ultimo_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OutboxDiagnosticsResponse {
    pub eventos: Vec<OutboxDiagnostic>,
}

pub async fn list_diagnostics(
    db: &PgPool,
    query: &OutboxDiagnosticsQuery,
) -> Result<OutboxDiagnosticsResponse, OutboxStoreError> {
    let limit = query.limite.unwrap_or(50).clamp(1, MAX_DIAGNOSTIC_LIMIT);
    let rows = sqlx::query(
        r#"
        SELECT e.id, e.destino, e.evento_tipo, e.entidad_tipo, e.entidad_id,
               e.referencia, e.idempotency_key,
               to_char(e.ocurrido_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS ocurrido_en,
               to_char(e.creado_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS creado_en,
               j.estado, j.intentos, j.max_intentos,
               to_char(j.next_attempt_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS next_attempt_at,
               CASE WHEN j.bloqueado_en IS NULL THEN NULL ELSE to_char(j.bloqueado_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END AS bloqueado_en,
               j.bloqueado_por,
               to_char(j.actualizado_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS actualizado_en,
               CASE WHEN j.completado_en IS NULL THEN NULL ELSE to_char(j.completado_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END AS completado_en,
               j.ultimo_error
        FROM public.outbox_events AS e
        INNER JOIN public.outbox_job_links AS l ON l.outbox_event_id = e.id
        INNER JOIN public.jobs AS j ON j.id = l.job_id
        WHERE ($1::text IS NULL OR j.estado = $1)
          AND ($2::text IS NULL OR e.destino = $2)
          AND ($3::text IS NULL OR e.evento_tipo = $3)
        ORDER BY j.actualizado_en DESC, e.id DESC
        LIMIT $4
        "#,
    )
    .bind(query.estado.map(JobState::as_str))
    .bind(query.destino.as_deref())
    .bind(query.evento_tipo.as_deref())
    .bind(i64::from(limit))
    .fetch_all(db)
    .await?;

    let eventos = rows
        .into_iter()
        .map(diagnostic_from_row)
        .collect::<Result<Vec<_>, OutboxStoreError>>()?;
    Ok(OutboxDiagnosticsResponse { eventos })
}

fn diagnostic_from_row(row: PgRow) -> Result<OutboxDiagnostic, OutboxStoreError> {
    let state: String = row.try_get("estado")?;
    Ok(OutboxDiagnostic {
        id: row.try_get::<Uuid, _>("id")?.to_string(),
        destino: row.try_get("destino")?,
        evento_tipo: row.try_get("evento_tipo")?,
        entidad_tipo: row.try_get("entidad_tipo")?,
        entidad_id: row
            .try_get::<Option<Uuid>, _>("entidad_id")?
            .map(|id| id.to_string()),
        referencia: row.try_get("referencia")?,
        idempotency_key: row.try_get("idempotency_key")?,
        ocurrido_en: row.try_get("ocurrido_en")?,
        creado_en: row.try_get("creado_en")?,
        estado: JobState::try_from(state.as_str())
            .map_err(|_| OutboxStoreError::InvariantViolation)?,
        intentos: row.try_get("intentos")?,
        max_intentos: row.try_get("max_intentos")?,
        next_attempt_at: row.try_get("next_attempt_at")?,
        bloqueado_en: row.try_get("bloqueado_en")?,
        bloqueado_por: row
            .try_get::<Option<Uuid>, _>("bloqueado_por")?
            .map(|id| id.to_string()),
        actualizado_en: row.try_get("actualizado_en")?,
        completado_en: row.try_get("completado_en")?,
        ultimo_error: row.try_get("ultimo_error")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_outbox_contract_codes_and_text() {
        assert!(OutboxDestination::new("finnegans").is_ok());
        assert!(OutboxDestination::new("Finnegans").is_err());
        assert!(OutboxEventType::new("stock.consumo_confirmado").is_ok());
        assert!(OutboxEventType::new("stock").is_err());
        assert!(OutboxEntityType::new("stock.movimiento").is_ok());
        assert!(OutboxEntityType::new("Stock").is_err());
        assert!(IdempotencyKey::new("pedido-123").is_ok());
        assert!(IdempotencyKey::new(" pedido-123").is_err());
    }
}
