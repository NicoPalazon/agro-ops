use std::{fmt, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

pub const MAX_CLAIM_BATCH_SIZE: u16 = 100;
pub const MAX_DIAGNOSTIC_LIMIT: u16 = 100;
pub const STALE_RECOVERY_ERROR: &str = "Ejecución recuperada por bloqueo obsoleto.";

const ENQUEUE_SQL: &str = r#"
INSERT INTO public.jobs (
    organizacion_id,
    tipo,
    payload,
    max_intentos,
    next_attempt_at
)
VALUES ($1, $2, $3, $4, $5)
RETURNING
    id,
    organizacion_id,
    tipo,
    estado,
    payload,
    intentos,
    max_intentos,
    next_attempt_at,
    bloqueado_en,
    bloqueado_por,
    creado_en,
    actualizado_en,
    completado_en,
    ultimo_error
"#;

const CLAIM_SQL: &str = r#"
WITH candidatos AS MATERIALIZED (
    SELECT id, next_attempt_at, creado_en
    FROM public.jobs
    WHERE estado = 'pendiente'
      AND next_attempt_at <= statement_timestamp()
      AND intentos < max_intentos
      AND tipo = ANY($1::text[])
    ORDER BY next_attempt_at ASC, creado_en ASC, id ASC
    FOR UPDATE SKIP LOCKED
    LIMIT $2
),
reclamados AS (
    UPDATE public.jobs AS job
    SET estado = 'ejecutando',
        intentos = job.intentos + 1,
        bloqueado_en = statement_timestamp(),
        bloqueado_por = $3,
        actualizado_en = statement_timestamp()
    FROM candidatos
    WHERE job.id = candidatos.id
    RETURNING job.*
)
SELECT
    reclamados.id,
    reclamados.organizacion_id,
    reclamados.tipo,
    reclamados.estado,
    reclamados.payload,
    reclamados.intentos,
    reclamados.max_intentos,
    reclamados.next_attempt_at,
    reclamados.bloqueado_en,
    reclamados.bloqueado_por,
    reclamados.creado_en,
    reclamados.actualizado_en,
    reclamados.completado_en,
    reclamados.ultimo_error
FROM reclamados
INNER JOIN candidatos ON candidatos.id = reclamados.id
ORDER BY candidatos.next_attempt_at ASC, candidatos.creado_en ASC, candidatos.id ASC
"#;

const MARK_SUCCEEDED_SQL: &str = r#"
UPDATE public.jobs
SET estado = 'completado',
    completado_en = statement_timestamp(),
    bloqueado_en = NULL,
    bloqueado_por = NULL,
    actualizado_en = statement_timestamp()
WHERE id = $1
  AND estado = 'ejecutando'
  AND bloqueado_por = $2
RETURNING
    id,
    organizacion_id,
    tipo,
    estado,
    payload,
    intentos,
    max_intentos,
    next_attempt_at,
    bloqueado_en,
    bloqueado_por,
    creado_en,
    actualizado_en,
    completado_en,
    ultimo_error
"#;

const MARK_FAILED_SQL: &str = r#"
UPDATE public.jobs
SET estado = CASE
        WHEN intentos < max_intentos THEN 'pendiente'
        ELSE 'agotado'
    END,
    next_attempt_at = CASE
        WHEN intentos < max_intentos THEN $3
        ELSE next_attempt_at
    END,
    bloqueado_en = NULL,
    bloqueado_por = NULL,
    actualizado_en = statement_timestamp(),
    ultimo_error = $4
WHERE id = $1
  AND estado = 'ejecutando'
  AND bloqueado_por = $2
RETURNING
    id,
    organizacion_id,
    tipo,
    estado,
    payload,
    intentos,
    max_intentos,
    next_attempt_at,
    bloqueado_en,
    bloqueado_por,
    creado_en,
    actualizado_en,
    completado_en,
    ultimo_error
"#;

const RECOVER_STALE_SQL: &str = r#"
WITH candidatos AS MATERIALIZED (
    SELECT id
    FROM public.jobs
    WHERE estado = 'ejecutando'
      AND bloqueado_en < statement_timestamp() - make_interval(secs => $1::double precision)
    ORDER BY bloqueado_en ASC, id ASC
    FOR UPDATE SKIP LOCKED
    LIMIT $2
),
recuperados AS (
    UPDATE public.jobs AS job
    SET estado = CASE
            WHEN job.intentos < job.max_intentos THEN 'pendiente'
            ELSE 'agotado'
        END,
        next_attempt_at = CASE
            WHEN job.intentos < job.max_intentos THEN statement_timestamp()
            ELSE job.next_attempt_at
        END,
        bloqueado_en = NULL,
        bloqueado_por = NULL,
        actualizado_en = statement_timestamp(),
        ultimo_error = $3
    FROM candidatos
    WHERE job.id = candidatos.id
    RETURNING job.estado
)
SELECT estado, COUNT(*)::bigint
FROM recuperados
GROUP BY estado
"#;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JobType(String);

impl JobType {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidJobType> {
        let value = value.into();
        if value.len() > 128 || !is_canonical_code(&value) {
            return Err(InvalidJobType);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidJobType;

impl fmt::Display for InvalidJobType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("job type must be a bounded canonical technical code")
    }
}

impl std::error::Error for InvalidJobType {}

fn is_canonical_code(value: &str) -> bool {
    let mut parts = value.split('.');
    let valid_part = |part: &str| {
        let mut bytes = part.bytes();
        matches!(bytes.next(), Some(b'a'..=b'z'))
            && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    };
    let Some(first) = parts.next() else {
        return false;
    };
    let remaining: Vec<_> = parts.collect();
    valid_part(first) && !remaining.is_empty() && remaining.into_iter().all(valid_part)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeErrorSummary(String);

impl SafeErrorSummary {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidSafeErrorSummary> {
        let value = value.into();
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed.chars().count() > 1024 {
            return Err(InvalidSafeErrorSummary);
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSafeErrorSummary;

impl fmt::Display for InvalidSafeErrorSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("safe job error summary must be nonblank and at most 1024 characters")
    }
}

impl std::error::Error for InvalidSafeErrorSummary {}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Pendiente,
    Ejecutando,
    Completado,
    Agotado,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pendiente => "pendiente",
            Self::Ejecutando => "ejecutando",
            Self::Completado => "completado",
            Self::Agotado => "agotado",
        }
    }
}

impl TryFrom<&str> for JobState {
    type Error = InvalidJobState;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pendiente" => Ok(Self::Pendiente),
            "ejecutando" => Ok(Self::Ejecutando),
            "completado" => Ok(Self::Completado),
            "agotado" => Ok(Self::Agotado),
            _ => Err(InvalidJobState),
        }
    }
}

