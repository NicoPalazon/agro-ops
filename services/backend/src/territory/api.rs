//! Territorial read HTTP boundary. PostgreSQL geometry is converted to GeoJSON
//! in the territory infrastructure before these DTOs are serialized.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState, RequestAccessError,
    territory::{
        application::{
            self, BasePlotView, CampaignView, EstablishmentView, ExternalReferenceView,
            GeoJsonMultiPolygon, OperationalUnitView, TerritorialUseAssignmentView,
            TerritoryApplicationError,
        },
        infrastructure::PostgresTerritoryStore,
    },
};

#[derive(Serialize, ToSchema)]
pub struct GeoJsonMultiPolygonResponse {
    #[serde(rename = "type")]
    geometry_type: &'static str,
    coordinates: Vec<Vec<Vec<Vec<f64>>>>,
}

#[derive(Serialize, ToSchema)]
pub struct ExternalReferenceResponse {
    sistema_externo: String,
    external_id: String,
}

#[derive(Serialize, ToSchema)]
pub struct BasePlotResponse {
    #[schema(value_type = String)]
    id: Uuid,
    codigo: String,
    nombre: String,
    activo: bool,
    geometria: GeoJsonMultiPolygonResponse,
}

#[derive(Serialize, ToSchema)]
pub struct EstablishmentResponse {
    #[schema(value_type = String)]
    id: Uuid,
    codigo: String,
    nombre: String,
    activo: bool,
    geometria: GeoJsonMultiPolygonResponse,
    referencias_externas: Vec<ExternalReferenceResponse>,
    lotes_base: Vec<BasePlotResponse>,
}

#[derive(Serialize, ToSchema)]
pub struct CampaignResponse {
    #[schema(value_type = String)]
    id: Uuid,
    codigo: String,
    nombre: String,
    #[schema(value_type = String)]
    fecha_inicio: String,
    #[schema(value_type = String)]
    fecha_fin: String,
    activa: bool,
}

#[derive(Serialize, ToSchema)]
pub struct OperationalUnitResponse {
    #[schema(value_type = String)]
    id: Uuid,
    #[schema(value_type = String)]
    campana_id: Uuid,
    #[schema(value_type = String)]
    establecimiento_id: Uuid,
    codigo: String,
    nombre: String,
    activa: bool,
    geometria: GeoJsonMultiPolygonResponse,
    #[schema(value_type = Vec<String>)]
    lote_base_ids: Vec<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct TerritorialUseAssignmentResponse {
    #[schema(value_type = String)]
    id: Uuid,
    #[schema(value_type = String)]
    uso_territorial_id: Uuid,
    uso_codigo: String,
    uso_nombre: String,
    #[schema(value_type = String)]
    fecha_inicio: String,
    #[schema(value_type = String)]
    fecha_fin: String,
}

#[derive(Debug)]
pub(crate) enum TerritoryRequestError {
    Access(RequestAccessError),
    Application(TerritoryApplicationError),
}

impl From<RequestAccessError> for TerritoryRequestError {
    fn from(error: RequestAccessError) -> Self {
        Self::Access(error)
    }
}

impl From<TerritoryApplicationError> for TerritoryRequestError {
    fn from(error: TerritoryApplicationError) -> Self {
        Self::Application(error)
    }
}

impl IntoResponse for TerritoryRequestError {
    fn into_response(self) -> Response {
        match self {
            Self::Access(error) => error.into_response(),
            Self::Application(TerritoryApplicationError::PermissionDenied) => {
                StatusCode::FORBIDDEN.into_response()
            }
            Self::Application(TerritoryApplicationError::NotFound) => {
                StatusCode::NOT_FOUND.into_response()
            }
            Self::Application(TerritoryApplicationError::Unavailable) => {
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            }
            Self::Application(TerritoryApplicationError::Conflict) => {
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/territorio/establecimientos", get(list_establecimientos))
        .route(
            "/territorio/establecimientos/{establecimiento_id}",
            get(get_establecimiento),
        )
        .route("/territorio/campanas", get(list_campanas))
        .route("/territorio/campanas/{campana_id}", get(get_campana))
        .route(
            "/territorio/campanas/{campana_id}/establecimientos/{establecimiento_id}/unidades-operativas",
            get(list_unidades_operativas),
        )
        .route(
            "/territorio/unidades-operativas/{unidad_operativa_id}/usos",
            get(list_usos_unidad_operativa),
        )
}

#[utoipa::path(
    get,
    path = "/territorio/establecimientos",
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Establishments visible to the caller organization.", body = [EstablishmentResponse]),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The caller lacks territorio:ver."),
        (status = 503, description = "Authentication, authorization, or territory storage is unavailable.")
    )
)]
pub(crate) async fn list_establecimientos(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<EstablishmentResponse>>, TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    Ok(Json(
        application::list_establecimientos(&store, &context)
            .await?
            .into_iter()
            .map(EstablishmentResponse::from)
            .collect(),
    ))
}

#[utoipa::path(
    get,
    path = "/territorio/establecimientos/{establecimiento_id}",
    params(("establecimiento_id" = String, Path, description = "Agro Ops establishment UUID.")),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Establishment detail.", body = EstablishmentResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The caller lacks territorio:ver."),
        (status = 404, description = "The establishment is not visible to the caller organization."),
        (status = 503, description = "Authentication, authorization, or territory storage is unavailable.")
    )
)]
pub(crate) async fn get_establecimiento(
    State(state): State<AppState>,
    Path(establecimiento_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<EstablishmentResponse>, TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    Ok(Json(
        application::get_establecimiento(&store, &context, establecimiento_id)
            .await?
            .into(),
    ))
}

#[utoipa::path(
    get,
    path = "/territorio/campanas",
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Campaigns visible to the caller organization.", body = [CampaignResponse]),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The caller lacks territorio:ver."),
        (status = 503, description = "Authentication, authorization, or territory storage is unavailable.")
    )
)]
pub(crate) async fn list_campanas(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<CampaignResponse>>, TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    Ok(Json(
        application::list_campanas(&store, &context)
            .await?
            .into_iter()
            .map(CampaignResponse::from)
            .collect(),
    ))
}

