use std::{fmt, sync::Arc};

use async_trait::async_trait;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction, postgres::PgRow};
use time::OffsetDateTime;
use tracing::warn;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    audit::{self, AuditActor, NewAuditEvent},
    authorization::AuthorizationContext,
    config::DocumentConfig,
};

const MAX_FILENAME_LENGTH: usize = 255;
const MAX_MIME_LENGTH: usize = 255;
const MAX_DUPLICATE_CONTENT_MATCHES: i64 = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentSettings {
    storage_bucket: String,
    max_upload_bytes: usize,
}

impl DocumentSettings {
    pub fn from_config(config: &DocumentConfig) -> Self {
        Self {
            storage_bucket: config.storage_bucket().to_owned(),
            max_upload_bytes: config.max_upload_bytes(),
        }
    }

    pub fn new(storage_bucket: impl Into<String>, max_upload_bytes: usize) -> Self {
        Self {
            storage_bucket: storage_bucket.into(),
            max_upload_bytes,
        }
    }

    pub fn storage_bucket(&self) -> &str {
        &self.storage_bucket
    }

    pub fn max_upload_bytes(&self) -> usize {
        self.max_upload_bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateDownloadAccess {
    pub url: String,
    pub expires_in_seconds: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentStorageError {
    Unavailable,
}

#[async_trait]
pub trait DocumentStorage: Send + Sync {
    async fn put(
        &self,
        bucket: &str,
        key: &str,
        mime_type: &str,
        content: &[u8],
    ) -> Result<(), DocumentStorageError>;

    async fn create_download_access(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<PrivateDownloadAccess, DocumentStorageError>;

    async fn delete(&self, bucket: &str, key: &str) -> Result<(), DocumentStorageError>;
}

#[derive(Debug)]
pub struct UnavailableDocumentStorage;

#[async_trait]
impl DocumentStorage for UnavailableDocumentStorage {
    async fn put(
        &self,
        _bucket: &str,
        _key: &str,
        _mime_type: &str,
        _content: &[u8],
    ) -> Result<(), DocumentStorageError> {
        Err(DocumentStorageError::Unavailable)
    }

    async fn create_download_access(
        &self,
        _bucket: &str,
        _key: &str,
    ) -> Result<PrivateDownloadAccess, DocumentStorageError> {
        Err(DocumentStorageError::Unavailable)
    }

    async fn delete(&self, _bucket: &str, _key: &str) -> Result<(), DocumentStorageError> {
        Err(DocumentStorageError::Unavailable)
    }
}

#[derive(Debug)]
pub enum DocumentUploadError {
    MissingFile,
    InvalidFilename,
    InvalidMimeType,
    Empty,
    TooLarge,
}

impl fmt::Display for DocumentUploadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MissingFile => "a document file is required",
            Self::InvalidFilename => "document filename is invalid",
            Self::InvalidMimeType => "document MIME type is invalid",
            Self::Empty => "document file must not be empty",
            Self::TooLarge => "document file exceeds the configured maximum size",
        })
    }
}

impl std::error::Error for DocumentUploadError {}

/// Bounded backend-side upload collector. Chunks are hashed as they are read;
/// buffering is capped by `DocumentSettings::max_upload_bytes` because the
/// current Storage HTTP adapter requires a bounded request body.
pub struct DocumentUploadCollector {
    filename: String,
    mime_type: String,
    maximum_size: usize,
    bytes: Vec<u8>,
    hasher: Sha256,
}

impl DocumentUploadCollector {
    pub fn new(
        filename: impl Into<String>,
        mime_type: impl Into<String>,
        maximum_size: usize,
    ) -> Result<Self, DocumentUploadError> {
        let filename = filename.into();
        let mime_type = mime_type.into();
        validate_filename(&filename)?;
        validate_mime_type(&mime_type)?;
        Ok(Self {
            filename,
            mime_type,
            maximum_size,
            bytes: Vec::new(),
            hasher: Sha256::new(),
        })
    }

    pub fn push_chunk(&mut self, chunk: &[u8]) -> Result<(), DocumentUploadError> {
        let size = self
            .bytes
            .len()
            .checked_add(chunk.len())
            .filter(|size| *size <= self.maximum_size)
            .ok_or(DocumentUploadError::TooLarge)?;
        self.bytes.reserve(size.saturating_sub(self.bytes.len()));
        self.hasher.update(chunk);
        self.bytes.extend_from_slice(chunk);
        Ok(())
    }

