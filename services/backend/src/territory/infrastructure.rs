use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Executor, PgPool, Postgres, Transaction, types::Json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    audit::{self, AuditActor, NewAuditEvent},
    external_references::{
        self, ExternalEntityType, ExternalReferenceStoreError, ExternalSystem, LastSyncStatus,
        NewExternalReference,
    },
    idempotency::{
        self, IdempotencyDecision, IdempotencyError, IdempotencyRequest, OperationCode,
        SafeIdempotencyResult,
    },
    territory::{
        application::{
            BasePlotView, CampaignView, CanonicalSourceGrouping, ConfirmedGeographicSource,
            EstablishmentView, ExternalReferenceView, GeoJsonMultiPolygon, GeographicSourceView,
            NewCanonicalSourceGrouping, NewSenasaGeographicSource, OperationalUnitView,
            TerritorialUseAssignmentView, TerritoryPreviewStore, TerritoryPreviewStoreError,
            TerritoryReadStore, TerritoryReadStoreError, TerritorySourceGroupingStore,
            TerritorySourceGroupingStoreError, TerritorySourceStore, TerritorySourceStoreError,
            TerritoryStore, TerritoryStoreError,
        },
        domain::{
            CanonicalTerritorialCode, Establecimiento, FunctionalName, GeometryProvenance,
            LoteBase, NewEstablecimiento, NewLoteBase, TERRITORIAL_SRID,
        },
        geographic_source::{
            GeographicSourceFingerprint, GeographicSourceType, SENASA_PARSER_VERSION,
        },
        senasa::NormalizedPolygon4326,
    },
};

const RENSPA_SYSTEM: &str = "senasa";
const ESTABLECIMIENTO_ENTITY_TYPE: &str = "establecimiento";

#[derive(Clone)]
pub struct PostgresTerritoryStore {
    db: PgPool,
}

impl PostgresTerritoryStore {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl TerritoryStore for PostgresTerritoryStore {
    async fn create_establecimiento(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        new_establecimiento: NewEstablecimiento,
    ) -> Result<Establecimiento, TerritoryStoreError> {
        let mut transaction = self.db.begin().await.map_err(map_database_error)?;
        let id = Uuid::new_v4();
        let result = insert_establecimiento(
            &mut transaction,
            id,
            organization_id,
            actor_id,
            &new_establecimiento,
        )
        .await;
        match result {
            Ok(establecimiento) => transaction
                .commit()
                .await
                .map(|()| establecimiento)
                .map_err(map_database_error),
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(error)
            }
        }
    }

    async fn create_lote_base(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        new_lote_base: NewLoteBase,
    ) -> Result<LoteBase, TerritoryStoreError> {
        let mut transaction = self.db.begin().await.map_err(map_database_error)?;
        let id = Uuid::new_v4();
        let result = insert_lote_base(
            &mut transaction,
            id,
            organization_id,
            actor_id,
            &new_lote_base,
        )
        .await;
        match result {
            Ok(lote_base) => transaction
                .commit()
                .await
                .map(|()| lote_base)
                .map_err(map_database_error),
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(error)
            }
        }
    }
}

#[async_trait]
impl TerritoryPreviewStore for PostgresTerritoryStore {
    async fn preview_normalized_polygon(
        &self,
        polygon: &NormalizedPolygon4326,
    ) -> Result<GeoJsonMultiPolygon, TerritoryPreviewStoreError> {
        validate_normalized_polygon(&self.db, polygon).await
    }
}

#[async_trait]
impl TerritorySourceStore for PostgresTerritoryStore {
    async fn confirm_senasa_source(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        source: NewSenasaGeographicSource,
    ) -> Result<ConfirmedGeographicSource, TerritorySourceStoreError> {
        let mut transaction = self
            .db
            .begin()
            .await
            .map_err(|_| TerritorySourceStoreError::Unavailable)?;
        let result = confirm_senasa_source_in_transaction(
            &mut transaction,
            organization_id,
            actor_id,
            source,
        )
        .await;
        match result {
            Ok(source) => transaction
                .commit()
                .await
                .map(|()| source)
                .map_err(|_| TerritorySourceStoreError::Unavailable),
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(error)
            }
        }
    }

    async fn list_geographic_sources(
        &self,
        organization_id: Uuid,
        establecimiento_id: Uuid,
    ) -> Result<Vec<GeographicSourceView>, TerritorySourceStoreError> {
        let establishment_exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM public.establecimientos WHERE id = $1 AND organizacion_id = $2",
        )
        .bind(establecimiento_id)
        .bind(organization_id)
        .fetch_optional(&self.db)
        .await
        .map_err(|_| TerritorySourceStoreError::Unavailable)?;
        if establishment_exists.is_none() {
            return Err(TerritorySourceStoreError::NotFound);
        }
        let rows: Vec<GeographicSourceRow> = sqlx::query_as(
            r#"
            SELECT fuente.id, fuente.establecimiento_id, fuente.external_reference_id,
                   referencia.external_id, fuente.tipo_origen, fuente.nombre_externo,
                   fuente.texto_fuente_original, ST_AsGeoJSON(fuente.geometria, 9, 0)::jsonb,
                   fuente.huella_sha256, fuente.version_parser, fuente.creado_por, fuente.creado_en,
                   contribucion.confirmado_por, contribucion.confirmado_en
            FROM public.fuentes_geograficas AS fuente
            JOIN public.external_references AS referencia ON referencia.id = fuente.external_reference_id
            LEFT JOIN public.fuentes_geograficas_contribuciones_canonicas AS contribucion
              ON contribucion.fuente_geografica_id = fuente.id
            WHERE fuente.organizacion_id = $1 AND fuente.establecimiento_id = $2
            ORDER BY fuente.creado_en, fuente.id
            "#,
        )
        .bind(organization_id)
        .bind(establecimiento_id)
        .fetch_all(&self.db)
        .await
        .map_err(|_| TerritorySourceStoreError::Unavailable)?;
        rows.into_iter().map(geographic_source_from_row).collect()
    }
}