#[utoipa::path(
    get,
    path = "/territorio/campanas/{campana_id}",
    params(("campana_id" = String, Path, description = "Agro Ops campaign UUID.")),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Campaign detail.", body = CampaignResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The caller lacks territorio:ver."),
        (status = 404, description = "The campaign is not visible to the caller organization."),
        (status = 503, description = "Authentication, authorization, or territory storage is unavailable.")
    )
)]
pub(crate) async fn get_campana(
    State(state): State<AppState>,
    Path(campana_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<CampaignResponse>, TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    Ok(Json(
        application::get_campana(&store, &context, campana_id)
            .await?
            .into(),
    ))
}

#[utoipa::path(
    get,
    path = "/territorio/campanas/{campana_id}/establecimientos/{establecimiento_id}/unidades-operativas",
    params(
        ("campana_id" = String, Path, description = "Agro Ops campaign UUID."),
        ("establecimiento_id" = String, Path, description = "Agro Ops establishment UUID.")
    ),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Operational units in the campaign and establishment context.", body = [OperationalUnitResponse]),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The caller lacks territorio:ver."),
        (status = 404, description = "The campaign or establishment is not visible to the caller organization."),
        (status = 503, description = "Authentication, authorization, or territory storage is unavailable.")
    )
)]
pub(crate) async fn list_unidades_operativas(
    State(state): State<AppState>,
    Path((campana_id, establecimiento_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<Vec<OperationalUnitResponse>>, TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    Ok(Json(
        application::list_unidades_operativas(&store, &context, campana_id, establecimiento_id)
            .await?
            .into_iter()
            .map(OperationalUnitResponse::from)
            .collect(),
    ))
}

#[utoipa::path(
    get,
    path = "/territorio/unidades-operativas/{unidad_operativa_id}/usos",
    params(("unidad_operativa_id" = String, Path, description = "Agro Ops operational-unit UUID.")),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Immutable territorial-use assignment history for the operational unit.", body = [TerritorialUseAssignmentResponse]),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The caller lacks territorio:ver."),
        (status = 404, description = "The operational unit is not visible to the caller organization."),
        (status = 503, description = "Authentication, authorization, or territory storage is unavailable.")
    )
)]
pub(crate) async fn list_usos_unidad_operativa(
    State(state): State<AppState>,
    Path(unidad_operativa_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Vec<TerritorialUseAssignmentResponse>>, TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    Ok(Json(
        application::list_usos_unidad_operativa(&store, &context, unidad_operativa_id)
            .await?
            .into_iter()
            .map(TerritorialUseAssignmentResponse::from)
            .collect(),
    ))
}

impl From<GeoJsonMultiPolygon> for GeoJsonMultiPolygonResponse {
    fn from(value: GeoJsonMultiPolygon) -> Self {
        Self {
            geometry_type: "MultiPolygon",
            coordinates: value.coordinates,
        }
    }
}

impl From<ExternalReferenceView> for ExternalReferenceResponse {
    fn from(value: ExternalReferenceView) -> Self {
        Self {
            sistema_externo: value.sistema_externo,
            external_id: value.external_id,
        }
    }
}

impl From<BasePlotView> for BasePlotResponse {
    fn from(value: BasePlotView) -> Self {
        Self {
            id: value.id,
            codigo: value.codigo,
            nombre: value.nombre,
            activo: value.activo,
            geometria: value.geometria.into(),
        }
    }
}

impl From<EstablishmentView> for EstablishmentResponse {
    fn from(value: EstablishmentView) -> Self {
        Self {
            id: value.id,
            codigo: value.codigo,
            nombre: value.nombre,
            activo: value.activo,
            geometria: value.geometria.into(),
            referencias_externas: value
                .referencias_externas
                .into_iter()
                .map(Into::into)
                .collect(),
            lotes_base: value.lotes_base.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<CampaignView> for CampaignResponse {
    fn from(value: CampaignView) -> Self {
        Self {
            id: value.id,
            codigo: value.codigo,
            nombre: value.nombre,
            fecha_inicio: value.fecha_inicio.to_string(),
            fecha_fin: value.fecha_fin.to_string(),
            activa: value.activa,
        }
    }
}

impl From<OperationalUnitView> for OperationalUnitResponse {
    fn from(value: OperationalUnitView) -> Self {
        Self {
            id: value.id,
            campana_id: value.campana_id,
            establecimiento_id: value.establecimiento_id,
            codigo: value.codigo,
            nombre: value.nombre,
            activa: value.activa,
            geometria: value.geometria.into(),
            lote_base_ids: value.lote_base_ids,
        }
    }
}

impl From<TerritorialUseAssignmentView> for TerritorialUseAssignmentResponse {
    fn from(value: TerritorialUseAssignmentView) -> Self {
        Self {
            id: value.id,
            uso_territorial_id: value.uso_territorial_id,
            uso_codigo: value.uso_codigo,
            uso_nombre: value.uso_nombre,
            fecha_inicio: value.fecha_inicio.to_string(),
            fecha_fin: value.fecha_fin.to_string(),
        }
    }
}