    pub fn finish(self) -> Result<DocumentUpload, DocumentUploadError> {
        if self.bytes.is_empty() {
            return Err(DocumentUploadError::Empty);
        }
        Ok(DocumentUpload {
            filename: self.filename,
            mime_type: self.mime_type,
            sha256: self.hasher.finalize().into(),
            bytes: self.bytes,
        })
    }
}

pub struct DocumentUpload {
    filename: String,
    mime_type: String,
    sha256: [u8; 32],
    bytes: Vec<u8>,
}

impl DocumentUpload {
    #[cfg(test)]
    pub fn from_bytes(
        filename: impl Into<String>,
        mime_type: impl Into<String>,
        bytes: &[u8],
        maximum_size: usize,
    ) -> Result<Self, DocumentUploadError> {
        let mut collector = DocumentUploadCollector::new(filename, mime_type, maximum_size)?;
        collector.push_chunk(bytes)?;
        collector.finish()
    }

    fn size_bytes(&self) -> i64 {
        self.bytes.len() as i64
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct DocumentMetadata {
    pub id: String,
    pub nombre_original: String,
    pub tipo_mime: String,
    pub tamano_bytes: i64,
    pub sha256: String,
    pub creado_en: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct CreatedDocument {
    #[serde(flatten)]
    pub documento: DocumentMetadata,
    pub duplicate_content_of: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct DocumentDownloadResponse {
    pub url: String,
    pub expires_in_seconds: u16,
}

#[derive(Debug)]
pub enum DocumentError {
    NotFound,
    StorageUnavailable,
    Database(sqlx::Error),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "document was not found",
            Self::StorageUnavailable => "document storage is unavailable",
            Self::Database(_) => "document metadata operation failed",
        })
    }
}

impl std::error::Error for DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Database(error) => Some(error),
            _ => None,
        }
    }
}

/// Uploads an object before atomically writing its metadata and audit fact.
///
/// PostgreSQL and Storage do not share a transaction. If PostgreSQL or audit
/// persistence fails after `put`, this function rolls back metadata and makes a
/// best-effort delete of the just-created object before returning an error.
pub async fn create(
    db: &PgPool,
    storage: &Arc<dyn DocumentStorage>,
    settings: &DocumentSettings,
    context: &AuthorizationContext,
    upload: DocumentUpload,
) -> Result<CreatedDocument, DocumentError> {
    let document_id = Uuid::new_v4();
    let storage_key = storage_key(context.organization_id, document_id);
    storage
        .put(
            settings.storage_bucket(),
            &storage_key,
            &upload.mime_type,
            &upload.bytes,
        )
        .await
        .map_err(|_| DocumentError::StorageUnavailable)?;

    let mut transaction = db.begin().await.map_err(DocumentError::Database)?;
    let result = create_metadata_and_audit(
        &mut transaction,
        context,
        settings.storage_bucket(),
        &storage_key,
        document_id,
        &upload,
    )
    .await;
    match result {
        Ok((metadata, duplicates)) => match transaction.commit().await {
            Ok(()) => Ok(CreatedDocument {
                documento: metadata,
                duplicate_content_of: duplicates.into_iter().map(|id| id.to_string()).collect(),
            }),
            Err(error) => {
                cleanup_failed_creation(
                    storage,
                    settings.storage_bucket(),
                    &storage_key,
                    document_id,
                )
                .await;
                Err(DocumentError::Database(error))
            }
        },
        Err(error) => {
            let _ = transaction.rollback().await;
            cleanup_failed_creation(
                storage,
                settings.storage_bucket(),
                &storage_key,
                document_id,
            )
            .await;
            Err(DocumentError::Database(error))
        }
    }
}