#[async_trait]
impl TerritorySourceGroupingStore for PostgresTerritoryStore {
    async fn set_canonical_source_contributors(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        grouping: NewCanonicalSourceGrouping,
    ) -> Result<CanonicalSourceGrouping, TerritorySourceGroupingStoreError> {
        let mut transaction = self
            .db
            .begin()
            .await
            .map_err(|_| TerritorySourceGroupingStoreError::Unavailable)?;
        let result = set_canonical_source_contributors_in_transaction(
            &mut transaction,
            organization_id,
            actor_id,
            grouping,
        )
        .await;
        match result {
            Ok(grouping) => transaction
                .commit()
                .await
                .map(|()| grouping)
                .map_err(|_| TerritorySourceGroupingStoreError::Unavailable),
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(error)
            }
        }
    }
}

async fn set_canonical_source_contributors_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    actor_id: Uuid,
    grouping: NewCanonicalSourceGrouping,
) -> Result<CanonicalSourceGrouping, TerritorySourceGroupingStoreError> {
    let operation = OperationCode::new("territorio.confirmar_fuentes_geograficas_canonicas")
        .expect("territorial grouping operation code must be canonical");
    let decision = idempotency::begin(
        transaction,
        IdempotencyRequest {
            organization_id: Some(organization_id),
            operation,
            key: grouping.idempotency_key,
            request_fingerprint: grouping.idempotency_request_fingerprint,
        },
    )
    .await
    .map_err(map_grouping_idempotency_error)?;

    if let IdempotencyDecision::Replay(replay) = decision {
        let source_ids = replay_source_ids(replay.result.as_value())?;
        validate_grouping_sources(
            transaction,
            organization_id,
            grouping.establecimiento_id,
            &source_ids,
        )
        .await?;
        let mut result = canonical_grouping_diagnostics(
            transaction,
            organization_id,
            grouping.establecimiento_id,
            &source_ids,
        )
        .await?;
        result.updated = replay
            .result
            .as_value()
            .get("actualizada")
            .and_then(Value::as_bool)
            .ok_or(TerritorySourceGroupingStoreError::Unavailable)?;
        return Ok(result);
    }
    let IdempotencyDecision::Proceed(pending) = decision else {
        unreachable!("idempotency decision was handled above")
    };

    let locked_establishment: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM public.establecimientos WHERE id = $1 AND organizacion_id = $2 FOR UPDATE",
    )
    .bind(grouping.establecimiento_id)
    .bind(organization_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_grouping_database_error)?;
    if locked_establishment.is_none() {
        return Err(TerritorySourceGroupingStoreError::NotFound);
    }

    let source_ids = grouping.contributors.source_ids();
    validate_grouping_sources(
        transaction,
        organization_id,
        grouping.establecimiento_id,
        source_ids,
    )
    .await?;
    let current_source_ids: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT fuente_geografica_id
        FROM public.fuentes_geograficas_contribuciones_canonicas
        WHERE organizacion_id = $1 AND establecimiento_id = $2
        ORDER BY fuente_geografica_id
        "#,
    )
    .bind(organization_id)
    .bind(grouping.establecimiento_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_grouping_database_error)?;

    let changed = current_source_ids != source_ids;
    let mut result = canonical_grouping_diagnostics(
        transaction,
        organization_id,
        grouping.establecimiento_id,
        source_ids,
    )
    .await?;

    if changed {
        sqlx::query(
            r#"
            DELETE FROM public.fuentes_geograficas_contribuciones_canonicas
            WHERE organizacion_id = $1 AND establecimiento_id = $2
            "#,
        )
        .bind(organization_id)
        .bind(grouping.establecimiento_id)
        .execute(&mut **transaction)
        .await
        .map_err(map_grouping_database_error)?;
        sqlx::query(
            r#"
            INSERT INTO public.fuentes_geograficas_contribuciones_canonicas (
                organizacion_id, establecimiento_id, fuente_geografica_id, confirmado_por
            )
            SELECT $1, $2, fuente_id, $4
            FROM unnest($3::uuid[]) AS fuente_id
            "#,
        )
        .bind(organization_id)
        .bind(grouping.establecimiento_id)
        .bind(source_ids)
        .bind(actor_id)
        .execute(&mut **transaction)
        .await
        .map_err(map_grouping_database_error)?;

        let updated = sqlx::query(
            r#"
            UPDATE public.establecimientos
            SET geometria = (
                    SELECT ST_Multi(ST_UnaryUnion(ST_Collect(fuente.geometria)))
                    FROM public.fuentes_geograficas AS fuente
                    WHERE fuente.organizacion_id = $1
                      AND fuente.establecimiento_id = $2
                      AND fuente.id = ANY($3::uuid[])
                ),
                origen_geometria = 'senasa_renspa'
            WHERE id = $2 AND organizacion_id = $1
            "#,
        )
        .bind(organization_id)
        .bind(grouping.establecimiento_id)
        .bind(source_ids)
        .execute(&mut **transaction)
        .await
        .map_err(map_grouping_database_error)?;
        if updated.rows_affected() != 1 {
            return Err(TerritorySourceGroupingStoreError::NotFound);
        }

        audit::record(
            transaction,
            &NewAuditEvent {
                organization_id,
                actor: AuditActor::Usuario(actor_id),
                action: "establecimiento.fuentes_geograficas_confirmadas",
                entity_type: ESTABLECIMIENTO_ENTITY_TYPE,
                entity_id: Some(grouping.establecimiento_id),
                reference: None,
                before_state: Some(json!({
                    "fuente_geografica_ids": current_source_ids,
                })),
                after_state: Some(json!({
                    "fuente_geografica_ids": source_ids,
                    "superposicion_detectada": result.overlap_detected,
                    "area_fuentes_m2": result.sources_area_m2,
                    "area_canonica_m2": result.canonical_area_m2,
                    "area_superpuesta_m2": result.overlap_area_m2,
                })),
            },
        )
        .await
        .map_err(|_| TerritorySourceGroupingStoreError::Unavailable)?;
    }

    result.updated = changed;
    let replay_result = SafeIdempotencyResult::new(json!({
        "fuente_geografica_ids": source_ids,
        "actualizada": changed,
    }))
    .expect("bounded territorial grouping replay result must fit");
    idempotency::complete(transaction, pending, replay_result)
        .await
        .map_err(map_grouping_idempotency_error)?;

    Ok(result)
}

