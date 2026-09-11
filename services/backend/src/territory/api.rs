//! Territorial read HTTP boundary. PostgreSQL geometry is converted to GeoJSON
//! in the territory infrastructure before these DTOs are serialized.

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState, RequestAccessError,
    idempotency::IdempotencyKey,
    territory::{
        application::{
            self, BasePlotView, CampaignView, CanonicalSourceGrouping,
            CreatedAlternativeEstablecimiento, CreatedGeoJsonLoteBase, EstablishmentView,
            ExternalReferenceView, GeoJsonGeometryPreview, GeoJsonMultiPolygon,
            GeographicSourceView, OperationalUnitView, SenasaPolygonPreview,
            TerritorialUseAssignmentView, TerritoryApplicationError, TerritoryValidationError,
        },
        geographic_source::GeographicSourceType,
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

#[derive(Deserialize, ToSchema)]
pub struct SenasaPolygonPreviewRequest {
    /// One `latitud, longitud` source pair per line.
    texto_poligono: String,
}

#[derive(Serialize, ToSchema)]
pub struct SenasaPolygonPreviewResponse {
    cantidad_pares_coordenadas_fuente: usize,
    geometria: GeoJsonMultiPolygonResponse,
    tipo_geometria: &'static str,
    srid: i32,
    valida: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct GeoJsonGeometryPreviewRequest {
    geometry: serde_json::Value,
}

#[derive(Serialize, ToSchema)]
pub struct GeoJsonGeometryPreviewResponse {
    geometria: GeoJsonMultiPolygonResponse,
    tipo_geometria: &'static str,
    srid: i32,
    valida: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateAlternativeEstablecimientoRequest {
    codigo: String,
    nombre: String,
    tipo_origen: String,
    geometry: serde_json::Value,
    renspa: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedAlternativeEstablecimientoResponse {
    #[schema(value_type = String)]
    establecimiento_id: Uuid,
    codigo: String,
    nombre: String,
    origen_geometria: &'static str,
    fuente_geografica: GeographicSourceResponse,
    creada: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateGeoJsonLoteBaseRequest {
    codigo: String,
    nombre: String,
    geometry: serde_json::Value,
}

#[derive(Serialize, ToSchema)]
pub struct CreatedGeoJsonLoteBaseResponse {
    #[schema(value_type = String)]
    lote_base_id: Uuid,
    establecimiento_id: Uuid,
    codigo: String,
    nombre: String,
    creado: bool,
}

#[derive(Serialize, ToSchema)]
pub struct TerritorialValidationErrorResponse {
    codigo: &'static str,
    mensaje: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detalle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    linea: Option<usize>,
}

#[derive(Deserialize, ToSchema)]
pub struct ConfirmSenasaGeographicSourceRequest {
    external_id: String,
    nombre_externo: Option<String>,
    texto_poligono: String,
}

#[derive(Serialize, ToSchema)]
pub struct GeographicSourceResponse {
    #[schema(value_type = String)]
    id: Uuid,
    #[schema(value_type = String)]
    establecimiento_id: Uuid,
    #[schema(value_type = String)]
    external_reference_id: Option<Uuid>,
    external_id: Option<String>,
    tipo_origen: &'static str,
    nombre_externo: Option<String>,
    texto_fuente_original: String,
    geometria: GeoJsonMultiPolygonResponse,
    huella_sha256: String,
    version_parser: String,
    #[schema(value_type = String)]
    creado_por: Uuid,
    creado_en: String,
    confirmada_para_geometria_canonica: bool,
    #[schema(value_type = Option<String>)]
    confirmada_por: Option<Uuid>,
    confirmada_en: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ConfirmedGeographicSourceResponse {
    fuente: GeographicSourceResponse,
    creada: bool,
}

#[derive(Deserialize, ToSchema)]
pub struct SetCanonicalSourceContributorsRequest {
    #[schema(value_type = Vec<String>)]
    fuente_geografica_ids: Vec<Uuid>,
}

#[derive(Serialize, ToSchema)]
pub struct CanonicalSourceGroupingResponse {
    #[schema(value_type = String)]
    establecimiento_id: Uuid,
    #[schema(value_type = Vec<String>)]
    fuente_geografica_ids: Vec<Uuid>,
    geometria_canonica: GeoJsonMultiPolygonResponse,
    area_fuentes_m2: f64,
    area_canonica_m2: f64,
    area_superpuesta_m2: f64,
    superposicion_detectada: bool,
    actualizada: bool,
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
            Self::Application(TerritoryApplicationError::Validation(error)) => (
                StatusCode::BAD_REQUEST,
                Json(TerritorialValidationErrorResponse::from(error)),
            )
                .into_response(),
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
                StatusCode::CONFLICT.into_response()
            }
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/territorio/establecimientos", get(list_establecimientos).post(create_alternative_establecimiento))
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
        .route(
            "/territorio/previsualizaciones/senasa-poligono",
            post(preview_senasa_polygon),
        )
        .route("/territorio/previsualizaciones/geojson", post(preview_geojson_geometry))
        .route("/territorio/establecimientos/{establecimiento_id}/lotes-base", post(create_geojson_lote_base))
        .route(
            "/territorio/establecimientos/{establecimiento_id}/fuentes-geograficas/senasa",
            post(confirm_senasa_geographic_source),
        )
        .route(
            "/territorio/establecimientos/{establecimiento_id}/fuentes-geograficas",
            get(list_geographic_sources),
        )
        .route(
            "/territorio/establecimientos/{establecimiento_id}/fuentes-geograficas-confirmadas",
            post(set_canonical_source_contributors),
        )
}

#[utoipa::path(
    post,
    path = "/territorio/establecimientos/{establecimiento_id}/fuentes-geograficas/senasa",
    params(("establecimiento_id" = String, Path, description = "Agro Ops establishment UUID.")),
    request_body = ConfirmSenasaGeographicSourceRequest,
    security(("supabaseBearer" = [])),
    responses(
        (status = 201, description = "SENASA source evidence was persisted.", body = ConfirmedGeographicSourceResponse),
        (status = 200, description = "A duplicate source or idempotent replay returned its existing evidence.", body = ConfirmedGeographicSourceResponse),
        (status = 400, description = "The source, idempotency key, or topology is invalid.", body = TerritorialValidationErrorResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The caller lacks territorio:crear."),
        (status = 404, description = "The establishment is not visible to the caller organization."),
        (status = 409, description = "The external reference or idempotency key conflicts."),
        (status = 503, description = "Authorization or territorial storage is unavailable.")
    )
)]
pub(crate) async fn confirm_senasa_geographic_source(
    State(state): State<AppState>,
    Path(establecimiento_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<ConfirmSenasaGeographicSourceRequest>,
) -> Result<(StatusCode, Json<ConfirmedGeographicSourceResponse>), TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let idempotency_key = source_idempotency_key(&headers)?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    let confirmed = application::confirm_senasa_source(
        &store,
        &context,
        establecimiento_id,
        request.external_id,
        request.nombre_externo,
        request.texto_poligono,
        idempotency_key,
    )
    .await?;
    let status = if confirmed.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(confirmed.into())))
}

#[utoipa::path(
    get,
    path = "/territorio/establecimientos/{establecimiento_id}/fuentes-geograficas",
    params(("establecimiento_id" = String, Path, description = "Agro Ops establishment UUID.")),
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Individual source evidence; it is distinct from canonical establishment geometry.", body = [GeographicSourceResponse]),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The caller lacks territorio:ver."),
        (status = 404, description = "The establishment is not visible to the caller organization."),
        (status = 503, description = "Authorization or territorial storage is unavailable.")
    )
)]
pub(crate) async fn list_geographic_sources(
    State(state): State<AppState>,
    Path(establecimiento_id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Vec<GeographicSourceResponse>>, TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    Ok(Json(
        application::list_geographic_sources(&store, &context, establecimiento_id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

#[utoipa::path(
    post,
    path = "/territorio/establecimientos/{establecimiento_id}/fuentes-geograficas-confirmadas",
    params(("establecimiento_id" = String, Path, description = "Agro Ops establishment UUID.")),
    request_body = SetCanonicalSourceContributorsRequest,
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "The explicit contributor set and exact canonical union were applied atomically.", body = CanonicalSourceGroupingResponse),
        (status = 400, description = "The contributor set or idempotency key is invalid.", body = TerritorialValidationErrorResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The caller lacks territorio:crear."),
        (status = 404, description = "The establishment or a selected source is outside the caller scope."),
        (status = 409, description = "The grouping conflicts with current territorial invariants or idempotency."),
        (status = 503, description = "Territorial storage is unavailable.")
    )
)]
pub(crate) async fn set_canonical_source_contributors(
    State(state): State<AppState>,
    Path(establecimiento_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<SetCanonicalSourceContributorsRequest>,
) -> Result<Json<CanonicalSourceGroupingResponse>, TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let idempotency_key = grouping_idempotency_key(&headers)?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    application::set_canonical_source_contributors(
        &store,
        &context,
        establecimiento_id,
        request.fuente_geografica_ids,
        idempotency_key,
    )
    .await
    .map(CanonicalSourceGroupingResponse::from)
    .map(Json)
    .map_err(Into::into)
}

#[utoipa::path(
    post,
    path = "/territorio/previsualizaciones/senasa-poligono",
    request_body = SenasaPolygonPreviewRequest,
    security(("supabaseBearer" = [])),
    responses(
        (status = 200, description = "Read-only normalized MultiPolygon preview.", body = SenasaPolygonPreviewResponse),
        (status = 400, description = "The pasted source or its topology is invalid.", body = TerritorialValidationErrorResponse),
        (status = 401, description = "A valid Supabase user access token is required."),
        (status = 403, description = "The caller lacks territorio:crear."),
        (status = 503, description = "Authentication or PostGIS validation is unavailable.")
    )
)]
pub(crate) async fn preview_senasa_polygon(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SenasaPolygonPreviewRequest>,
) -> Result<Json<SenasaPolygonPreviewResponse>, TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    application::preview_senasa_polygon(&store, &context, &request.texto_poligono)
        .await
        .map(SenasaPolygonPreviewResponse::from)
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(post, path = "/territorio/previsualizaciones/geojson", request_body = GeoJsonGeometryPreviewRequest, security(("supabaseBearer" = [])), responses((status = 200, body = GeoJsonGeometryPreviewResponse), (status = 400, body = TerritorialValidationErrorResponse), (status = 401), (status = 403), (status = 503)))]
pub(crate) async fn preview_geojson_geometry(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<GeoJsonGeometryPreviewRequest>,
) -> Result<Json<GeoJsonGeometryPreviewResponse>, TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    application::preview_geojson_geometry(&store, &context, request.geometry)
        .await
        .map(GeoJsonGeometryPreviewResponse::from)
        .map(Json)
        .map_err(Into::into)
}

