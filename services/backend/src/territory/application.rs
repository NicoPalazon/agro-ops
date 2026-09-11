use async_trait::async_trait;
use serde_json::json;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::{
    authorization::{
        AuthorizationContext, PermissionDenied,
        permission_codes::{TERRITORIO_CREAR, TERRITORIO_VER},
    },
    external_references::ExternalId,
    idempotency::{IdempotencyKey, RequestFingerprint},
    territory::{
        domain::{Establecimiento, LoteBase, NewEstablecimiento, NewLoteBase, TERRITORIAL_SRID},
        geographic_source::{
            ExternalSourceName, GeographicSourceFingerprint, GeographicSourceType,
        },
        senasa::{self, NormalizedPolygon4326, SenasaPolygonParseError},
        source_grouping::{ConfirmedSourceSet, InvalidConfirmedSourceSet},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerritoryStoreError {
    Conflict,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SenasaPolygonPreview {
    pub source_coordinate_pair_count: usize,
    pub geometry: GeoJsonMultiPolygon,
    pub srid: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerritoryValidationError {
    pub code: &'static str,
    pub message: String,
    pub detail: Option<String>,
    pub line: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerritoryPreviewStoreError {
    InvalidTopology { reason: String },
    EmptyGeometry,
    UnexpectedSrid,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeographicSourceView {
    pub id: Uuid,
    pub establecimiento_id: Uuid,
    pub external_reference_id: Uuid,
    pub external_id: String,
    pub source_type: GeographicSourceType,
    pub external_name: Option<String>,
    pub original_source_text: String,
    pub geometry: GeoJsonMultiPolygon,
    pub fingerprint: GeographicSourceFingerprint,
    pub parser_version: String,
    pub created_by: Uuid,
    pub created_at: OffsetDateTime,
    pub confirmed_for_canonical_geometry: bool,
    pub confirmed_by: Option<Uuid>,
    pub confirmed_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfirmedGeographicSource {
    pub source: GeographicSourceView,
    pub created: bool,
}

#[derive(Clone, Debug)]
pub struct NewSenasaGeographicSource {
    pub establecimiento_id: Uuid,
    pub external_id: ExternalId,
    pub external_name: Option<ExternalSourceName>,
    pub original_source_text: String,
    pub normalized_polygon: NormalizedPolygon4326,
    pub fingerprint: GeographicSourceFingerprint,
    pub idempotency_key: IdempotencyKey,
    pub idempotency_request_fingerprint: RequestFingerprint,
}

#[derive(Clone, Debug)]
pub struct NewCanonicalSourceGrouping {
    pub establecimiento_id: Uuid,
    pub contributors: ConfirmedSourceSet,
    pub idempotency_key: IdempotencyKey,
    pub idempotency_request_fingerprint: RequestFingerprint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanonicalSourceGrouping {
    pub establecimiento_id: Uuid,
    pub source_ids: Vec<Uuid>,
    pub geometry: GeoJsonMultiPolygon,
    pub sources_area_m2: f64,
    pub canonical_area_m2: f64,
    pub overlap_area_m2: f64,
    pub overlap_detected: bool,
    pub updated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerritorySourceStoreError {
    NotFound,
    Conflict,
    Validation(TerritoryPreviewStoreError),
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerritorySourceGroupingStoreError {
    NotFound,
    Conflict,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeoJsonMultiPolygon {
    pub coordinates: Vec<Vec<Vec<Vec<f64>>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalReferenceView {
    pub sistema_externo: String,
    pub external_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BasePlotView {
    pub id: Uuid,
    pub codigo: String,
    pub nombre: String,
    pub activo: bool,
    pub geometria: GeoJsonMultiPolygon,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EstablishmentView {
    pub id: Uuid,
    pub codigo: String,
    pub nombre: String,
    pub activo: bool,
    pub geometria: GeoJsonMultiPolygon,
    pub referencias_externas: Vec<ExternalReferenceView>,
    pub lotes_base: Vec<BasePlotView>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CampaignView {
    pub id: Uuid,
    pub codigo: String,
    pub nombre: String,
    pub fecha_inicio: Date,
    pub fecha_fin: Date,
    pub activa: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperationalUnitView {
    pub id: Uuid,
    pub campana_id: Uuid,
    pub establecimiento_id: Uuid,
    pub codigo: String,
    pub nombre: String,
    pub activa: bool,
    pub geometria: GeoJsonMultiPolygon,
    pub lote_base_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerritorialUseAssignmentView {
    pub id: Uuid,
    pub uso_territorial_id: Uuid,
    pub uso_codigo: String,
    pub uso_nombre: String,
    pub fecha_inicio: Date,
    pub fecha_fin: Date,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerritoryReadStoreError {
    NotFound,
    Unavailable,
}

#[async_trait]
pub trait TerritoryReadStore: Send + Sync {
    async fn list_establecimientos(
        &self,
        organization_id: Uuid,
    ) -> Result<Vec<EstablishmentView>, TerritoryReadStoreError>;

    async fn get_establecimiento(
        &self,
        organization_id: Uuid,
        establecimiento_id: Uuid,
    ) -> Result<EstablishmentView, TerritoryReadStoreError>;

    async fn list_campanas(
        &self,
        organization_id: Uuid,
    ) -> Result<Vec<CampaignView>, TerritoryReadStoreError>;

    async fn get_campana(
        &self,
        organization_id: Uuid,
        campana_id: Uuid,
    ) -> Result<CampaignView, TerritoryReadStoreError>;

    async fn list_unidades_operativas(
        &self,
        organization_id: Uuid,
        campana_id: Uuid,
        establecimiento_id: Uuid,
    ) -> Result<Vec<OperationalUnitView>, TerritoryReadStoreError>;

    async fn list_usos_unidad_operativa(
        &self,
        organization_id: Uuid,
        unidad_operativa_id: Uuid,
    ) -> Result<Vec<TerritorialUseAssignmentView>, TerritoryReadStoreError>;
}

#[async_trait]
pub trait TerritoryStore: Send + Sync {
    async fn create_establecimiento(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        new_establecimiento: NewEstablecimiento,
    ) -> Result<Establecimiento, TerritoryStoreError>;

    async fn create_lote_base(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        new_lote_base: NewLoteBase,
    ) -> Result<LoteBase, TerritoryStoreError>;
}

/// Read-only PostGIS validation boundary for normalized external geometry.
#[async_trait]
pub trait TerritoryPreviewStore: Send + Sync {
    async fn preview_normalized_polygon(
        &self,
        polygon: &NormalizedPolygon4326,
    ) -> Result<GeoJsonMultiPolygon, TerritoryPreviewStoreError>;
}

/// Transactional source-evidence persistence and provenance read boundary.
#[async_trait]
pub trait TerritorySourceStore: Send + Sync {
    async fn confirm_senasa_source(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        source: NewSenasaGeographicSource,
    ) -> Result<ConfirmedGeographicSource, TerritorySourceStoreError>;

    async fn list_geographic_sources(
        &self,
        organization_id: Uuid,
        establecimiento_id: Uuid,
    ) -> Result<Vec<GeographicSourceView>, TerritorySourceStoreError>;
}

/// Transactional boundary for the explicit current contributor set and its
/// PostGIS-derived canonical establishment geometry.
#[async_trait]
pub trait TerritorySourceGroupingStore: Send + Sync {
    async fn set_canonical_source_contributors(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        grouping: NewCanonicalSourceGrouping,
    ) -> Result<CanonicalSourceGrouping, TerritorySourceGroupingStoreError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerritoryApplicationError {
    Validation(TerritoryValidationError),
    PermissionDenied,
    NotFound,
    Conflict,
    Unavailable,
}

pub async fn preview_senasa_polygon(
    store: &dyn TerritoryPreviewStore,
    context: &AuthorizationContext,
    source_text: &str,
) -> Result<SenasaPolygonPreview, TerritoryApplicationError> {
    context
        .require_permission(TERRITORIO_CREAR)
        .map_err(|PermissionDenied| TerritoryApplicationError::PermissionDenied)?;
    let polygon = senasa::parse_polygon(source_text).map_err(parser_validation_error)?;
    let geometry = store
        .preview_normalized_polygon(&polygon)
        .await
        .map_err(map_preview_store_error)?;

    Ok(SenasaPolygonPreview {
        source_coordinate_pair_count: polygon.source_coordinate_pair_count(),
        geometry,
        srid: TERRITORIAL_SRID,
    })
}

pub async fn confirm_senasa_source(
    store: &dyn TerritorySourceStore,
    context: &AuthorizationContext,
    establecimiento_id: Uuid,
    external_id: String,
    external_name: Option<String>,
    source_text: String,
    idempotency_key: IdempotencyKey,
) -> Result<ConfirmedGeographicSource, TerritoryApplicationError> {
    context
        .require_permission(TERRITORIO_CREAR)
        .map_err(|PermissionDenied| TerritoryApplicationError::PermissionDenied)?;
    let external_id = ExternalId::new(external_id).map_err(|_| {
        TerritoryApplicationError::Validation(TerritoryValidationError {
            code: "referencia_externa_invalida",
            message: "La referencia externa debe ser texto válido y sin espacios laterales."
                .to_owned(),
            detail: None,
            line: None,
        })
    })?;
    let external_name = external_name
        .map(ExternalSourceName::new)
        .transpose()
        .map_err(|_| {
            TerritoryApplicationError::Validation(TerritoryValidationError {
                code: "nombre_externo_invalido",
                message: "El nombre externo debe ser texto válido y sin espacios laterales."
                    .to_owned(),
                detail: None,
                line: None,
            })
        })?;
    let normalized_polygon =
        senasa::parse_polygon(&source_text).map_err(parser_validation_error)?;
    let fingerprint = GeographicSourceFingerprint::for_senasa(
        establecimiento_id,
        &external_id,
        external_name.as_ref(),
        &normalized_polygon,
    );
    let request_fingerprint = RequestFingerprint::sha256(fingerprint.as_bytes());

    store
        .confirm_senasa_source(
            context.organization_id,
            context.user_id,
            NewSenasaGeographicSource {
                establecimiento_id,
                external_id,
                external_name,
                original_source_text: source_text,
                normalized_polygon,
                fingerprint,
                idempotency_key,
                idempotency_request_fingerprint: request_fingerprint,
            },
        )
        .await
        .map_err(map_source_store_error)
}

pub async fn list_geographic_sources(
    store: &dyn TerritorySourceStore,
    context: &AuthorizationContext,
    establecimiento_id: Uuid,
) -> Result<Vec<GeographicSourceView>, TerritoryApplicationError> {
    require_read_permission(context)?;
    store
        .list_geographic_sources(context.organization_id, establecimiento_id)
        .await
        .map_err(map_source_store_error)
}

pub async fn set_canonical_source_contributors(
    store: &dyn TerritorySourceGroupingStore,
    context: &AuthorizationContext,
    establecimiento_id: Uuid,
    source_ids: Vec<Uuid>,
    idempotency_key: IdempotencyKey,
) -> Result<CanonicalSourceGrouping, TerritoryApplicationError> {
    context
        .require_permission(TERRITORIO_CREAR)
        .map_err(|PermissionDenied| TerritoryApplicationError::PermissionDenied)?;
    let contributors = ConfirmedSourceSet::new(source_ids).map_err(grouping_validation_error)?;
    let canonical_request = serde_json::to_vec(&json!({
        "establecimiento_id": establecimiento_id,
        "fuente_geografica_ids": contributors.source_ids(),
    }))
    .expect("canonical source-grouping request serialization must succeed");

    store
        .set_canonical_source_contributors(
            context.organization_id,
            context.user_id,
            NewCanonicalSourceGrouping {
                establecimiento_id,
                contributors,
                idempotency_key,
                idempotency_request_fingerprint: RequestFingerprint::sha256(&canonical_request),
            },
        )
        .await
        .map_err(map_source_grouping_store_error)
}

pub async fn list_establecimientos(
    store: &dyn TerritoryReadStore,
    context: &AuthorizationContext,
) -> Result<Vec<EstablishmentView>, TerritoryApplicationError> {
    require_read_permission(context)?;
    store
        .list_establecimientos(context.organization_id)
        .await
        .map_err(map_read_store_error)
}

pub async fn get_establecimiento(
    store: &dyn TerritoryReadStore,
    context: &AuthorizationContext,
    establecimiento_id: Uuid,
) -> Result<EstablishmentView, TerritoryApplicationError> {
    require_read_permission(context)?;
    store
        .get_establecimiento(context.organization_id, establecimiento_id)
        .await
        .map_err(map_read_store_error)
}

pub async fn list_campanas(
    store: &dyn TerritoryReadStore,
    context: &AuthorizationContext,
) -> Result<Vec<CampaignView>, TerritoryApplicationError> {
    require_read_permission(context)?;
    store
        .list_campanas(context.organization_id)
        .await
        .map_err(map_read_store_error)
}

pub async fn get_campana(
    store: &dyn TerritoryReadStore,
    context: &AuthorizationContext,
    campana_id: Uuid,
) -> Result<CampaignView, TerritoryApplicationError> {
    require_read_permission(context)?;
    store
        .get_campana(context.organization_id, campana_id)
        .await
        .map_err(map_read_store_error)
}

pub async fn list_unidades_operativas(
    store: &dyn TerritoryReadStore,
    context: &AuthorizationContext,
    campana_id: Uuid,
    establecimiento_id: Uuid,
) -> Result<Vec<OperationalUnitView>, TerritoryApplicationError> {
    require_read_permission(context)?;
    store
        .list_unidades_operativas(context.organization_id, campana_id, establecimiento_id)
        .await
        .map_err(map_read_store_error)
}

pub async fn list_usos_unidad_operativa(
    store: &dyn TerritoryReadStore,
    context: &AuthorizationContext,
    unidad_operativa_id: Uuid,
) -> Result<Vec<TerritorialUseAssignmentView>, TerritoryApplicationError> {
    require_read_permission(context)?;
    store
        .list_usos_unidad_operativa(context.organization_id, unidad_operativa_id)
        .await
        .map_err(map_read_store_error)
}

fn require_read_permission(
    context: &AuthorizationContext,
) -> Result<(), TerritoryApplicationError> {
    context
        .require_permission(TERRITORIO_VER)
        .map_err(|PermissionDenied| TerritoryApplicationError::PermissionDenied)
}

pub async fn create_establecimiento(
    store: &dyn TerritoryStore,
    context: &AuthorizationContext,
    new_establecimiento: NewEstablecimiento,
) -> Result<Establecimiento, TerritoryApplicationError> {
    context
        .require_permission(TERRITORIO_CREAR)
        .map_err(|PermissionDenied| TerritoryApplicationError::PermissionDenied)?;
    store
        .create_establecimiento(
            context.organization_id,
            context.user_id,
            new_establecimiento,
        )
        .await
        .map_err(map_store_error)
}

pub async fn create_lote_base(
    store: &dyn TerritoryStore,
    context: &AuthorizationContext,
    new_lote_base: NewLoteBase,
) -> Result<LoteBase, TerritoryApplicationError> {
    context
        .require_permission(TERRITORIO_CREAR)
        .map_err(|PermissionDenied| TerritoryApplicationError::PermissionDenied)?;
    store
        .create_lote_base(context.organization_id, context.user_id, new_lote_base)
        .await
        .map_err(map_store_error)
}

fn map_store_error(error: TerritoryStoreError) -> TerritoryApplicationError {
    match error {
        TerritoryStoreError::Conflict => TerritoryApplicationError::Conflict,
        TerritoryStoreError::Unavailable => TerritoryApplicationError::Unavailable,
    }
}

fn parser_validation_error(error: SenasaPolygonParseError) -> TerritoryApplicationError {
    TerritoryApplicationError::Validation(TerritoryValidationError {
        code: error.kind.code(),
        message: error.kind.message().to_owned(),
        detail: None,
        line: error.line,
    })
}

fn map_preview_store_error(error: TerritoryPreviewStoreError) -> TerritoryApplicationError {
    match error {
        TerritoryPreviewStoreError::InvalidTopology { reason } => {
            TerritoryApplicationError::Validation(TerritoryValidationError {
                code: "topologia_invalida",
                message: "La geometría contiene una topología inválida.".to_owned(),
                detail: Some(reason),
                line: None,
            })
        }
        TerritoryPreviewStoreError::EmptyGeometry => {
            TerritoryApplicationError::Validation(TerritoryValidationError {
                code: "geometria_vacia",
                message: "La geometría no puede estar vacía.".to_owned(),
                detail: None,
                line: None,
            })
        }
        TerritoryPreviewStoreError::UnexpectedSrid => {
            TerritoryApplicationError::Validation(TerritoryValidationError {
                code: "srid_invalido",
                message: "La geometría debe usar SRID 4326.".to_owned(),
                detail: None,
                line: None,
            })
        }
        TerritoryPreviewStoreError::Unavailable => TerritoryApplicationError::Unavailable,
    }
}

fn map_source_store_error(error: TerritorySourceStoreError) -> TerritoryApplicationError {
    match error {
        TerritorySourceStoreError::NotFound => TerritoryApplicationError::NotFound,
        TerritorySourceStoreError::Conflict => TerritoryApplicationError::Conflict,
        TerritorySourceStoreError::Validation(error) => map_preview_store_error(error),
        TerritorySourceStoreError::Unavailable => TerritoryApplicationError::Unavailable,
    }
}

fn grouping_validation_error(error: InvalidConfirmedSourceSet) -> TerritoryApplicationError {
    let (code, message) = match error {
        InvalidConfirmedSourceSet::Empty => (
            "fuentes_confirmadas_requeridas",
            "Debe confirmar al menos una fuente geográfica.",
        ),
        InvalidConfirmedSourceSet::TooMany => (
            "demasiadas_fuentes_confirmadas",
            "La cantidad de fuentes geográficas supera el límite permitido.",
        ),
        InvalidConfirmedSourceSet::Duplicate => (
            "fuente_confirmada_duplicada",
            "Cada fuente geográfica debe aparecer una sola vez.",
        ),
    };
    TerritoryApplicationError::Validation(TerritoryValidationError {
        code,
        message: message.to_owned(),
        detail: None,
        line: None,
    })
}

fn map_source_grouping_store_error(
    error: TerritorySourceGroupingStoreError,
) -> TerritoryApplicationError {
    match error {
        TerritorySourceGroupingStoreError::NotFound => TerritoryApplicationError::NotFound,
        TerritorySourceGroupingStoreError::Conflict => TerritoryApplicationError::Conflict,
        TerritorySourceGroupingStoreError::Unavailable => TerritoryApplicationError::Unavailable,
    }
}

fn map_read_store_error(error: TerritoryReadStoreError) -> TerritoryApplicationError {
    match error {
        TerritoryReadStoreError::NotFound => TerritoryApplicationError::NotFound,
        TerritoryReadStoreError::Unavailable => TerritoryApplicationError::Unavailable,
    }
}