async fn validate_grouping_sources(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    establecimiento_id: Uuid,
    source_ids: &[Uuid],
) -> Result<(), TerritorySourceGroupingStoreError> {
    let rows: Vec<(Uuid, String, i32, bool, bool)> = sqlx::query_as(
        r#"
        SELECT id, GeometryType(geometria), ST_SRID(geometria),
               ST_IsValid(geometria), ST_IsEmpty(geometria)
        FROM public.fuentes_geograficas
        WHERE organizacion_id = $1
          AND establecimiento_id = $2
          AND id = ANY($3::uuid[])
        ORDER BY id
        "#,
    )
    .bind(organization_id)
    .bind(establecimiento_id)
    .bind(source_ids)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_grouping_database_error)?;
    let persisted_ids = rows.iter().map(|row| row.0).collect::<Vec<_>>();
    if persisted_ids != source_ids {
        return Err(TerritorySourceGroupingStoreError::NotFound);
    }
    if rows
        .iter()
        .any(|row| row.1 != "MULTIPOLYGON" || row.2 != TERRITORIAL_SRID || !row.3 || row.4)
    {
        return Err(TerritorySourceGroupingStoreError::Conflict);
    }
    Ok(())
}

async fn canonical_grouping_diagnostics(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    establecimiento_id: Uuid,
    source_ids: &[Uuid],
) -> Result<CanonicalSourceGrouping, TerritorySourceGroupingStoreError> {
    let row: (Json<Value>, String, i32, bool, bool, f64, f64, f64, bool) = sqlx::query_as(
        r#"
        WITH seleccionadas AS MATERIALIZED (
            SELECT id, geometria
            FROM public.fuentes_geograficas
            WHERE organizacion_id = $1
              AND establecimiento_id = $2
              AND id = ANY($3::uuid[])
        ), canonica AS (
            SELECT ST_Multi(ST_UnaryUnion(ST_Collect(geometria))) AS geometria,
                   SUM(ST_Area(geometria::geography))::float8 AS area_fuentes_m2
            FROM seleccionadas
        ), diagnostico AS (
            SELECT EXISTS (
                SELECT 1
                FROM seleccionadas AS izquierda
                JOIN seleccionadas AS derecha ON izquierda.id < derecha.id
                WHERE ST_Intersects(izquierda.geometria, derecha.geometria)
                  AND ST_Area(
                        ST_Intersection(izquierda.geometria, derecha.geometria)::geography
                      ) > 0
            ) AS superposicion_detectada
        )
        SELECT ST_AsGeoJSON(canonica.geometria, 9, 0)::jsonb,
               GeometryType(canonica.geometria), ST_SRID(canonica.geometria),
               ST_IsEmpty(canonica.geometria), ST_IsValid(canonica.geometria),
               canonica.area_fuentes_m2,
               ST_Area(canonica.geometria::geography)::float8 AS area_canonica_m2,
               GREATEST(
                   canonica.area_fuentes_m2 - ST_Area(canonica.geometria::geography),
                   0::float8
               )::float8 AS area_superpuesta_m2,
               diagnostico.superposicion_detectada
        FROM canonica CROSS JOIN diagnostico
        "#,
    )
    .bind(organization_id)
    .bind(establecimiento_id)
    .bind(source_ids)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_grouping_database_error)?;
    let (
        Json(geometry),
        geometry_type,
        srid,
        is_empty,
        is_valid,
        sources_area_m2,
        canonical_area_m2,
        overlap_area_m2,
        overlap_detected,
    ) = row;
    if geometry_type != "MULTIPOLYGON" || srid != TERRITORIAL_SRID || is_empty || !is_valid {
        return Err(TerritorySourceGroupingStoreError::Conflict);
    }
    Ok(CanonicalSourceGrouping {
        establecimiento_id,
        source_ids: source_ids.to_vec(),
        geometry: multipolygon_from_geojson(geometry)
            .map_err(|_| TerritorySourceGroupingStoreError::Unavailable)?,
        sources_area_m2,
        canonical_area_m2,
        overlap_area_m2,
        overlap_detected,
        updated: false,
    })
}

fn replay_source_ids(value: &Value) -> Result<Vec<Uuid>, TerritorySourceGroupingStoreError> {
    value
        .get("fuente_geografica_ids")
        .cloned()
        .and_then(|ids| serde_json::from_value(ids).ok())
        .ok_or(TerritorySourceGroupingStoreError::Unavailable)
}

fn map_grouping_idempotency_error(error: IdempotencyError) -> TerritorySourceGroupingStoreError {
    match error {
        IdempotencyError::Conflict(_) => TerritorySourceGroupingStoreError::Conflict,
        IdempotencyError::InvariantViolation | IdempotencyError::Database(_) => {
            TerritorySourceGroupingStoreError::Unavailable
        }
    }
}

fn map_grouping_database_error(error: sqlx::Error) -> TerritorySourceGroupingStoreError {
    match &error {
        sqlx::Error::Database(database_error)
            if matches!(
                database_error.code().as_deref(),
                Some("23505" | "23503" | "P0001")
            ) =>
        {
            TerritorySourceGroupingStoreError::Conflict
        }
        _ => TerritorySourceGroupingStoreError::Unavailable,
    }
}