#[utoipa::path(post, path = "/territorio/establecimientos", request_body = CreateAlternativeEstablecimientoRequest, security(("supabaseBearer" = [])), responses((status = 201, body = CreatedAlternativeEstablecimientoResponse), (status = 200, body = CreatedAlternativeEstablecimientoResponse), (status = 400, body = TerritorialValidationErrorResponse), (status = 401), (status = 403), (status = 409), (status = 503)))]
pub(crate) async fn create_alternative_establecimiento(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateAlternativeEstablecimientoRequest>,
) -> Result<(StatusCode, Json<CreatedAlternativeEstablecimientoResponse>), TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let idempotency_key = geojson_idempotency_key(&headers)?;
    let source_type = match request.tipo_origen.as_str() {
        "manual" => GeographicSourceType::Manual,
        "importada" => GeographicSourceType::Importada,
        _ => {
            return Err(TerritoryRequestError::Application(
                TerritoryApplicationError::Validation(TerritoryValidationError {
                    code: "tipo_origen_invalido",
                    message: "El origen debe ser manual o importada.".to_owned(),
                    detail: None,
                    line: None,
                }),
            ));
        }
    };
    let store = PostgresTerritoryStore::new(state.db.clone());
    let created = application::create_alternative_establecimiento(
        &store,
        &context,
        request.codigo,
        request.nombre,
        source_type,
        request.geometry,
        request.renspa,
        idempotency_key,
    )
    .await?;
    let status = if created.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(created.into())))
}