#[derive(Debug)]
pub struct InvalidJobState;

impl fmt::Display for InvalidJobState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("database returned an unsupported job state")
    }
}

impl std::error::Error for InvalidJobState {}

#[derive(Clone, Debug)]
pub struct NewJob {
    pub organization_id: Option<Uuid>,
    pub job_type: JobType,
    pub payload: Value,
    pub max_attempts: i32,
    pub initial_next_attempt_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: Uuid,
    pub organization_id: Option<Uuid>,
    pub job_type: JobType,
    pub state: JobState,
    pub payload: Value,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_attempt_at: OffsetDateTime,
    pub locked_at: Option<OffsetDateTime>,
    pub locked_by: Option<Uuid>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
}

pub type ClaimedJob = Job;

#[derive(Debug)]
pub enum JobStoreError {
    InvalidBatchSize,
    InvalidStaleThreshold,
    Database(sqlx::Error),
}

impl fmt::Display for JobStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBatchSize => formatter.write_str("job batch size is outside safe bounds"),
            Self::InvalidStaleThreshold => {
                formatter.write_str("stale job threshold must be positive")
            }
            Self::Database(_) => formatter.write_str("PostgreSQL job operation failed"),
        }
    }
}

impl std::error::Error for JobStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for JobStoreError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum OwnershipTransition {
    Applied(Box<Job>),
    OwnershipLost,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecoveryResult {
    pub pending: u64,
    pub exhausted: u64,
}

pub async fn enqueue(
    transaction: &mut Transaction<'_, Postgres>,
    new_job: NewJob,
) -> Result<Job, JobStoreError> {
    let row = sqlx::query(ENQUEUE_SQL)
        .bind(new_job.organization_id)
        .bind(new_job.job_type.as_str())
        .bind(new_job.payload)
        .bind(new_job.max_attempts)
        .bind(new_job.initial_next_attempt_at)
        .fetch_one(&mut **transaction)
        .await?;
    job_from_row(row).map_err(JobStoreError::Database)
}

pub async fn claim(
    db: &PgPool,
    worker_id: Uuid,
    supported_types: &[JobType],
    batch_size: u16,
) -> Result<Vec<ClaimedJob>, JobStoreError> {
    validate_batch_size(batch_size)?;
    if supported_types.is_empty() {
        return Ok(Vec::new());
    }
    let supported_types: Vec<&str> = supported_types.iter().map(JobType::as_str).collect();
    sqlx::query(CLAIM_SQL)
        .bind(supported_types)
        .bind(i64::from(batch_size))
        .bind(worker_id)
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|row| job_from_row(row).map_err(JobStoreError::Database))
        .collect()
}