type GeographicSourceRow = (
    Uuid,
    Uuid,
    Uuid,
    String,
    String,
    Option<String>,
    String,
    Json<Value>,
    Vec<u8>,
    String,
    Uuid,
    OffsetDateTime,
    Option<Uuid>,
    Option<OffsetDateTime>,
);

async fn validate_normalized_polygon<'e, E>(
    executor: E,
    polygon: &NormalizedPolygon4326,
) -> Result<GeoJsonMultiPolygon, TerritoryPreviewStoreError>
where
    E: Executor<'e, Database = Postgres>,
{
    let row: (i32, bool, bool, Option<String>, Json<Value>) = sqlx::query_as(
        r#"
        WITH normalized AS (
            SELECT ST_Multi(ST_SetSRID(ST_GeomFromGeoJSON($1::jsonb), $2)) AS geometria
        )
        SELECT ST_SRID(geometria),
               ST_IsEmpty(geometria),
               ST_IsValid(geometria),
               CASE WHEN ST_IsValid(geometria) THEN NULL ELSE ST_IsValidReason(geometria) END,
               ST_AsGeoJSON(geometria, 9, 0)::jsonb
        FROM normalized
        "#,
    )
    .bind(Json(polygon.as_geojson_polygon()))
    .bind(TERRITORIAL_SRID)
    .fetch_one(executor)
    .await
    .map_err(|_| TerritoryPreviewStoreError::Unavailable)?;

    let (srid, is_empty, is_valid, validity_reason, Json(geometry)) = row;
    if srid != TERRITORIAL_SRID {
        return Err(TerritoryPreviewStoreError::UnexpectedSrid);
    }
    if is_empty {
        return Err(TerritoryPreviewStoreError::EmptyGeometry);
    }
    if !is_valid {
        return Err(TerritoryPreviewStoreError::InvalidTopology {
            reason: validity_reason
                .unwrap_or_else(|| "PostGIS no pudo validar la geometría.".to_owned()),
        });
    }
    multipolygon_from_geojson(geometry).map_err(|_| TerritoryPreviewStoreError::Unavailable)
}

async fn confirm_senasa_source_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    actor_id: Uuid,
    source: NewSenasaGeographicSource,
) -> Result<ConfirmedGeographicSource, TerritorySourceStoreError> {
    let operation = OperationCode::new("territorio.confirmar_fuente_geografica_senasa")
        .expect("territorial source operation code must be canonical");
    let decision = idempotency::begin(
        transaction,
        IdempotencyRequest {
            organization_id: Some(organization_id),
            operation,
            key: source.idempotency_key.clone(),
            request_fingerprint: source.idempotency_request_fingerprint,
        },
    )
    .await
    .map_err(map_idempotency_error)?;
    if let IdempotencyDecision::Replay(replay) = decision {
        let source_id = replay
            .result
            .as_value()
            .get("fuente_geografica_id")
            .and_then(Value::as_str)
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or(TerritorySourceStoreError::Unavailable)?;
        return load_geographic_source_by_id(transaction, organization_id, source_id)
            .await
            .map(|source| ConfirmedGeographicSource {
                source,
                created: false,
            });
    }
    let IdempotencyDecision::Proceed(pending) = decision else {
        unreachable!("idempotency decision was handled above")
    };

    let establishment_exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM public.establecimientos WHERE id = $1 AND organizacion_id = $2",
    )
    .bind(source.establecimiento_id)
    .bind(organization_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| TerritorySourceStoreError::Unavailable)?;
    if establishment_exists.is_none() {
        return Err(TerritorySourceStoreError::NotFound);
    }

    // This is the same 3.6a PostGIS validation path, now run in the mutation
    // transaction before source evidence can be inserted.
    validate_normalized_polygon(&mut **transaction, &source.normalized_polygon)
        .await
        .map_err(TerritorySourceStoreError::Validation)?;

    let reference = external_references::register(
        transaction,
        NewExternalReference {
            organization_id: Some(organization_id),
            system: ExternalSystem::new(RENSPA_SYSTEM)
                .expect("SENASA system code must be canonical"),
            external_id: source.external_id.clone(),
            entity_type: ExternalEntityType::new(ESTABLECIMIENTO_ENTITY_TYPE)
                .expect("establishment entity type must be canonical"),
            entity_id: source.establecimiento_id,
            sync_version: None,
            last_sync_status: LastSyncStatus::Sincronizado,
            last_synced_at: Some(OffsetDateTime::now_utc()),
        },
    )
    .await
    .map_err(map_external_reference_source_error)?;

    let inserted: Option<GeographicSourceRow> = sqlx::query_as(
        r#"
        INSERT INTO public.fuentes_geograficas (
            organizacion_id, establecimiento_id, external_reference_id, tipo_origen,
            nombre_externo, texto_fuente_original, geometria, huella_sha256,
            version_parser, creado_por
        )
        VALUES (
            $1, $2, $3, $4, $5, $6,
            ST_Multi(ST_SetSRID(ST_GeomFromGeoJSON($7::jsonb), $8)),
            $9, $10, $11
        )
        ON CONFLICT (organizacion_id, huella_sha256) DO NOTHING
        RETURNING id, establecimiento_id, external_reference_id,
                  $12::text AS external_id, tipo_origen, nombre_externo,
                  texto_fuente_original, ST_AsGeoJSON(geometria, 9, 0)::jsonb,
                  huella_sha256, version_parser, creado_por, creado_en,
                  NULL::uuid AS confirmado_por, NULL::timestamptz AS confirmado_en
        "#,
    )
    .bind(organization_id)
    .bind(source.establecimiento_id)
    .bind(reference.reference.id)
    .bind(GeographicSourceType::SenasaRenspa.as_str())
    .bind(source.external_name.as_ref().map(|name| name.as_str()))
    .bind(&source.original_source_text)
    .bind(Json(source.normalized_polygon.as_geojson_polygon()))
    .bind(TERRITORIAL_SRID)
    .bind(source.fingerprint.as_bytes().as_slice())
    .bind(SENASA_PARSER_VERSION)
    .bind(actor_id)
    .bind(source.external_id.as_str())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| TerritorySourceStoreError::Unavailable)?;

    let (source, created) = match inserted {
        Some(row) => {
            let source = geographic_source_from_row(row)?;
            audit::record(
                transaction,
                &NewAuditEvent {
                    organization_id,
                    actor: AuditActor::Usuario(actor_id),
                    action: "fuente_geografica.senasa_confirmada",
                    entity_type: "fuente_geografica",
                    entity_id: Some(source.id),
                    reference: Some(source.external_id.as_str()),
                    before_state: None,
                    after_state: Some(json!({
                        "fuente_geografica_id": source.id,
                        "establecimiento_id": source.establecimiento_id,
                        "external_reference_id": source.external_reference_id,
                        "tipo_origen": source.source_type.as_str(),
                        "huella_sha256": source.fingerprint.to_hex(),
                        "version_parser": source.parser_version,
                    })),
                },
            )
            .await
            .map_err(|_| TerritorySourceStoreError::Unavailable)?;
            (source, true)
        }
        None => (
            load_geographic_source_by_fingerprint(transaction, organization_id, source.fingerprint)
                .await?,
            false,
        ),
    };

    let replay_result = SafeIdempotencyResult::new(json!({
        "fuente_geografica_id": source.id.to_string(),
    }))
    .expect("territorial source idempotency result must be small");
    idempotency::complete(transaction, pending, replay_result)
        .await
        .map_err(map_idempotency_error)?;

    Ok(ConfirmedGeographicSource { source, created })
}