#[utoipa::path(post, path = "/territorio/establecimientos/{establecimiento_id}/lotes-base", params(("establecimiento_id" = String, Path)), request_body = CreateGeoJsonLoteBaseRequest, security(("supabaseBearer" = [])), responses((status = 201, body = CreatedGeoJsonLoteBaseResponse), (status = 200, body = CreatedGeoJsonLoteBaseResponse), (status = 400, body = TerritorialValidationErrorResponse), (status = 401), (status = 403), (status = 404), (status = 409), (status = 503)))]
pub(crate) async fn create_geojson_lote_base(
    State(state): State<AppState>,
    Path(establecimiento_id): Path<Uuid>,
    headers: HeaderMap,
    Json(request): Json<CreateGeoJsonLoteBaseRequest>,
) -> Result<(StatusCode, Json<CreatedGeoJsonLoteBaseResponse>), TerritoryRequestError> {
    let context = crate::resolve_request_context(&state, &headers).await?;
    let idempotency_key = geojson_idempotency_key(&headers)?;
    let store = PostgresTerritoryStore::new(state.db.clone());
    let created = application::create_geojson_lote_base(
        &store,
        &context,
        establecimiento_id,
        request.codigo,
        request.nombre,
        request.geometry,
        idempotency_key,
    )
    .await?;
    let status = if created.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(created.into())))
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

impl From<SenasaPolygonPreview> for SenasaPolygonPreviewResponse {
    fn from(value: SenasaPolygonPreview) -> Self {
        Self {
            cantidad_pares_coordenadas_fuente: value.source_coordinate_pair_count,
            geometria: value.geometry.into(),
            tipo_geometria: "MultiPolygon",
            srid: value.srid,
            valida: true,
        }
    }
}

