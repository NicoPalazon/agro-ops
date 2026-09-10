//! Durable correspondence between Agro Ops UUIDs and opaque external identities.
//!
//! This module deliberately makes no provider requests. An external reference is
//! integration metadata, not ownership of the referenced Agro Ops entity. Callers
//! must create or update it through their own PostgreSQL transaction and must not
//! hold that transaction open while communicating with a remote provider.

use std::fmt;

use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use time::OffsetDateTime;
use uuid::Uuid;

const MAX_SYSTEM_LENGTH: usize = 64;
const MAX_EXTERNAL_ID_LENGTH: usize = 256;
const MAX_ENTITY_TYPE_LENGTH: usize = 128;
const MAX_SYNC_VERSION_LENGTH: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalSystem(String);

impl ExternalSystem {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidExternalReferenceCode> {
        let value = value.into();
        if value.len() > MAX_SYSTEM_LENGTH || !is_canonical_part(&value) {
            return Err(InvalidExternalReferenceCode);
        }
        Ok(Self(value))
    }

    pub fn finnegans() -> Self {
        Self("finnegans".to_owned())
    }

    pub fn arca() -> Self {
        Self("arca".to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalId(String);

impl ExternalId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidExternalReferenceText> {
        let value = value.into();
        if !is_bounded_trimmed_text(&value, MAX_EXTERNAL_ID_LENGTH) {
            return Err(InvalidExternalReferenceText);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalEntityType(String);

impl ExternalEntityType {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidExternalReferenceCode> {
        let value = value.into();
        if value.len() > MAX_ENTITY_TYPE_LENGTH || !is_canonical_code(&value) {
            return Err(InvalidExternalReferenceCode);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncVersion(String);

impl SyncVersion {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidExternalReferenceText> {
        let value = value.into();
        if !is_bounded_trimmed_text(&value, MAX_SYNC_VERSION_LENGTH) {
            return Err(InvalidExternalReferenceText);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastSyncStatus {
    Pendiente,
    Sincronizado,
    Fallido,
    Desactualizado,
}

impl LastSyncStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pendiente => "pendiente",
            Self::Sincronizado => "sincronizado",
            Self::Fallido => "fallido",
            Self::Desactualizado => "desactualizado",
        }
    }
}

impl TryFrom<&str> for LastSyncStatus {
    type Error = InvalidExternalReferenceCode;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pendiente" => Ok(Self::Pendiente),
            "sincronizado" => Ok(Self::Sincronizado),
            "fallido" => Ok(Self::Fallido),
            "desactualizado" => Ok(Self::Desactualizado),
            _ => Err(InvalidExternalReferenceCode),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidExternalReferenceCode;

impl fmt::Display for InvalidExternalReferenceCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("external reference code must be bounded and canonical")
    }
}

impl std::error::Error for InvalidExternalReferenceCode {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidExternalReferenceText;

impl fmt::Display for InvalidExternalReferenceText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("external reference text must be trimmed, nonblank, and bounded")
    }
}

impl std::error::Error for InvalidExternalReferenceText {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewExternalReference {
    pub organization_id: Option<Uuid>,
    pub system: ExternalSystem,
    pub external_id: ExternalId,
    pub entity_type: ExternalEntityType,
    pub entity_id: Uuid,
    pub sync_version: Option<SyncVersion>,
    pub last_sync_status: LastSyncStatus,
    pub last_synced_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalReference {
    pub id: Uuid,
    pub organization_id: Option<Uuid>,
    pub system: ExternalSystem,
    pub external_id: ExternalId,
    pub entity_type: ExternalEntityType,
    pub entity_id: Uuid,
    pub sync_version: Option<SyncVersion>,
    pub last_sync_status: LastSyncStatus,
    pub last_synced_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredExternalReference {
    pub reference: ExternalReference,
    pub created: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncMetadata {
    pub sync_version: Option<SyncVersion>,
    pub last_sync_status: LastSyncStatus,
    pub last_synced_at: Option<OffsetDateTime>,
}

#[derive(Debug)]
pub enum ExternalReferenceStoreError {
    IdentityConflict,
    NotFound,
    InvariantViolation,
    Database(sqlx::Error),
}

impl fmt::Display for ExternalReferenceStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdentityConflict => formatter
                .write_str("external identity already belongs to a different Agro Ops entity"),
            Self::NotFound => formatter.write_str("external reference was not found"),
            Self::InvariantViolation => {
                formatter.write_str("external reference persistence invariant failed")
            }
            Self::Database(_) => {
                formatter.write_str("PostgreSQL external reference operation failed")
            }
        }
    }
}

impl std::error::Error for ExternalReferenceStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

impl From<sqlx::Error> for ExternalReferenceStoreError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Registers a durable external identity in the caller-owned transaction.
///
/// An identical registration returns the existing reference without rewriting
/// synchronization metadata. If the same external identity points to another
/// Agro Ops entity, this returns [`ExternalReferenceStoreError::IdentityConflict`].
/// This function never obtains another connection or commits the transaction.
pub async fn register(
    transaction: &mut Transaction<'_, Postgres>,
    reference: NewExternalReference,
) -> Result<RegisteredExternalReference, ExternalReferenceStoreError> {
    let inserted = sqlx::query(
        r#"
        INSERT INTO public.external_references (
            organizacion_id, sistema_externo, external_id, entidad_tipo, entidad_id,
            sync_version, last_sync_status, ultimo_sync_en
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (organizacion_id, sistema_externo, external_id) DO NOTHING
        RETURNING id, organizacion_id, sistema_externo, external_id, entidad_tipo, entidad_id,
                  sync_version, last_sync_status, ultimo_sync_en, creado_en, actualizado_en
        "#,
    )
    .bind(reference.organization_id)
    .bind(reference.system.as_str())
    .bind(reference.external_id.as_str())
    .bind(reference.entity_type.as_str())
    .bind(reference.entity_id)
    .bind(reference.sync_version.as_ref().map(SyncVersion::as_str))
    .bind(reference.last_sync_status.as_str())
    .bind(reference.last_synced_at)
    .fetch_optional(&mut **transaction)
    .await?;

    if let Some(row) = inserted {
        return Ok(RegisteredExternalReference {
            reference: reference_from_row(row)?,
            created: true,
        });
    }

    let row = sqlx::query(
        r#"
        SELECT id, organizacion_id, sistema_externo, external_id, entidad_tipo, entidad_id,
               sync_version, last_sync_status, ultimo_sync_en, creado_en, actualizado_en
        FROM public.external_references
        WHERE organizacion_id IS NOT DISTINCT FROM $1
          AND sistema_externo = $2
          AND external_id = $3
        "#,
    )
    .bind(reference.organization_id)
    .bind(reference.system.as_str())
    .bind(reference.external_id.as_str())
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(ExternalReferenceStoreError::InvariantViolation)?;
    let stored = reference_from_row(row)?;

    if !same_identity(&stored, &reference) {
        return Err(ExternalReferenceStoreError::IdentityConflict);
    }

    Ok(RegisteredExternalReference {
        reference: stored,
        created: false,
    })
}

pub async fn resolve_by_external_identity(
    db: &PgPool,
    organization_id: Option<Uuid>,
    system: &ExternalSystem,
    external_id: &ExternalId,
) -> Result<Option<ExternalReference>, ExternalReferenceStoreError> {
    let row = sqlx::query(
        r#"
        SELECT id, organizacion_id, sistema_externo, external_id, entidad_tipo, entidad_id,
               sync_version, last_sync_status, ultimo_sync_en, creado_en, actualizado_en
        FROM public.external_references
        WHERE organizacion_id IS NOT DISTINCT FROM $1
          AND sistema_externo = $2
          AND external_id = $3
        "#,
    )
    .bind(organization_id)
    .bind(system.as_str())
    .bind(external_id.as_str())
    .fetch_optional(db)
    .await?;

    row.map(reference_from_row).transpose()
}

pub async fn list_for_entity(
    db: &PgPool,
    organization_id: Option<Uuid>,
    entity_type: &ExternalEntityType,
    entity_id: Uuid,
) -> Result<Vec<ExternalReference>, ExternalReferenceStoreError> {
    let rows = sqlx::query(
        r#"
        SELECT id, organizacion_id, sistema_externo, external_id, entidad_tipo, entidad_id,
               sync_version, last_sync_status, ultimo_sync_en, creado_en, actualizado_en
        FROM public.external_references
        WHERE organizacion_id IS NOT DISTINCT FROM $1
          AND entidad_tipo = $2
          AND entidad_id = $3
        ORDER BY creado_en ASC, id ASC
        "#,
    )
    .bind(organization_id)
    .bind(entity_type.as_str())
    .bind(entity_id)
    .fetch_all(db)
    .await?;

    rows.into_iter().map(reference_from_row).collect()
}

/// Updates only synchronization diagnostics in the caller-owned transaction.
/// Identity fields cannot be supplied to this operation and are also guarded by
/// PostgreSQL, so an external record is never silently relinked.
pub async fn update_sync_metadata(
    transaction: &mut Transaction<'_, Postgres>,
    reference_id: Uuid,
    metadata: SyncMetadata,
) -> Result<ExternalReference, ExternalReferenceStoreError> {
    let row = sqlx::query(
        r#"
        UPDATE public.external_references
        SET sync_version = $2,
            last_sync_status = $3,
            ultimo_sync_en = $4,
            actualizado_en = statement_timestamp()
        WHERE id = $1
        RETURNING id, organizacion_id, sistema_externo, external_id, entidad_tipo, entidad_id,
                  sync_version, last_sync_status, ultimo_sync_en, creado_en, actualizado_en
        "#,
    )
    .bind(reference_id)
    .bind(metadata.sync_version.as_ref().map(SyncVersion::as_str))
    .bind(metadata.last_sync_status.as_str())
    .bind(metadata.last_synced_at)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(ExternalReferenceStoreError::NotFound)?;

    reference_from_row(row)
}

fn same_identity(stored: &ExternalReference, requested: &NewExternalReference) -> bool {
    stored.organization_id == requested.organization_id
        && stored.system == requested.system
        && stored.external_id == requested.external_id
        && stored.entity_type == requested.entity_type
        && stored.entity_id == requested.entity_id
}

fn reference_from_row(row: PgRow) -> Result<ExternalReference, ExternalReferenceStoreError> {
    let system: String = row.try_get("sistema_externo")?;
    let external_id: String = row.try_get("external_id")?;
    let entity_type: String = row.try_get("entidad_tipo")?;
    let sync_version: Option<String> = row.try_get("sync_version")?;
    let status: String = row.try_get("last_sync_status")?;

    Ok(ExternalReference {
        id: row.try_get("id")?,
        organization_id: row.try_get("organizacion_id")?,
        system: ExternalSystem::new(system)
            .map_err(|_| ExternalReferenceStoreError::InvariantViolation)?,
        external_id: ExternalId::new(external_id)
            .map_err(|_| ExternalReferenceStoreError::InvariantViolation)?,
        entity_type: ExternalEntityType::new(entity_type)
            .map_err(|_| ExternalReferenceStoreError::InvariantViolation)?,
        entity_id: row.try_get("entidad_id")?,
        sync_version: sync_version
            .map(SyncVersion::new)
            .transpose()
            .map_err(|_| ExternalReferenceStoreError::InvariantViolation)?,
        last_sync_status: LastSyncStatus::try_from(status.as_str())
            .map_err(|_| ExternalReferenceStoreError::InvariantViolation)?,
        last_synced_at: row.try_get("ultimo_sync_en")?,
        created_at: row.try_get("creado_en")?,
        updated_at: row.try_get("actualizado_en")?,
    })
}

fn is_bounded_trimmed_text(value: &str, max_length: usize) -> bool {
    value.trim() == value && !value.is_empty() && value.chars().count() <= max_length
}

fn is_canonical_part(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z'))
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn is_canonical_code(value: &str) -> bool {
    !value.is_empty() && value.split('.').all(is_canonical_part)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_canonical_codes_and_opaque_text() {
        assert_eq!(ExternalSystem::finnegans().as_str(), "finnegans");
        assert_eq!(ExternalSystem::arca().as_str(), "arca");
        assert!(ExternalSystem::new("custom_provider").is_ok());
        assert!(ExternalSystem::new("Finnegans").is_err());
        assert!(ExternalEntityType::new("stock.movimiento").is_ok());
        assert!(ExternalEntityType::new("Stock.movimiento").is_err());
        assert!(ExternalId::new("external/item?rev=001").is_ok());
        assert!(ExternalId::new(" external/item").is_err());
        assert!(SyncVersion::new("etag-123").is_ok());
        assert!(SyncVersion::new(" ").is_err());
        assert!(LastSyncStatus::try_from("sincronizado").is_ok());
        assert!(LastSyncStatus::try_from("running").is_err());
    }
}