async fn load_geographic_source_by_id(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    source_id: Uuid,
) -> Result<GeographicSourceView, TerritorySourceStoreError> {
    let row: Option<GeographicSourceRow> = sqlx::query_as(
        r#"
        SELECT fuente.id, fuente.establecimiento_id, fuente.external_reference_id,
               referencia.external_id, fuente.tipo_origen, fuente.nombre_externo,
               fuente.texto_fuente_original, ST_AsGeoJSON(fuente.geometria, 9, 0)::jsonb,
               fuente.huella_sha256, fuente.version_parser, fuente.creado_por, fuente.creado_en,
               contribucion.confirmado_por, contribucion.confirmado_en
        FROM public.fuentes_geograficas AS fuente
        JOIN public.external_references AS referencia ON referencia.id = fuente.external_reference_id
        LEFT JOIN public.fuentes_geograficas_contribuciones_canonicas AS contribucion
          ON contribucion.fuente_geografica_id = fuente.id
        WHERE fuente.organizacion_id = $1 AND fuente.id = $2
        "#,
    )
    .bind(organization_id)
    .bind(source_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| TerritorySourceStoreError::Unavailable)?;
    row.ok_or(TerritorySourceStoreError::Unavailable)
        .and_then(geographic_source_from_row)
}

async fn load_geographic_source_by_fingerprint(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    fingerprint: GeographicSourceFingerprint,
) -> Result<GeographicSourceView, TerritorySourceStoreError> {
    let row: Option<GeographicSourceRow> = sqlx::query_as(
        r#"
        SELECT fuente.id, fuente.establecimiento_id, fuente.external_reference_id,
               referencia.external_id, fuente.tipo_origen, fuente.nombre_externo,
               fuente.texto_fuente_original, ST_AsGeoJSON(fuente.geometria, 9, 0)::jsonb,
               fuente.huella_sha256, fuente.version_parser, fuente.creado_por, fuente.creado_en,
               contribucion.confirmado_por, contribucion.confirmado_en
        FROM public.fuentes_geograficas AS fuente
        JOIN public.external_references AS referencia ON referencia.id = fuente.external_reference_id
        LEFT JOIN public.fuentes_geograficas_contribuciones_canonicas AS contribucion
          ON contribucion.fuente_geografica_id = fuente.id
        WHERE fuente.organizacion_id = $1 AND fuente.huella_sha256 = $2
        "#,
    )
    .bind(organization_id)
    .bind(fingerprint.as_bytes().as_slice())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| TerritorySourceStoreError::Unavailable)?;
    row.ok_or(TerritorySourceStoreError::Unavailable)
        .and_then(geographic_source_from_row)
}

fn geographic_source_from_row(
    (
        id,
        establecimiento_id,
        external_reference_id,
        external_id,
        source_type,
        external_name,
        original_source_text,
        Json(geometry),
        fingerprint,
        parser_version,
        created_by,
        created_at,
        confirmed_by,
        confirmed_at,
    ): GeographicSourceRow,
) -> Result<GeographicSourceView, TerritorySourceStoreError> {
    let fingerprint: [u8; 32] = fingerprint
        .try_into()
        .map_err(|_| TerritorySourceStoreError::Unavailable)?;
    if source_type != GeographicSourceType::SenasaRenspa.as_str()
        || parser_version != SENASA_PARSER_VERSION
    {
        return Err(TerritorySourceStoreError::Unavailable);
    }
    Ok(GeographicSourceView {
        id,
        establecimiento_id,
        external_reference_id,
        external_id,
        source_type: GeographicSourceType::SenasaRenspa,
        external_name,
        original_source_text,
        geometry: multipolygon_from_geojson(geometry)
            .map_err(|_| TerritorySourceStoreError::Unavailable)?,
        fingerprint: GeographicSourceFingerprint::from_bytes(fingerprint),
        parser_version,
        created_by,
        created_at,
        confirmed_for_canonical_geometry: confirmed_by.is_some(),
        confirmed_by,
        confirmed_at,
    })
}

fn map_idempotency_error(error: IdempotencyError) -> TerritorySourceStoreError {
    match error {
        IdempotencyError::Conflict(_) => TerritorySourceStoreError::Conflict,
        IdempotencyError::InvariantViolation | IdempotencyError::Database(_) => {
            TerritorySourceStoreError::Unavailable
        }
    }
}