impl From<GeoJsonGeometryPreview> for GeoJsonGeometryPreviewResponse {
    fn from(value: GeoJsonGeometryPreview) -> Self {
        Self {
            geometria: value.geometry.into(),
            tipo_geometria: "MultiPolygon",
            srid: value.srid,
            valida: true,
        }
    }
}

impl From<CreatedAlternativeEstablecimiento> for CreatedAlternativeEstablecimientoResponse {
    fn from(value: CreatedAlternativeEstablecimiento) -> Self {
        Self {
            establecimiento_id: value.establecimiento.id,
            codigo: value.establecimiento.codigo.as_str().to_owned(),
            nombre: value.establecimiento.nombre.as_str().to_owned(),
            origen_geometria: value.establecimiento.origen_geometria.as_str(),
            fuente_geografica: value.source.into(),
            creada: value.created,
        }
    }
}

impl From<CreatedGeoJsonLoteBase> for CreatedGeoJsonLoteBaseResponse {
    fn from(value: CreatedGeoJsonLoteBase) -> Self {
        Self {
            lote_base_id: value.lote_base.id,
            establecimiento_id: value.lote_base.establecimiento_id,
            codigo: value.lote_base.codigo.as_str().to_owned(),
            nombre: value.lote_base.nombre.as_str().to_owned(),
            creado: value.created,
        }
    }
}

impl From<GeographicSourceView> for GeographicSourceResponse {
    fn from(value: GeographicSourceView) -> Self {
        Self {
            id: value.id,
            establecimiento_id: value.establecimiento_id,
            external_reference_id: value.external_reference_id,
            external_id: value.external_id,
            tipo_origen: value.source_type.as_str(),
            nombre_externo: value.external_name,
            texto_fuente_original: value.original_source_text,
            geometria: value.geometry.into(),
            huella_sha256: value.fingerprint.to_hex(),
            version_parser: value.parser_version,
            creado_por: value.created_by,
            creado_en: value.created_at.to_string(),
            confirmada_para_geometria_canonica: value.confirmed_for_canonical_geometry,
            confirmada_por: value.confirmed_by,
            confirmada_en: value.confirmed_at.map(|timestamp| timestamp.to_string()),
        }
    }
}

impl From<application::ConfirmedGeographicSource> for ConfirmedGeographicSourceResponse {
    fn from(value: application::ConfirmedGeographicSource) -> Self {
        Self {
            fuente: value.source.into(),
            creada: value.created,
        }
    }
}

impl From<CanonicalSourceGrouping> for CanonicalSourceGroupingResponse {
    fn from(value: CanonicalSourceGrouping) -> Self {
        Self {
            establecimiento_id: value.establecimiento_id,
            fuente_geografica_ids: value.source_ids,
            geometria_canonica: value.geometry.into(),
            area_fuentes_m2: value.sources_area_m2,
            area_canonica_m2: value.canonical_area_m2,
            area_superpuesta_m2: value.overlap_area_m2,
            superposicion_detectada: value.overlap_detected,
            actualizada: value.updated,
        }
    }
}

impl From<TerritoryValidationError> for TerritorialValidationErrorResponse {
    fn from(value: TerritoryValidationError) -> Self {
        Self {
            codigo: value.code,
            mensaje: value.message,
            detalle: value.detail,
            linea: value.line,
        }
    }
}

fn source_idempotency_key(headers: &HeaderMap) -> Result<IdempotencyKey, TerritoryRequestError> {
    required_idempotency_key(
        headers,
        "Se requiere el encabezado Idempotency-Key para confirmar una fuente.",
    )
}

fn geojson_idempotency_key(headers: &HeaderMap) -> Result<IdempotencyKey, TerritoryRequestError> {
    required_idempotency_key(
        headers,
        "Se requiere el encabezado Idempotency-Key para confirmar la geometría.",
    )
}

fn grouping_idempotency_key(headers: &HeaderMap) -> Result<IdempotencyKey, TerritoryRequestError> {
    required_idempotency_key(
        headers,
        "Se requiere el encabezado Idempotency-Key para confirmar la agrupación.",
    )
}

fn required_idempotency_key(
    headers: &HeaderMap,
    required_message: &'static str,
) -> Result<IdempotencyKey, TerritoryRequestError> {
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            TerritoryRequestError::Application(TerritoryApplicationError::Validation(
                TerritoryValidationError {
                    code: "idempotency_key_requerida",
                    message: required_message.to_owned(),
                    detail: None,
                    line: None,
                },
            ))
        })?;
    IdempotencyKey::new(key.to_owned()).map_err(|_| {
        TerritoryRequestError::Application(TerritoryApplicationError::Validation(
            TerritoryValidationError {
                code: "idempotency_key_invalida",
                message: "El encabezado Idempotency-Key no es válido.".to_owned(),
                detail: None,
                line: None,
            },
        ))
    })
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