pub async fn mark_succeeded(
    db: &PgPool,
    job_id: Uuid,
    worker_id: Uuid,
) -> Result<OwnershipTransition, JobStoreError> {
    owned_transition(
        sqlx::query(MARK_SUCCEEDED_SQL)
            .bind(job_id)
            .bind(worker_id)
            .fetch_optional(db)
            .await?,
    )
}

pub async fn mark_failed(
    db: &PgPool,
    job_id: Uuid,
    worker_id: Uuid,
    next_attempt_at: OffsetDateTime,
    error: &SafeErrorSummary,
) -> Result<OwnershipTransition, JobStoreError> {
    owned_transition(
        sqlx::query(MARK_FAILED_SQL)
            .bind(job_id)
            .bind(worker_id)
            .bind(next_attempt_at)
            .bind(error.as_str())
            .fetch_optional(db)
            .await?,
    )
}

pub async fn recover_stale(
    db: &PgPool,
    stale_threshold: Duration,
    batch_size: u16,
) -> Result<RecoveryResult, JobStoreError> {
    validate_batch_size(batch_size)?;
    if stale_threshold.is_zero() {
        return Err(JobStoreError::InvalidStaleThreshold);
    }
    let seconds = stale_threshold.as_secs_f64();
    let rows = sqlx::query(RECOVER_STALE_SQL)
        .bind(seconds)
        .bind(i64::from(batch_size))
        .bind(STALE_RECOVERY_ERROR)
        .fetch_all(db)
        .await?;
    let mut result = RecoveryResult::default();
    for row in rows {
        let state: String = row.try_get("estado")?;
        let count: i64 = row.try_get("count")?;
        match JobState::try_from(state.as_str()) {
            Ok(JobState::Pendiente) => result.pending = count as u64,
            Ok(JobState::Agotado) => result.exhausted = count as u64,
            _ => return Err(JobStoreError::Database(invalid_state_error())),
        }
    }
    Ok(result)
}