fn map_external_reference_source_error(
    error: ExternalReferenceStoreError,
) -> TerritorySourceStoreError {
    match error {
        ExternalReferenceStoreError::IdentityConflict => TerritorySourceStoreError::Conflict,
        ExternalReferenceStoreError::NotFound
        | ExternalReferenceStoreError::InvariantViolation
        | ExternalReferenceStoreError::Database(_) => TerritorySourceStoreError::Unavailable,
    }
}

#[async_trait]
impl TerritoryReadStore for PostgresTerritoryStore {
    async fn list_establecimientos(
        &self,
        organization_id: Uuid,
    ) -> Result<Vec<EstablishmentView>, TerritoryReadStoreError> {
        let ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM public.establecimientos WHERE organizacion_id = $1 ORDER BY codigo, id",
        )
        .bind(organization_id)
        .fetch_all(&self.db)
        .await
        .map_err(map_read_database_error)?;

        let mut establecimientos = Vec::with_capacity(ids.len());
        for id in ids {
            establecimientos.push(self.establecimiento(organization_id, id).await?);
        }
        Ok(establecimientos)
    }

    async fn get_establecimiento(
        &self,
        organization_id: Uuid,
        establecimiento_id: Uuid,
    ) -> Result<EstablishmentView, TerritoryReadStoreError> {
        self.establecimiento(organization_id, establecimiento_id)
            .await
    }

    async fn list_campanas(
        &self,
        organization_id: Uuid,
    ) -> Result<Vec<CampaignView>, TerritoryReadStoreError> {
        let rows: Vec<(Uuid, String, String, time::Date, time::Date, bool)> = sqlx::query_as(
            "SELECT id, codigo, nombre, fecha_inicio, fecha_fin, activa FROM public.campanas WHERE organizacion_id = $1 ORDER BY fecha_inicio DESC, codigo, id",
        )
        .bind(organization_id)
        .fetch_all(&self.db)
        .await
        .map_err(map_read_database_error)?;
        Ok(rows.into_iter().map(campaign_from_row).collect())
    }

    async fn get_campana(
        &self,
        organization_id: Uuid,
        campana_id: Uuid,
    ) -> Result<CampaignView, TerritoryReadStoreError> {
        let row: Option<(Uuid, String, String, time::Date, time::Date, bool)> = sqlx::query_as(
            "SELECT id, codigo, nombre, fecha_inicio, fecha_fin, activa FROM public.campanas WHERE id = $1 AND organizacion_id = $2",
        )
        .bind(campana_id)
        .bind(organization_id)
        .fetch_optional(&self.db)
        .await
        .map_err(map_read_database_error)?;
        row.map(campaign_from_row)
            .ok_or(TerritoryReadStoreError::NotFound)
    }

    async fn list_unidades_operativas(
        &self,
        organization_id: Uuid,
        campana_id: Uuid,
        establecimiento_id: Uuid,
    ) -> Result<Vec<OperationalUnitView>, TerritoryReadStoreError> {
        let context_exists: Option<bool> = sqlx::query_scalar(
            r#"
            SELECT EXISTS (SELECT 1 FROM public.campanas WHERE id = $1 AND organizacion_id = $3)
               AND EXISTS (SELECT 1 FROM public.establecimientos WHERE id = $2 AND organizacion_id = $3)
            "#,
        )
        .bind(campana_id)
        .bind(establecimiento_id)
        .bind(organization_id)
        .fetch_optional(&self.db)
        .await
        .map_err(map_read_database_error)?;
        if context_exists != Some(true) {
            return Err(TerritoryReadStoreError::NotFound);
        }

        let rows: Vec<(Uuid, Uuid, Uuid, String, String, bool, Json<Value>)> = sqlx::query_as(
            r#"
            SELECT id, campana_id, establecimiento_id, codigo, nombre, activa,
                   ST_AsGeoJSON(geometria, 9, 0)::jsonb
            FROM public.unidades_operativas
            WHERE organizacion_id = $1 AND campana_id = $2 AND establecimiento_id = $3
            ORDER BY codigo, id
            "#,
        )
        .bind(organization_id)
        .bind(campana_id)
        .bind(establecimiento_id)
        .fetch_all(&self.db)
        .await
        .map_err(map_read_database_error)?;

        let mut units = Vec::with_capacity(rows.len());
        for (id, campana_id, establecimiento_id, codigo, nombre, activa, Json(geometria)) in rows {
            let lote_base_ids: Vec<Uuid> = sqlx::query_scalar(
                r#"
                SELECT vinculo.lote_base_id
                FROM public.unidades_operativas_lotes_base AS vinculo
                JOIN public.lotes_base AS lote_base ON lote_base.id = vinculo.lote_base_id
                WHERE vinculo.unidad_operativa_id = $1
                ORDER BY lote_base.codigo, lote_base.id
                "#,
            )
            .bind(id)
            .fetch_all(&self.db)
            .await
            .map_err(map_read_database_error)?;
            units.push(OperationalUnitView {
                id,
                campana_id,
                establecimiento_id,
                codigo,
                nombre,
                activa,
                geometria: multipolygon_from_geojson(geometria)?,
                lote_base_ids,
            });
        }
        Ok(units)
    }

    async fn list_usos_unidad_operativa(
        &self,
        organization_id: Uuid,
        unidad_operativa_id: Uuid,
    ) -> Result<Vec<TerritorialUseAssignmentView>, TerritoryReadStoreError> {
        let exists: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM public.unidades_operativas WHERE id = $1 AND organizacion_id = $2",
        )
        .bind(unidad_operativa_id)
        .bind(organization_id)
        .fetch_optional(&self.db)
        .await
        .map_err(map_read_database_error)?;
        if exists.is_none() {
            return Err(TerritoryReadStoreError::NotFound);
        }
        let rows: Vec<(Uuid, Uuid, String, String, time::Date, time::Date)> = sqlx::query_as(
            r#"
            SELECT asignacion.id, uso.id, uso.codigo, uso.nombre,
                   asignacion.fecha_inicio, asignacion.fecha_fin
            FROM public.unidades_operativas_usos AS asignacion
            JOIN public.usos_territoriales AS uso ON uso.id = asignacion.uso_territorial_id
            WHERE asignacion.unidad_operativa_id = $1
            ORDER BY asignacion.fecha_inicio, asignacion.fecha_fin, asignacion.id
            "#,
        )
        .bind(unidad_operativa_id)
        .fetch_all(&self.db)
        .await
        .map_err(map_read_database_error)?;
        Ok(rows
            .into_iter()
            .map(
                |(id, uso_territorial_id, uso_codigo, uso_nombre, fecha_inicio, fecha_fin)| {
                    TerritorialUseAssignmentView {
                        id,
                        uso_territorial_id,
                        uso_codigo,
                        uso_nombre,
                        fecha_inicio,
                        fecha_fin,
                    }
                },
            )
            .collect())
    }
}