async fn create_metadata_and_audit(
    transaction: &mut Transaction<'_, Postgres>,
    context: &AuthorizationContext,
    storage_bucket: &str,
    storage_key: &str,
    document_id: Uuid,
    upload: &DocumentUpload,
) -> Result<(DocumentMetadata, Vec<Uuid>), sqlx::Error> {
    let duplicate_ids: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM public.documentos
        WHERE organizacion_id = $1
          AND sha256 = $2
        ORDER BY creado_en ASC, id ASC
        LIMIT $3
        "#,
    )
    .bind(context.organization_id)
    .bind(upload.sha256.as_slice())
    .bind(MAX_DUPLICATE_CONTENT_MATCHES)
    .fetch_all(&mut **transaction)
    .await?;
    let row = sqlx::query(
        r#"
        INSERT INTO public.documentos (
            id, organizacion_id, nombre_original, tipo_mime, tamano_bytes,
            sha256, storage_bucket, storage_key, creado_por
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id, nombre_original, tipo_mime, tamano_bytes, sha256, creado_en
        "#,
    )
    .bind(document_id)
    .bind(context.organization_id)
    .bind(&upload.filename)
    .bind(&upload.mime_type)
    .bind(upload.size_bytes())
    .bind(upload.sha256.as_slice())
    .bind(storage_bucket)
    .bind(storage_key)
    .bind(context.user_id)
    .fetch_one(&mut **transaction)
    .await?;
    let metadata = metadata_from_row(row)?;
    audit::record(
        transaction,
        &NewAuditEvent {
            organization_id: context.organization_id,
            actor: AuditActor::Usuario(context.user_id),
            action: "documento.creado",
            entity_type: "documento",
            entity_id: Some(document_id),
            reference: Some(&upload.filename),
            before_state: None,
            after_state: Some(json!({
                "documento_id": document_id.to_string(),
                "tipo_mime": upload.mime_type,
                "tamano_bytes": upload.size_bytes(),
                "sha256": sha256_hex(&upload.sha256),
            })),
        },
    )
    .await?;
    Ok((metadata, duplicate_ids))
}

async fn cleanup_failed_creation(
    storage: &Arc<dyn DocumentStorage>,
    bucket: &str,
    key: &str,
    document_id: Uuid,
) {
    if storage.delete(bucket, key).await.is_err() {
        warn!(%document_id, "failed to clean up orphaned private document object");
    }
}

pub async fn metadata(
    db: &PgPool,
    organization_id: Uuid,
    document_id: Uuid,
) -> Result<DocumentMetadata, DocumentError> {
    sqlx::query(
        r#"
        SELECT id, nombre_original, tipo_mime, tamano_bytes, sha256, creado_en
        FROM public.documentos
        WHERE id = $1 AND organizacion_id = $2
        "#,
    )
    .bind(document_id)
    .bind(organization_id)
    .fetch_optional(db)
    .await
    .map_err(DocumentError::Database)?
    .map(metadata_from_row)
    .transpose()
    .map_err(DocumentError::Database)?
    .ok_or(DocumentError::NotFound)
}

pub async fn download_access(
    db: &PgPool,
    storage: &Arc<dyn DocumentStorage>,
    organization_id: Uuid,
    document_id: Uuid,
) -> Result<DocumentDownloadResponse, DocumentError> {
    let row = sqlx::query(
        "SELECT storage_bucket, storage_key FROM public.documentos WHERE id = $1 AND organizacion_id = $2",
    )
    .bind(document_id)
    .bind(organization_id)
    .fetch_optional(db)
    .await
    .map_err(DocumentError::Database)?
    .ok_or(DocumentError::NotFound)?;
    let bucket: String = row
        .try_get("storage_bucket")
        .map_err(DocumentError::Database)?;
    let key: String = row
        .try_get("storage_key")
        .map_err(DocumentError::Database)?;
    let access = storage
        .create_download_access(&bucket, &key)
        .await
        .map_err(|_| DocumentError::StorageUnavailable)?;
    Ok(DocumentDownloadResponse {
        url: access.url,
        expires_in_seconds: access.expires_in_seconds,
    })
}

pub fn storage_key(organization_id: Uuid, document_id: Uuid) -> String {
    format!("organizaciones/{organization_id}/documentos/{document_id}/contenido")
}

fn metadata_from_row(row: PgRow) -> Result<DocumentMetadata, sqlx::Error> {
    let sha256: Vec<u8> = row.try_get("sha256")?;
    let created_at: OffsetDateTime = row.try_get("creado_en")?;
    Ok(DocumentMetadata {
        id: row.try_get::<Uuid, _>("id")?.to_string(),
        nombre_original: row.try_get("nombre_original")?,
        tipo_mime: row.try_get("tipo_mime")?,
        tamano_bytes: row.try_get("tamano_bytes")?,
        sha256: sha256_hex(&sha256),
        creado_en: created_at
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|error| sqlx::Error::Decode(Box::new(error)))?,
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        result.push(HEX[(byte & 0x0f) as usize] as char);
    }
    result
}

fn validate_filename(value: &str) -> Result<(), DocumentUploadError> {
    (value == value.trim() && !value.is_empty() && value.chars().count() <= MAX_FILENAME_LENGTH)
        .then_some(())
        .ok_or(DocumentUploadError::InvalidFilename)
}

fn validate_mime_type(value: &str) -> Result<(), DocumentUploadError> {
    (value == value.trim() && !value.is_empty() && value.chars().count() <= MAX_MIME_LENGTH)
        .then_some(())
        .ok_or(DocumentUploadError::InvalidMimeType)
}