#[derive(Clone, Debug, Default, Deserialize, IntoParams)]
pub struct JobDiagnosticsQuery {
    pub estado: Option<JobState>,
    pub limite: Option<u16>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct JobDiagnostic {
    pub id: String,
    pub tipo: String,
    pub estado: JobState,
    pub intentos: i32,
    pub max_intentos: i32,
    pub next_attempt_at: String,
    pub bloqueado_en: Option<String>,
    pub bloqueado_por: Option<String>,
    pub creado_en: String,
    pub actualizado_en: String,
    pub completado_en: Option<String>,
    pub ultimo_error: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct JobsDiagnosticsResponse {
    pub jobs: Vec<JobDiagnostic>,
}

pub async fn list_diagnostics(
    db: &PgPool,
    query: &JobDiagnosticsQuery,
) -> Result<JobsDiagnosticsResponse, JobStoreError> {
    let limit = query.limite.unwrap_or(50).clamp(1, MAX_DIAGNOSTIC_LIMIT);
    let rows = sqlx::query(
        r#"
        SELECT
            id,
            tipo,
            estado,
            intentos,
            max_intentos,
            to_char(next_attempt_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS next_attempt_at,
            CASE WHEN bloqueado_en IS NULL THEN NULL ELSE to_char(bloqueado_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END AS bloqueado_en,
            bloqueado_por,
            to_char(creado_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS creado_en,
            to_char(actualizado_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS actualizado_en,
            CASE WHEN completado_en IS NULL THEN NULL ELSE to_char(completado_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') END AS completado_en,
            ultimo_error
        FROM public.jobs
        WHERE ($1::text IS NULL OR estado = $1)
        ORDER BY actualizado_en DESC, id DESC
        LIMIT $2
        "#,
    )
    .bind(query.estado.map(JobState::as_str))
    .bind(i64::from(limit))
    .fetch_all(db)
    .await?;

    let jobs = rows
        .into_iter()
        .map(|row| {
            let state: String = row.try_get("estado")?;
            Ok(JobDiagnostic {
                id: row.try_get::<Uuid, _>("id")?.to_string(),
                tipo: row.try_get("tipo")?,
                estado: JobState::try_from(state.as_str()).map_err(|_| invalid_state_error())?,
                intentos: row.try_get("intentos")?,
                max_intentos: row.try_get("max_intentos")?,
                next_attempt_at: row.try_get("next_attempt_at")?,
                bloqueado_en: row.try_get("bloqueado_en")?,
                bloqueado_por: row
                    .try_get::<Option<Uuid>, _>("bloqueado_por")?
                    .map(|worker_id| worker_id.to_string()),
                creado_en: row.try_get("creado_en")?,
                actualizado_en: row.try_get("actualizado_en")?,
                completado_en: row.try_get("completado_en")?,
                ultimo_error: row.try_get("ultimo_error")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(JobsDiagnosticsResponse { jobs })
}

fn validate_batch_size(batch_size: u16) -> Result<(), JobStoreError> {
    if batch_size == 0 || batch_size > MAX_CLAIM_BATCH_SIZE {
        return Err(JobStoreError::InvalidBatchSize);
    }
    Ok(())
}

fn owned_transition(row: Option<PgRow>) -> Result<OwnershipTransition, JobStoreError> {
    match row {
        Some(row) => Ok(OwnershipTransition::Applied(Box::new(
            job_from_row(row).map_err(JobStoreError::Database)?,
        ))),
        None => Ok(OwnershipTransition::OwnershipLost),
    }
}

fn job_from_row(row: PgRow) -> Result<Job, sqlx::Error> {
    let state: String = row.try_get("estado")?;
    let job_type: String = row.try_get("tipo")?;
    Ok(Job {
        id: row.try_get("id")?,
        organization_id: row.try_get("organizacion_id")?,
        job_type: JobType::new(job_type).map_err(|_| invalid_job_type_error())?,
        state: JobState::try_from(state.as_str()).map_err(|_| invalid_state_error())?,
        payload: row.try_get("payload")?,
        attempts: row.try_get("intentos")?,
        max_attempts: row.try_get("max_intentos")?,
        next_attempt_at: row.try_get("next_attempt_at")?,
        locked_at: row.try_get("bloqueado_en")?,
        locked_by: row.try_get("bloqueado_por")?,
        created_at: row.try_get("creado_en")?,
        updated_at: row.try_get("actualizado_en")?,
        completed_at: row.try_get("completado_en")?,
        last_error: row.try_get("ultimo_error")?,
    })
}

fn invalid_state_error() -> sqlx::Error {
    sqlx::Error::Decode(Box::new(InvalidJobState))
}

fn invalid_job_type_error() -> sqlx::Error {
    sqlx::Error::Decode(Box::new(InvalidJobType))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_canonical_job_types() {
        for valid in ["test.success", "documentos.generar_pdf", "arca.sync_v2"] {
            assert!(JobType::new(valid).is_ok(), "{valid} must be valid");
        }
        for invalid in ["", "test", "Test.success", "test..success", "test-success"] {
            assert!(JobType::new(invalid).is_err(), "{invalid} must be invalid");
        }
    }

    #[test]
    fn validates_safe_error_summaries() {
        assert_eq!(
            SafeErrorSummary::new("  Error seguro  ")
                .expect("summary must be valid")
                .as_str(),
            "Error seguro"
        );
        assert!(SafeErrorSummary::new(" ").is_err());
        assert!(SafeErrorSummary::new("x".repeat(1025)).is_err());
    }
}