impl PostgresTerritoryStore {
    async fn establecimiento(
        &self,
        organization_id: Uuid,
        establecimiento_id: Uuid,
    ) -> Result<EstablishmentView, TerritoryReadStoreError> {
        let row: Option<(Uuid, String, String, bool, Json<Value>)> = sqlx::query_as(
            r#"
            SELECT id, codigo, nombre, activo, ST_AsGeoJSON(geometria, 9, 0)::jsonb
            FROM public.establecimientos
            WHERE id = $1 AND organizacion_id = $2
            "#,
        )
        .bind(establecimiento_id)
        .bind(organization_id)
        .fetch_optional(&self.db)
        .await
        .map_err(map_read_database_error)?;
        let (id, codigo, nombre, activo, Json(geometria)) =
            row.ok_or(TerritoryReadStoreError::NotFound)?;

        let base_rows: Vec<(Uuid, String, String, bool, Json<Value>)> = sqlx::query_as(
            r#"
            SELECT id, codigo, nombre, activo, ST_AsGeoJSON(geometria, 9, 0)::jsonb
            FROM public.lotes_base
            WHERE organizacion_id = $1 AND establecimiento_id = $2
            ORDER BY codigo, id
            "#,
        )
        .bind(organization_id)
        .bind(id)
        .fetch_all(&self.db)
        .await
        .map_err(map_read_database_error)?;
        let lotes_base = base_rows
            .into_iter()
            .map(|(id, codigo, nombre, activo, Json(geometria))| {
                Ok(BasePlotView {
                    id,
                    codigo,
                    nombre,
                    activo,
                    geometria: multipolygon_from_geojson(geometria)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let referencias_externas: Vec<(String, String)> = sqlx::query_as(
            r#"
            SELECT sistema_externo, external_id
            FROM public.external_references
            WHERE organizacion_id = $1 AND entidad_tipo = 'establecimiento' AND entidad_id = $2
            ORDER BY creado_en, id
            "#,
        )
        .bind(organization_id)
        .bind(id)
        .fetch_all(&self.db)
        .await
        .map_err(map_read_database_error)?;

        Ok(EstablishmentView {
            id,
            codigo,
            nombre,
            activo,
            geometria: multipolygon_from_geojson(geometria)?,
            referencias_externas: referencias_externas
                .into_iter()
                .map(|(sistema_externo, external_id)| ExternalReferenceView {
                    sistema_externo,
                    external_id,
                })
                .collect(),
            lotes_base,
        })
    }
}

fn campaign_from_row(
    (id, codigo, nombre, fecha_inicio, fecha_fin, activa): (
        Uuid,
        String,
        String,
        time::Date,
        time::Date,
        bool,
    ),
) -> CampaignView {
    CampaignView {
        id,
        codigo,
        nombre,
        fecha_inicio,
        fecha_fin,
        activa,
    }
}

#[derive(Deserialize)]
struct RawGeoJsonGeometry {
    #[serde(rename = "type")]
    geometry_type: String,
    coordinates: Vec<Vec<Vec<Vec<f64>>>>,
}

fn multipolygon_from_geojson(value: Value) -> Result<GeoJsonMultiPolygon, TerritoryReadStoreError> {
    let raw: RawGeoJsonGeometry =
        serde_json::from_value(value).map_err(|_| TerritoryReadStoreError::Unavailable)?;
    if raw.geometry_type != "MultiPolygon" {
        return Err(TerritoryReadStoreError::Unavailable);
    }
    Ok(GeoJsonMultiPolygon {
        coordinates: raw.coordinates,
    })
}

fn map_read_database_error(_: sqlx::Error) -> TerritoryReadStoreError {
    TerritoryReadStoreError::Unavailable
}

async fn insert_establecimiento(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    organization_id: Uuid,
    actor_id: Uuid,
    new_establecimiento: &NewEstablecimiento,
) -> Result<Establecimiento, TerritoryStoreError> {
    let row: (
        Uuid,
        Uuid,
        String,
        String,
        String,
        String,
        bool,
        Uuid,
        OffsetDateTime,
    ) = sqlx::query_as(
        r#"
        INSERT INTO public.establecimientos (
            id, organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por
        )
        VALUES ($1, $2, $3, $4, ST_Multi(ST_GeomFromText($5, $6)), $7, $8)
        RETURNING id, organizacion_id, codigo, nombre, ST_AsEWKT(geometria), origen_geometria,
                  activo, creado_por, creado_en
        "#,
    )
    .bind(id)
    .bind(organization_id)
    .bind(new_establecimiento.codigo.as_str())
    .bind(new_establecimiento.nombre.as_str())
    .bind(new_establecimiento.geometria_wkt.as_str())
    .bind(TERRITORIAL_SRID)
    .bind(new_establecimiento.origen_geometria.as_str())
    .bind(actor_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_database_error)?;
    let establecimiento =
        establecimiento_from_row(row).map_err(|_| TerritoryStoreError::Unavailable)?;
    external_references::register(
        transaction,
        NewExternalReference {
            organization_id: Some(organization_id),
            system: ExternalSystem::new(RENSPA_SYSTEM).expect("RENSPA system code must be valid"),
            external_id: new_establecimiento.renspa.clone(),
            entity_type: ExternalEntityType::new(ESTABLECIMIENTO_ENTITY_TYPE)
                .expect("establishment entity type must be valid"),
            entity_id: id,
            sync_version: None,
            last_sync_status: LastSyncStatus::Pendiente,
            last_synced_at: None,
        },
    )
    .await
    .map_err(map_external_reference_error)?;
    audit::record(
        transaction,
        &NewAuditEvent {
            organization_id,
            actor: AuditActor::Usuario(actor_id),
            action: "establecimiento.creado",
            entity_type: "establecimiento",
            entity_id: Some(id),
            reference: Some(new_establecimiento.codigo.as_str()),
            before_state: None,
            after_state: Some(json!({
                "establecimiento_id": id,
                "codigo": new_establecimiento.codigo.as_str(),
                "nombre": new_establecimiento.nombre.as_str(),
                "origen_geometria": new_establecimiento.origen_geometria.as_str(),
                "renspa": new_establecimiento.renspa.as_str(),
                "activo": true,
            })),
        },
    )
    .await
    .map_err(map_database_error)?;
    Ok(establecimiento)
}

async fn insert_lote_base(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    organization_id: Uuid,
    actor_id: Uuid,
    new_lote_base: &NewLoteBase,
) -> Result<LoteBase, TerritoryStoreError> {
    let row: (
        Uuid,
        Uuid,
        Uuid,
        String,
        String,
        String,
        bool,
        Uuid,
        OffsetDateTime,
    ) = sqlx::query_as(
        r#"
        INSERT INTO public.lotes_base (
            id, organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por
        )
        VALUES ($1, $2, $3, $4, $5, ST_Multi(ST_GeomFromText($6, $7)), $8)
        RETURNING id, organizacion_id, establecimiento_id, codigo, nombre,
                  ST_AsEWKT(geometria), activo, creado_por, creado_en
        "#,
    )
    .bind(id)
    .bind(organization_id)
    .bind(new_lote_base.establecimiento_id)
    .bind(new_lote_base.codigo.as_str())
    .bind(new_lote_base.nombre.as_str())
    .bind(new_lote_base.geometria_wkt.as_str())
    .bind(TERRITORIAL_SRID)
    .bind(actor_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_database_error)?;
    let lote_base = lote_base_from_row(row).map_err(|_| TerritoryStoreError::Unavailable)?;
    audit::record(
        transaction,
        &NewAuditEvent {
            organization_id,
            actor: AuditActor::Usuario(actor_id),
            action: "lote_base.creado",
            entity_type: "lote_base",
            entity_id: Some(id),
            reference: Some(new_lote_base.codigo.as_str()),
            before_state: None,
            after_state: Some(json!({
                "lote_base_id": id,
                "establecimiento_id": new_lote_base.establecimiento_id,
                "codigo": new_lote_base.codigo.as_str(),
                "nombre": new_lote_base.nombre.as_str(),
                "srid": TERRITORIAL_SRID,
                "geometria_ewkt": lote_base.geometria_ewkt,
                "activo": true,
            })),
        },
    )
    .await
    .map_err(map_database_error)?;
    Ok(lote_base)
}

fn establecimiento_from_row(
    (
        id,
        organization_id,
        codigo,
        nombre,
        geometria_ewkt,
        origen_geometria,
        activo,
        creado_por,
        creado_en,
    ): (
        Uuid,
        Uuid,
        String,
        String,
        String,
        String,
        bool,
        Uuid,
        OffsetDateTime,
    ),
) -> Result<Establecimiento, ()> {
    Ok(Establecimiento {
        id,
        organization_id,
        codigo: CanonicalTerritorialCode::new(codigo).map_err(|_| ())?,
        nombre: FunctionalName::new(nombre).map_err(|_| ())?,
        geometria_ewkt,
        origen_geometria: GeometryProvenance::try_from(origen_geometria.as_str())
            .map_err(|_| ())?,
        activo,
        creado_por,
        creado_en,
    })
}

fn map_external_reference_error(error: ExternalReferenceStoreError) -> TerritoryStoreError {
    match error {
        ExternalReferenceStoreError::IdentityConflict => TerritoryStoreError::Conflict,
        ExternalReferenceStoreError::NotFound
        | ExternalReferenceStoreError::InvariantViolation
        | ExternalReferenceStoreError::Database(_) => TerritoryStoreError::Unavailable,
    }
}

fn lote_base_from_row(
    (
        id,
        organization_id,
        establecimiento_id,
        codigo,
        nombre,
        geometria_ewkt,
        activo,
        creado_por,
        creado_en,
    ): (
        Uuid,
        Uuid,
        Uuid,
        String,
        String,
        String,
        bool,
        Uuid,
        OffsetDateTime,
    ),
) -> Result<LoteBase, ()> {
    Ok(LoteBase {
        id,
        organization_id,
        establecimiento_id,
        codigo: CanonicalTerritorialCode::new(codigo).map_err(|_| ())?,
        nombre: FunctionalName::new(nombre).map_err(|_| ())?,
        geometria_ewkt,
        activo,
        creado_por,
        creado_en,
    })
}

fn map_database_error(error: sqlx::Error) -> TerritoryStoreError {
    match &error {
        sqlx::Error::Database(database_error)
            if database_error.code().as_deref() == Some("23505") =>
        {
            TerritoryStoreError::Conflict
        }
        _ => TerritoryStoreError::Unavailable,
    }
}
