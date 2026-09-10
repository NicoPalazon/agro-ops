//! Transactional application-command idempotency.
//!
//! Callers provide a deterministic fingerprint of canonical semantic input and
//! an explicitly constructed, small replay result. This module never serializes
//! commands, HTTP requests, authorization context, or domain rows. Completed
//! records contain no request body and must never contain credentials or secrets.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use time::OffsetDateTime;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

const MAX_OPERATION_LENGTH: usize = 128;
const MAX_IDEMPOTENCY_KEY_LENGTH: usize = 256;
pub const MAX_SAFE_RESULT_BYTES: usize = 32 * 1024;
const MAX_DIAGNOSTIC_LIMIT: u16 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationCode(String);

impl OperationCode {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidOperationCode> {
        let value = value.into();
        if value.len() > MAX_OPERATION_LENGTH || !is_dotted_canonical_code(&value) {
            return Err(InvalidOperationCode);
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
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidIdempotencyKey> {
        let value = value.into();
        if value.trim() != value
            || value.is_empty()
            || value.chars().count() > MAX_IDEMPOTENCY_KEY_LENGTH
        {
            return Err(InvalidIdempotencyKey);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestFingerprint([u8; 32]);

impl RequestFingerprint {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Hashes bytes that the caller has already made semantically canonical.
    ///
    /// This helper does not define a serialization format. Callers must exclude
    /// credentials, request IDs, retry timestamps, and other non-semantic input.
    pub fn sha256(canonical_bytes: &[u8]) -> Self {
        Self(Sha256::digest(canonical_bytes).into())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

#[derive(Clone, PartialEq)]
pub struct SafeIdempotencyResult {
    value: Value,
    size_bytes: usize,
}

impl SafeIdempotencyResult {
    pub fn new(value: Value) -> Result<Self, InvalidSafeIdempotencyResult> {
        let size_bytes = serde_json::to_vec(&value)
            .map_err(|_| InvalidSafeIdempotencyResult)?
            .len();
        if size_bytes > MAX_SAFE_RESULT_BYTES {
            return Err(InvalidSafeIdempotencyResult);
        }
        Ok(Self { value, size_bytes })
    }

    pub fn as_value(&self) -> &Value {
        &self.value
    }

    pub fn into_value(self) -> Value {
        self.value
    }
}

impl fmt::Debug for SafeIdempotencyResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SafeIdempotencyResult")
            .field("size_bytes", &self.size_bytes)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidOperationCode;

impl fmt::Display for InvalidOperationCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("operation code must be bounded, lowercase, and dotted")
    }
}

impl std::error::Error for InvalidOperationCode {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidIdempotencyKey;

impl fmt::Display for InvalidIdempotencyKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("idempotency key must be trimmed, nonblank, and bounded")
    }
}

impl std::error::Error for InvalidIdempotencyKey {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSafeIdempotencyResult;

impl fmt::Display for InvalidSafeIdempotencyResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("idempotency replay result exceeds the safe size limit")
    }
}

impl std::error::Error for InvalidSafeIdempotencyResult {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdempotencyRequest {
    pub organization_id: Option<Uuid>,
    pub operation: OperationCode,
    pub key: IdempotencyKey,
    pub request_fingerprint: RequestFingerprint,
}

#[derive(Debug)]
pub enum IdempotencyDecision {
    Proceed(PendingIdempotency),
    Replay(StoredIdempotencyResult),
}

#[derive(Debug)]
pub struct PendingIdempotency {
    organization_id: Option<Uuid>,
    operation: OperationCode,
    key: IdempotencyKey,
    request_fingerprint: RequestFingerprint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredIdempotencyResult {
    pub record_id: Uuid,
    pub result: SafeIdempotencyResult,
    pub completed_at: OffsetDateTime,
}

#[derive(Clone, Debug, Default, Deserialize, IntoParams)]
pub struct IdempotencyDiagnosticsQuery {
    pub operacion: Option<String>,
    pub idempotency_key: Option<String>,
    pub limite: Option<u16>,
}

/// Safe diagnostic metadata for a completed idempotency record.
///
/// The stored replay result is deliberately excluded: this endpoint is for
/// coordination diagnosis, not replaying or inspecting application data.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct IdempotencyDiagnostic {
    pub id: String,
    pub operacion: String,
    pub idempotency_key: String,
    pub request_sha256: String,
    pub completado_en: String,
    pub resultado_bytes: i64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct IdempotencyDiagnosticsResponse {
    pub registros: Vec<IdempotencyDiagnostic>,
}

pub async fn list_diagnostics(
    db: &PgPool,
    organization_id: Uuid,
    query: &IdempotencyDiagnosticsQuery,
) -> Result<IdempotencyDiagnosticsResponse, sqlx::Error> {
    let limit = query.limite.unwrap_or(50).clamp(1, MAX_DIAGNOSTIC_LIMIT);
    let rows = sqlx::query(
        r#"
        SELECT
            id,
            operacion,
            idempotency_key,
            encode(request_sha256, 'hex') AS request_sha256,
            to_char(completado_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS completado_en,
            octet_length(resultado::text)::bigint AS resultado_bytes
        FROM public.idempotency_records
        WHERE organizacion_id = $1
          AND ($2::text IS NULL OR operacion = $2)
          AND ($3::text IS NULL OR idempotency_key = $3)
        ORDER BY completado_en DESC, id DESC
        LIMIT $4
        "#,
    )
    .bind(organization_id)
    .bind(query.operacion.as_deref())
    .bind(query.idempotency_key.as_deref())
    .bind(i64::from(limit))
    .fetch_all(db)
    .await?;

    let registros = rows
        .into_iter()
        .map(|row| {
            Ok(IdempotencyDiagnostic {
                id: row.try_get::<Uuid, _>("id")?.to_string(),
                operacion: row.try_get("operacion")?,
                idempotency_key: row.try_get("idempotency_key")?,
                request_sha256: row.try_get("request_sha256")?,
                completado_en: row.try_get("completado_en")?,
                resultado_bytes: row.try_get("resultado_bytes")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(IdempotencyDiagnosticsResponse { registros })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdempotencyConflict;

impl fmt::Display for IdempotencyConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("idempotency key already belongs to different semantic input")
    }
}

impl std::error::Error for IdempotencyConflict {}

#[derive(Debug)]
pub enum IdempotencyError {
    Conflict(IdempotencyConflict),
    InvariantViolation,
    Database(sqlx::Error),
}

impl fmt::Display for IdempotencyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict(error) => error.fmt(formatter),
            Self::InvariantViolation => {
                formatter.write_str("application idempotency persistence invariant failed")
            }
            Self::Database(_) => formatter.write_str("PostgreSQL idempotency operation failed"),
        }
    }
}

impl std::error::Error for IdempotencyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Conflict(error) => Some(error),
            Self::Database(error) => Some(error),
            Self::InvariantViolation => None,
        }
    }
}

impl From<sqlx::Error> for IdempotencyError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Acquires transaction-scoped coordination and decides whether to proceed.
///
/// The advisory lock remains held until the caller commits or rolls back. On a
/// replay, callers must skip all business, audit, outbox, job, and document writes.
/// This function never opens another connection, starts a transaction, or commits.
pub async fn begin(
    transaction: &mut Transaction<'_, Postgres>,
    request: IdempotencyRequest,
) -> Result<IdempotencyDecision, IdempotencyError> {
    acquire_identity_lock(
        transaction,
        request.organization_id,
        &request.operation,
        &request.key,
    )
    .await?;

    let stored = load_record(
        transaction,
        request.organization_id,
        &request.operation,
        &request.key,
    )
    .await?;

    match stored {
        None => Ok(IdempotencyDecision::Proceed(PendingIdempotency {
            organization_id: request.organization_id,
            operation: request.operation,
            key: request.key,
            request_fingerprint: request.request_fingerprint,
        })),
        Some(stored) if stored.request_fingerprint == request.request_fingerprint => {
            Ok(IdempotencyDecision::Replay(stored.result))
        }
        Some(_) => Err(IdempotencyError::Conflict(IdempotencyConflict)),
    }
}

/// Persists a successful replay result in the caller-owned transaction.
///
/// The non-constructible, consumed `PendingIdempotency` token binds completion to
/// the identity and fingerprint resolved by `begin`. A rollback removes this row
/// together with the caller's business/audit/outbox changes.
pub async fn complete(
    transaction: &mut Transaction<'_, Postgres>,
    pending: PendingIdempotency,
    result: SafeIdempotencyResult,
) -> Result<StoredIdempotencyResult, IdempotencyError> {
    acquire_identity_lock(
        transaction,
        pending.organization_id,
        &pending.operation,
        &pending.key,
    )
    .await?;

    let inserted = sqlx::query(
        r#"
        INSERT INTO public.idempotency_records (
            organizacion_id, operacion, idempotency_key, request_sha256, resultado
        )
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (organizacion_id, operacion, idempotency_key) DO NOTHING
        RETURNING id, request_sha256, resultado, completado_en
        "#,
    )
    .bind(pending.organization_id)
    .bind(pending.operation.as_str())
    .bind(pending.key.as_str())
    .bind(pending.request_fingerprint.as_bytes().as_slice())
    .bind(result.as_value())
    .fetch_optional(&mut **transaction)
    .await?;

    if let Some(row) = inserted {
        return Ok(stored_result_from_row(row)?.result);
    }

    let stored = load_record(
        transaction,
        pending.organization_id,
        &pending.operation,
        &pending.key,
    )
    .await?
    .ok_or(IdempotencyError::InvariantViolation)?;

    if stored.request_fingerprint != pending.request_fingerprint {
        return Err(IdempotencyError::Conflict(IdempotencyConflict));
    }

    Err(IdempotencyError::InvariantViolation)
}

struct StoredRecord {
    request_fingerprint: RequestFingerprint,
    result: StoredIdempotencyResult,
}

async fn acquire_identity_lock(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Option<Uuid>,
    operation: &OperationCode,
    key: &IdempotencyKey,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        SELECT pg_catalog.pg_advisory_xact_lock(
            pg_catalog.hashtextextended(
                pg_catalog.jsonb_build_array($1::uuid, $2::text, $3::text)::text,
                0::bigint
            )
        )
        "#,
    )
    .bind(organization_id)
    .bind(operation.as_str())
    .bind(key.as_str())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn load_record(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Option<Uuid>,
    operation: &OperationCode,
    key: &IdempotencyKey,
) -> Result<Option<StoredRecord>, IdempotencyError> {
    let row = sqlx::query(
        r#"
        SELECT id, request_sha256, resultado, completado_en
        FROM public.idempotency_records
        WHERE organizacion_id IS NOT DISTINCT FROM $1
          AND operacion = $2
          AND idempotency_key = $3
        "#,
    )
    .bind(organization_id)
    .bind(operation.as_str())
    .bind(key.as_str())
    .fetch_optional(&mut **transaction)
    .await?;

    row.map(stored_result_from_row).transpose()
}

fn stored_result_from_row(row: PgRow) -> Result<StoredRecord, IdempotencyError> {
    let fingerprint: Vec<u8> = row.try_get("request_sha256")?;
    let fingerprint: [u8; 32] = fingerprint
        .try_into()
        .map_err(|_| IdempotencyError::InvariantViolation)?;
    let value: Value = row.try_get("resultado")?;

    Ok(StoredRecord {
        request_fingerprint: RequestFingerprint::new(fingerprint),
        result: StoredIdempotencyResult {
            record_id: row.try_get("id")?,
            result: SafeIdempotencyResult::new(value)
                .map_err(|_| IdempotencyError::InvariantViolation)?,
            completed_at: row.try_get("completado_en")?,
        },
    })
}

fn is_canonical_part(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn is_dotted_canonical_code(value: &str) -> bool {
    value.contains('.') && value.split('.').all(is_canonical_part)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_typed_inputs_and_hashes_only_supplied_canonical_bytes() {
        assert!(OperationCode::new("stock.registrar_ajuste").is_ok());
        assert!(OperationCode::new("Stock.registrar_ajuste").is_err());
        assert!(OperationCode::new("stock").is_err());
        assert!(IdempotencyKey::new("command-123").is_ok());
        assert!(IdempotencyKey::new(" command-123").is_err());
        assert_eq!(
            RequestFingerprint::sha256(b"canonical-command").as_bytes(),
            &[
                89, 121, 202, 108, 0, 210, 200, 255, 126, 240, 47, 56, 173, 109, 245, 228, 96, 139,
                76, 13, 118, 219, 204, 149, 132, 7, 125, 150, 173, 201, 64, 193,
            ]
        );
        assert!(SafeIdempotencyResult::new(Value::String("x".repeat(32 * 1024))).is_err());
    }
}
