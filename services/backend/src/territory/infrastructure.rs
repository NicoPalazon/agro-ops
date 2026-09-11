use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Executor, PgConnection, PgPool, Postgres, Transaction, types::Json};
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
            BasePlotView, CampaignView, CanonicalGeometryImpact, CanonicalSourceGrouping,
            ConfirmedGeographicSource, CreatedAlternativeEstablecimiento, CreatedGeoJsonLoteBase,
            EstablishmentView, ExternalReferenceView, GeoJsonMultiPolygon,
            GeographicSourceCorrectionCommand, GeographicSourceCorrectionPreview,
            GeographicSourceCorrectionResult, GeographicSourceView, NewAlternativeEstablecimiento,
            NewCanonicalSourceGrouping, NewGeoJsonLoteBase, NewSenasaGeographicSource,
            OperationalUnitView, TerritorialUseAssignmentView, TerritoryCorrectionStore,
            TerritoryCorrectionStoreError, TerritoryGeoJsonStore, TerritoryPreviewStore,
            TerritoryPreviewStoreError, TerritoryReadStore, TerritoryReadStoreError,
            TerritorySourceGroupingStore, TerritorySourceGroupingStoreError, TerritorySourceStore,
            TerritorySourceStoreError, TerritoryStore, TerritoryStoreError,
        },
        domain::{
            CanonicalTerritorialCode, Establecimiento, FunctionalName, GeometryProvenance,
            LoteBase, NewEstablecimiento, NewLoteBase, TERRITORIAL_SRID,
        },
        geographic_source::{
            CORRECTION_PARSER_VERSION, GeographicSourceFingerprint, GeographicSourceType,
            SENASA_PARSER_VERSION,
        },
        geojson::{GEOJSON_INPUT_VERSION, NormalizedGeoJsonMultiPolygon},
    },
};

const RENSPA_SYSTEM: &str = "senasa";
const ESTABLECIMIENTO_ENTITY_TYPE: &str = "establecimiento";

#[derive(sqlx::FromRow)]
struct CorrectionImpactRow {
    geometry: Option<Json<Value>>,
    canonical_area_m2: Option<f64>,
    source_ids: Vec<Uuid>,
    excluded_base_plot_ids: Vec<Uuid>,
    valid: Option<bool>,
}

#[derive(sqlx::FromRow)]
struct CorrectionSourceRow {
    old_contribution_id: Uuid,
    previous_establishment_id: Uuid,
    external_reference_id: Option<Uuid>,
    source_type: String,
    external_name: Option<String>,
    capture_establishment_id: Uuid,
}

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
    async fn preview_normalized_geometry(
        &self,
        geometry: &NormalizedGeoJsonMultiPolygon,
    ) -> Result<GeoJsonMultiPolygon, TerritoryPreviewStoreError> {
        validate_normalized_geometry(&self.db, geometry).await
    }
}

#[async_trait]
impl TerritoryGeoJsonStore for PostgresTerritoryStore {
    async fn create_alternative_establecimiento(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        request: NewAlternativeEstablecimiento,
    ) -> Result<CreatedAlternativeEstablecimiento, TerritorySourceStoreError> {
        let mut transaction = self
            .db
            .begin()
            .await
            .map_err(|_| TerritorySourceStoreError::Unavailable)?;
        let result = create_alternative_establecimiento_in_transaction(
            &mut transaction,
            organization_id,
            actor_id,
            request,
        )
        .await;
        match result {
            Ok(value) => transaction
                .commit()
                .await
                .map(|()| value)
                .map_err(|_| TerritorySourceStoreError::Unavailable),
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(error)
            }
        }
    }

    async fn create_geojson_lote_base(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        request: NewGeoJsonLoteBase,
    ) -> Result<CreatedGeoJsonLoteBase, TerritorySourceStoreError> {
        let mut transaction = self
            .db
            .begin()
            .await
            .map_err(|_| TerritorySourceStoreError::Unavailable)?;
        let result = create_geojson_lote_base_in_transaction(
            &mut transaction,
            organization_id,
            actor_id,
            request,
        )
        .await;
        match result {
            Ok(value) => transaction
                .commit()
                .await
                .map(|()| value)
                .map_err(|_| TerritorySourceStoreError::Unavailable),
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(error)
            }
        }
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
                   fuente.texto_fuente_original, ST_AsGeoJSON(fuente.geometria, 9, 0)::jsonb AS geometria_geojson,
                   fuente.huella_sha256, fuente.version_parser, fuente.creado_por, fuente.creado_en,
                   fuente.reemplaza_fuente_geografica_id, fuente.motivo_correccion,
                   contribucion.establecimiento_id AS establecimiento_actual_id,
                   contribucion.confirmado_por, contribucion.confirmado_en
            FROM public.fuentes_geograficas AS fuente
            LEFT JOIN public.external_references AS referencia ON referencia.id = fuente.external_reference_id
            LEFT JOIN public.fuentes_geograficas_contribuciones_canonicas AS contribucion
              ON contribucion.fuente_geografica_id = fuente.id
             AND contribucion.vigente_hasta IS NULL
            WHERE fuente.organizacion_id = $1
              AND (
                  fuente.establecimiento_id = $2
                  OR EXISTS (
                      SELECT 1 FROM public.fuentes_geograficas_contribuciones_canonicas AS historia
                      WHERE historia.fuente_geografica_id = fuente.id
                        AND historia.establecimiento_id = $2
                  )
              )
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

#[async_trait]
impl TerritoryCorrectionStore for PostgresTerritoryStore {
    async fn preview_geographic_source_correction(
        &self,
        organization_id: Uuid,
        source_id: Uuid,
        target_establishment_id: Uuid,
        corrected_geometry: Option<&NormalizedGeoJsonMultiPolygon>,
    ) -> Result<GeographicSourceCorrectionPreview, TerritoryCorrectionStoreError> {
        let mut connection = self
            .db
            .acquire()
            .await
            .map_err(|_| TerritoryCorrectionStoreError::Unavailable)?;
        correction_preview_on_connection(
            &mut connection,
            organization_id,
            source_id,
            target_establishment_id,
            corrected_geometry,
        )
        .await
    }

    async fn confirm_geographic_source_correction(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        command: GeographicSourceCorrectionCommand,
    ) -> Result<GeographicSourceCorrectionResult, TerritoryCorrectionStoreError> {
        let mut transaction = self
            .db
            .begin()
            .await
            .map_err(|_| TerritoryCorrectionStoreError::Unavailable)?;
        let result =
            confirm_correction_in_transaction(&mut transaction, organization_id, actor_id, command)
                .await;
        match result {
            Ok(result) => transaction
                .commit()
                .await
                .map(|()| result)
                .map_err(|_| TerritoryCorrectionStoreError::Unavailable),
            Err(error) => {
                let _ = transaction.rollback().await;
                Err(error)
            }
        }
    }
}

async fn correction_preview_on_connection(
    connection: &mut PgConnection,
    organization_id: Uuid,
    source_id: Uuid,
    target_establishment_id: Uuid,
    corrected_geometry: Option<&NormalizedGeoJsonMultiPolygon>,
) -> Result<GeographicSourceCorrectionPreview, TerritoryCorrectionStoreError> {
    if let Some(geometry) = corrected_geometry {
        validate_normalized_geometry(&mut *connection, geometry)
            .await
            .map_err(TerritoryCorrectionStoreError::Validation)?;
    }
    let source: Option<(Uuid, Json<Value>)> = sqlx::query_as(
        r#"
        SELECT contribucion.establecimiento_id,
               ST_AsGeoJSON(fuente.geometria, 9, 0)::jsonb
        FROM public.fuentes_geograficas AS fuente
        JOIN public.fuentes_geograficas_contribuciones_canonicas AS contribucion
          ON contribucion.fuente_geografica_id = fuente.id
         AND contribucion.vigente_hasta IS NULL
        WHERE fuente.organizacion_id = $1 AND fuente.id = $2
        "#,
    )
    .bind(organization_id)
    .bind(source_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(|_| TerritoryCorrectionStoreError::Unavailable)?;
    let Some((current_establishment_id, Json(source_geometry))) = source else {
        return Err(TerritoryCorrectionStoreError::NotFound);
    };
    let target_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM public.establecimientos WHERE organizacion_id = $1 AND id = $2)",
    )
    .bind(organization_id)
    .bind(target_establishment_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(|_| TerritoryCorrectionStoreError::Unavailable)?;
    if !target_exists {
        return Err(TerritoryCorrectionStoreError::NotFound);
    }
    let effective_geometry = corrected_geometry
        .map(NormalizedGeoJsonMultiPolygon::as_geojson)
        .unwrap_or(source_geometry);
    let mut affected_establishments = vec![current_establishment_id];
    if target_establishment_id != current_establishment_id {
        affected_establishments.push(target_establishment_id);
    }
    affected_establishments.sort_unstable();

    let mut impacts = Vec::with_capacity(affected_establishments.len());
    let mut conflicts = Vec::new();
    if corrected_geometry.is_none() && target_establishment_id == current_establishment_id {
        conflicts.push("correccion_sin_cambios".to_owned());
    }
    for establishment_id in affected_establishments {
        let row: CorrectionImpactRow = sqlx::query_as(
                r#"
                WITH candidatas AS MATERIALIZED (
                    SELECT fuente.id, fuente.geometria
                    FROM public.fuentes_geograficas_contribuciones_canonicas AS contribucion
                    JOIN public.fuentes_geograficas AS fuente
                      ON fuente.id = contribucion.fuente_geografica_id
                     AND fuente.organizacion_id = contribucion.organizacion_id
                    WHERE contribucion.organizacion_id = $1
                      AND contribucion.establecimiento_id = $2
                      AND contribucion.vigente_hasta IS NULL
                      AND fuente.id <> $3
                    UNION ALL
                    SELECT $3, ST_Multi(ST_SetSRID(ST_GeomFromGeoJSON($5::jsonb), 4326))
                    WHERE $2 = $4
                ), canonica AS (
                    SELECT ST_Multi(ST_UnaryUnion(ST_Collect(geometria))) AS geometria,
                           array_agg(id ORDER BY id) AS ids
                    FROM candidatas
                )
                SELECT ST_AsGeoJSON(canonica.geometria, 9, 0)::jsonb AS geometry,
                       ST_Area(canonica.geometria::geography)::float8 AS canonical_area_m2,
                       COALESCE(canonica.ids, ARRAY[]::uuid[]) AS source_ids,
                       COALESCE((
                           SELECT array_agg(lote.id ORDER BY lote.id)
                           FROM public.lotes_base AS lote
                           WHERE lote.organizacion_id = $1
                             AND lote.establecimiento_id = $2
                             AND lote.activo
                             AND (canonica.geometria IS NULL OR NOT ST_CoveredBy(lote.geometria, canonica.geometria))
                       ), ARRAY[]::uuid[]) AS excluded_base_plot_ids,
                       CASE WHEN canonica.geometria IS NULL THEN NULL
                            ELSE ST_IsValid(canonica.geometria) AND NOT ST_IsEmpty(canonica.geometria)
                                 AND GeometryType(canonica.geometria) = 'MULTIPOLYGON'
                                 AND ST_SRID(canonica.geometria) = 4326 END AS valid
                FROM canonica
                "#,
            )
            .bind(organization_id)
            .bind(establishment_id)
            .bind(source_id)
            .bind(target_establishment_id)
            .bind(Json(effective_geometry.clone()))
            .fetch_one(&mut *connection)
            .await
            .map_err(|_| TerritoryCorrectionStoreError::Unavailable)?;
        if row.geometry.is_none() || row.valid != Some(true) {
            conflicts.push("establecimiento_sin_geometria_confirmada".to_owned());
        }
        if !row.excluded_base_plot_ids.is_empty() {
            conflicts.push("lotes_base_fuera_geometria_canonica".to_owned());
        }
        impacts.push(CanonicalGeometryImpact {
            establecimiento_id: establishment_id,
            source_ids: row.source_ids,
            geometry: row
                .geometry
                .map(|Json(value)| multipolygon_from_geojson(value))
                .transpose()
                .map_err(|_| TerritoryCorrectionStoreError::Unavailable)?,
            canonical_area_m2: row.canonical_area_m2,
            excluded_base_plot_ids: row.excluded_base_plot_ids,
        });
    }
    conflicts.sort();
    conflicts.dedup();
    Ok(GeographicSourceCorrectionPreview {
        source_id,
        current_establishment_id,
        target_establishment_id,
        creates_revision: corrected_geometry.is_some(),
        impacts,
        conflicts,
    })
}

async fn confirm_correction_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    actor_id: Uuid,
    command: GeographicSourceCorrectionCommand,
) -> Result<GeographicSourceCorrectionResult, TerritoryCorrectionStoreError> {
    let operation = OperationCode::new("territorio.corregir_fuente_geografica")
        .expect("territorial correction operation code must be canonical");
    let decision = idempotency::begin(
        transaction,
        IdempotencyRequest {
            organization_id: Some(organization_id),
            operation,
            key: command.idempotency_key,
            request_fingerprint: command.idempotency_request_fingerprint,
        },
    )
    .await
    .map_err(map_correction_idempotency_error)?;
    if let IdempotencyDecision::Replay(replay) = decision {
        let value = replay.result.as_value();
        return Ok(GeographicSourceCorrectionResult {
            previous_source_id: parse_replay_uuid(value, "fuente_anterior_id")?,
            active_source_id: parse_replay_uuid(value, "fuente_activa_id")?,
            previous_establishment_id: parse_replay_uuid(value, "establecimiento_anterior_id")?,
            target_establishment_id: parse_replay_uuid(value, "establecimiento_destino_id")?,
            created_revision: value
                .get("revision_creada")
                .and_then(Value::as_bool)
                .ok_or(TerritoryCorrectionStoreError::Unavailable)?,
            geometry_version_ids: value
                .get("version_geometria_ids")
                .cloned()
                .and_then(|ids| serde_json::from_value(ids).ok())
                .ok_or(TerritoryCorrectionStoreError::Unavailable)?,
            applied: false,
        });
    }
    let IdempotencyDecision::Proceed(pending) = decision else {
        unreachable!()
    };

    let source: Option<CorrectionSourceRow> = sqlx::query_as(
        r#"
        SELECT contribucion.id AS old_contribution_id,
               contribucion.establecimiento_id AS previous_establishment_id,
               fuente.external_reference_id,
               fuente.tipo_origen AS source_type,
               fuente.nombre_externo AS external_name,
               fuente.establecimiento_id AS capture_establishment_id
        FROM public.fuentes_geograficas AS fuente
        JOIN public.fuentes_geograficas_contribuciones_canonicas AS contribucion
          ON contribucion.fuente_geografica_id = fuente.id
         AND contribucion.vigente_hasta IS NULL
        WHERE fuente.organizacion_id = $1 AND fuente.id = $2
        FOR UPDATE OF fuente, contribucion
        "#,
    )
    .bind(organization_id)
    .bind(command.source_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| TerritoryCorrectionStoreError::Unavailable)?;
    let Some(CorrectionSourceRow {
        old_contribution_id,
        previous_establishment_id,
        external_reference_id,
        source_type,
        external_name,
        capture_establishment_id,
    }) = source
    else {
        return Err(TerritoryCorrectionStoreError::NotFound);
    };
    let mut affected = vec![previous_establishment_id];
    if command.target_establishment_id != previous_establishment_id {
        affected.push(command.target_establishment_id);
    }
    affected.sort_unstable();
    let locked: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM public.establecimientos WHERE organizacion_id = $1 AND id = ANY($2::uuid[]) ORDER BY id FOR UPDATE",
    )
    .bind(organization_id)
    .bind(&affected)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| TerritoryCorrectionStoreError::Unavailable)?;
    if locked != affected {
        return Err(TerritoryCorrectionStoreError::NotFound);
    }
    if let Some(geometry) = command.corrected_geometry.as_ref() {
        validate_normalized_geometry(&mut **transaction, geometry)
            .await
            .map_err(TerritoryCorrectionStoreError::Validation)?;
    }
    let preview = correction_preview_on_connection(
        transaction,
        organization_id,
        command.source_id,
        command.target_establishment_id,
        command.corrected_geometry.as_ref(),
    )
    .await?;
    if let Some(code) = preview.conflicts.first() {
        let affected_base_plot_ids = preview
            .impacts
            .iter()
            .flat_map(|impact| impact.excluded_base_plot_ids.iter().copied())
            .collect();
        let message = match code.as_str() {
            "establecimiento_sin_geometria_confirmada" => {
                "La corrección dejaría un establecimiento confirmado sin geometría canónica válida."
            }
            "lotes_base_fuera_geometria_canonica" => {
                "La corrección dejaría lotes base actuales fuera de la geometría canónica."
            }
            _ => "La corrección no produce ningún cambio territorial.",
        };
        return Err(TerritoryCorrectionStoreError::Conflict {
            code: code.clone(),
            message: message.to_owned(),
            affected_base_plot_ids,
        });
    }

    let active_source_id = if let Some(geometry) = command.corrected_geometry.as_ref() {
        let replacement_id = Uuid::new_v4();
        let original_text = serde_json::to_string(&geometry.as_geojson())
            .map_err(|_| TerritoryCorrectionStoreError::Unavailable)?;
        sqlx::query(
            r#"
            INSERT INTO public.fuentes_geograficas (
                id, organizacion_id, establecimiento_id, external_reference_id, tipo_origen,
                nombre_externo, texto_fuente_original, geometria, huella_sha256,
                version_parser, creado_por, reemplaza_fuente_geografica_id, motivo_correccion
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                ST_Multi(ST_SetSRID(ST_GeomFromGeoJSON($8::jsonb), 4326)),
                $9, $10, $11, $12, $13
            )
            "#,
        )
        .bind(replacement_id)
        .bind(organization_id)
        .bind(capture_establishment_id)
        .bind(external_reference_id)
        .bind(source_type)
        .bind(external_name)
        .bind(original_text)
        .bind(Json(geometry.as_geojson()))
        .bind(
            command
                .replacement_fingerprint
                .expect("correction geometry has fingerprint")
                .as_bytes()
                .as_slice(),
        )
        .bind(CORRECTION_PARSER_VERSION)
        .bind(actor_id)
        .bind(command.source_id)
        .bind(&command.correction_reason)
        .execute(&mut **transaction)
        .await
        .map_err(map_correction_database_error)?;
        replacement_id
    } else {
        command.source_id
    };
    let new_contribution_id = Uuid::new_v4();
    sqlx::query(
        r#"
        UPDATE public.fuentes_geograficas_contribuciones_canonicas
        SET vigente_hasta = statement_timestamp(), cerrada_por = $1,
            reemplazada_por_contribucion_id = $2
        WHERE id = $3 AND organizacion_id = $4 AND vigente_hasta IS NULL
        "#,
    )
    .bind(actor_id)
    .bind(new_contribution_id)
    .bind(old_contribution_id)
    .bind(organization_id)
    .execute(&mut **transaction)
    .await
    .map_err(map_correction_database_error)?;
    sqlx::query(
        r#"
        INSERT INTO public.fuentes_geograficas_contribuciones_canonicas (
            id, organizacion_id, establecimiento_id, fuente_geografica_id, confirmado_por
        ) VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(new_contribution_id)
    .bind(organization_id)
    .bind(command.target_establishment_id)
    .bind(active_source_id)
    .bind(actor_id)
    .execute(&mut **transaction)
    .await
    .map_err(map_correction_database_error)?;

    let mut geometry_version_ids = Vec::with_capacity(affected.len());
    for establishment_id in &affected {
        geometry_version_ids.push(
            persist_canonical_geometry_version(
                transaction,
                organization_id,
                *establishment_id,
                actor_id,
            )
            .await
            .map_err(map_correction_database_error)?,
        );
    }
    audit::record(
        transaction,
        &NewAuditEvent {
            organization_id,
            actor: AuditActor::Usuario(actor_id),
            action: "fuente_geografica.corregida_reasignada",
            entity_type: "fuente_geografica",
            entity_id: Some(active_source_id),
            reference: Some(command.correction_reason.as_str()),
            before_state: Some(json!({
                "fuente_geografica_id": command.source_id,
                "establecimiento_id": previous_establishment_id,
                "contribucion_id": old_contribution_id,
            })),
            after_state: Some(json!({
                "fuente_geografica_id": active_source_id,
                "reemplaza_fuente_geografica_id": (active_source_id != command.source_id).then_some(command.source_id),
                "establecimiento_id": command.target_establishment_id,
                "contribucion_id": new_contribution_id,
                "version_geometria_ids": geometry_version_ids,
            })),
        },
    )
    .await
    .map_err(|_| TerritoryCorrectionStoreError::Unavailable)?;
    let replay_result = SafeIdempotencyResult::new(json!({
        "fuente_anterior_id": command.source_id,
        "fuente_activa_id": active_source_id,
        "establecimiento_anterior_id": previous_establishment_id,
        "establecimiento_destino_id": command.target_establishment_id,
        "revision_creada": active_source_id != command.source_id,
        "version_geometria_ids": geometry_version_ids,
    }))
    .expect("geographic correction replay result is bounded");
    idempotency::complete(transaction, pending, replay_result)
        .await
        .map_err(map_correction_idempotency_error)?;
    Ok(GeographicSourceCorrectionResult {
        previous_source_id: command.source_id,
        active_source_id,
        previous_establishment_id,
        target_establishment_id: command.target_establishment_id,
        created_revision: active_source_id != command.source_id,
        geometry_version_ids,
        applied: true,
    })
}

fn parse_replay_uuid(value: &Value, field: &str) -> Result<Uuid, TerritoryCorrectionStoreError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or(TerritoryCorrectionStoreError::Unavailable)
}

fn map_correction_idempotency_error(error: IdempotencyError) -> TerritoryCorrectionStoreError {
    match error {
        IdempotencyError::Conflict(_) => TerritoryCorrectionStoreError::Conflict {
            code: "idempotency_key_en_conflicto".to_owned(),
            message: "La clave de idempotencia ya pertenece a otra corrección.".to_owned(),
            affected_base_plot_ids: Vec::new(),
        },
        IdempotencyError::InvariantViolation | IdempotencyError::Database(_) => {
            TerritoryCorrectionStoreError::Unavailable
        }
    }
}

fn map_correction_database_error(error: sqlx::Error) -> TerritoryCorrectionStoreError {
    match &error {
        sqlx::Error::Database(database_error)
            if matches!(
                database_error.code().as_deref(),
                Some("23505" | "23503" | "P0001")
            ) =>
        {
            TerritoryCorrectionStoreError::Conflict {
                code: "correccion_geografica_en_conflicto".to_owned(),
                message: "La corrección entra en conflicto con el estado territorial actual."
                    .to_owned(),
                affected_base_plot_ids: Vec::new(),
            }
        }
        _ => TerritoryCorrectionStoreError::Unavailable,
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
          AND vigente_hasta IS NULL
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
        let replacement_contribution_ids = source_ids
            .iter()
            .map(|_| Uuid::new_v4())
            .collect::<Vec<_>>();
        sqlx::query(
            r#"
            UPDATE public.fuentes_geograficas_contribuciones_canonicas
            SET vigente_hasta = statement_timestamp(),
                cerrada_por = $3
            WHERE organizacion_id = $1 AND establecimiento_id = $2
              AND vigente_hasta IS NULL
            "#,
        )
        .bind(organization_id)
        .bind(grouping.establecimiento_id)
        .bind(actor_id)
        .execute(&mut **transaction)
        .await
        .map_err(map_grouping_database_error)?;
        sqlx::query(
            r#"
            INSERT INTO public.fuentes_geograficas_contribuciones_canonicas (
                id, organizacion_id, establecimiento_id, fuente_geografica_id, confirmado_por
            )
            SELECT contribucion_id, $1, $2, fuente_id, $5
            FROM unnest($3::uuid[], $4::uuid[]) AS nueva(fuente_id, contribucion_id)
            "#,
        )
        .bind(organization_id)
        .bind(grouping.establecimiento_id)
        .bind(source_ids)
        .bind(&replacement_contribution_ids)
        .bind(actor_id)
        .execute(&mut **transaction)
        .await
        .map_err(map_grouping_database_error)?;
        persist_canonical_geometry_version(
            transaction,
            organization_id,
            grouping.establecimiento_id,
            actor_id,
        )
        .await
        .map_err(|_| TerritorySourceGroupingStoreError::Conflict)?;

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

async fn persist_canonical_geometry_version(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    establishment_id: Uuid,
    actor_id: Uuid,
) -> Result<Uuid, sqlx::Error> {
    let (Json(geometry), provenance, source_ids, previous_version_id): (
        Json<Value>,
        String,
        Vec<Uuid>,
        Option<Uuid>,
    ) = sqlx::query_as(
        r#"
        WITH fuentes_activas AS MATERIALIZED (
            SELECT fuente.id, fuente.geometria, fuente.tipo_origen
            FROM public.fuentes_geograficas_contribuciones_canonicas AS contribucion
            JOIN public.fuentes_geograficas AS fuente
              ON fuente.id = contribucion.fuente_geografica_id
             AND fuente.organizacion_id = contribucion.organizacion_id
            WHERE contribucion.organizacion_id = $1
              AND contribucion.establecimiento_id = $2
              AND contribucion.vigente_hasta IS NULL
        )
        SELECT ST_AsGeoJSON(ST_Multi(ST_UnaryUnion(ST_Collect(geometria))), 9, 0)::jsonb,
               CASE
                   WHEN bool_and(tipo_origen = 'senasa_renspa') THEN 'senasa_renspa'
                   WHEN bool_and(tipo_origen = 'importada') THEN 'importada'
                   ELSE 'manual'
               END,
               array_agg(id ORDER BY id),
               (SELECT geometria_version_actual_id FROM public.establecimientos
                WHERE id = $2 AND organizacion_id = $1)
        FROM fuentes_activas
        "#,
    )
    .bind(organization_id)
    .bind(establishment_id)
    .fetch_one(&mut **transaction)
    .await?;
    let version_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO public.establecimientos_geometrias_versiones (
            id, organizacion_id, establecimiento_id, version_anterior_id,
            geometria, origen_geometria, fuente_geografica_ids, creado_por
        ) VALUES (
            $1, $2, $3, $4,
            ST_Multi(ST_SetSRID(ST_GeomFromGeoJSON($5::jsonb), $6)), $7, $8, $9
        )
        "#,
    )
    .bind(version_id)
    .bind(organization_id)
    .bind(establishment_id)
    .bind(previous_version_id)
    .bind(Json(geometry))
    .bind(TERRITORIAL_SRID)
    .bind(provenance.as_str())
    .bind(&source_ids)
    .bind(actor_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        r#"
        UPDATE public.establecimientos AS establecimiento
        SET geometria = version.geometria,
            origen_geometria = version.origen_geometria,
            geometria_version_actual_id = version.id
        FROM public.establecimientos_geometrias_versiones AS version
        WHERE establecimiento.id = $1
          AND establecimiento.organizacion_id = $2
          AND version.id = $3
        "#,
    )
    .bind(establishment_id)
    .bind(organization_id)
    .bind(version_id)
    .execute(&mut **transaction)
    .await?;
    Ok(version_id)
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

#[derive(sqlx::FromRow)]
struct GeographicSourceRow {
    id: Uuid,
    establecimiento_id: Uuid,
    external_reference_id: Option<Uuid>,
    external_id: Option<String>,
    tipo_origen: String,
    nombre_externo: Option<String>,
    texto_fuente_original: String,
    geometria_geojson: Json<Value>,
    huella_sha256: Vec<u8>,
    version_parser: String,
    creado_por: Uuid,
    creado_en: OffsetDateTime,
    reemplaza_fuente_geografica_id: Option<Uuid>,
    motivo_correccion: Option<String>,
    establecimiento_actual_id: Option<Uuid>,
    confirmado_por: Option<Uuid>,
    confirmado_en: Option<OffsetDateTime>,
}

async fn validate_normalized_geometry<'e, E>(
    executor: E,
    geometry: &NormalizedGeoJsonMultiPolygon,
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
    .bind(Json(geometry.as_geojson()))
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
    let normalized = crate::territory::geojson::normalize_geometry(
        source.normalized_polygon.as_geojson_polygon(),
    )
    .expect("parsed SENASA geometry is valid GeoJSON structure");
    validate_normalized_geometry(&mut **transaction, &normalized)
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
                  texto_fuente_original, ST_AsGeoJSON(geometria, 9, 0)::jsonb AS geometria_geojson,
                  huella_sha256, version_parser, creado_por, creado_en,
                  reemplaza_fuente_geografica_id, motivo_correccion,
                  NULL::uuid AS establecimiento_actual_id,
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
                    reference: source.external_id.as_deref(),
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

async fn create_alternative_establecimiento_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    actor_id: Uuid,
    request: NewAlternativeEstablecimiento,
) -> Result<CreatedAlternativeEstablecimiento, TerritorySourceStoreError> {
    let operation = OperationCode::new("territorio.crear_establecimiento_geojson")
        .expect("operation code is canonical");
    let decision = idempotency::begin(
        transaction,
        IdempotencyRequest {
            organization_id: Some(organization_id),
            operation,
            key: request.idempotency_key.clone(),
            request_fingerprint: request.idempotency_request_fingerprint,
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
        let source = load_geographic_source_by_id(transaction, organization_id, source_id).await?;
        let establecimiento = load_establecimiento_in_transaction(
            transaction,
            organization_id,
            source.establecimiento_id,
        )
        .await?;
        return Ok(CreatedAlternativeEstablecimiento {
            establecimiento,
            source,
            created: false,
        });
    }
    let IdempotencyDecision::Proceed(pending) = decision else {
        unreachable!()
    };
    validate_normalized_geometry(&mut **transaction, &request.geometry)
        .await
        .map_err(TerritorySourceStoreError::Validation)?;
    let id = Uuid::new_v4();
    let establishment_row: (Uuid, Uuid, String, String, String, String, bool, Uuid, OffsetDateTime) = sqlx::query_as(r#"
        INSERT INTO public.establecimientos (id, organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por)
        VALUES ($1, $2, $3, $4, ST_Multi(ST_SetSRID(ST_GeomFromGeoJSON($5::jsonb), $6)), $7, $8)
        RETURNING id, organizacion_id, codigo, nombre, ST_AsEWKT(geometria), origen_geometria, activo, creado_por, creado_en
    "#).bind(id).bind(organization_id).bind(request.codigo.as_str()).bind(request.nombre.as_str()).bind(Json(request.geometry.as_geojson())).bind(TERRITORIAL_SRID).bind(request.source_type.as_str()).bind(actor_id).fetch_one(&mut **transaction).await.map_err(map_source_database_error)?;
    let establecimiento = establecimiento_from_row(establishment_row)
        .map_err(|_| TerritorySourceStoreError::Unavailable)?;
    if let Some(renspa) = request.renspa.clone() {
        external_references::register(
            transaction,
            NewExternalReference {
                organization_id: Some(organization_id),
                system: ExternalSystem::new(RENSPA_SYSTEM).expect("system is canonical"),
                external_id: renspa,
                entity_type: ExternalEntityType::new(ESTABLECIMIENTO_ENTITY_TYPE)
                    .expect("entity is canonical"),
                entity_id: id,
                sync_version: None,
                last_sync_status: LastSyncStatus::Pendiente,
                last_synced_at: None,
            },
        )
        .await
        .map_err(map_external_reference_source_error)?;
    }
    let original_geojson = serde_json::to_string(&request.original_geojson)
        .map_err(|_| TerritorySourceStoreError::Unavailable)?;
    let row: GeographicSourceRow = sqlx::query_as(r#"
        INSERT INTO public.fuentes_geograficas (organizacion_id, establecimiento_id, external_reference_id, tipo_origen, nombre_externo, texto_fuente_original, geometria, huella_sha256, version_parser, creado_por)
        VALUES ($1, $2, NULL, $3, NULL, $4, ST_Multi(ST_SetSRID(ST_GeomFromGeoJSON($5::jsonb), $6)), $7, $8, $9)
        RETURNING id, establecimiento_id, external_reference_id, NULL::text AS external_id, tipo_origen, nombre_externo, texto_fuente_original, ST_AsGeoJSON(geometria, 9, 0)::jsonb AS geometria_geojson, huella_sha256, version_parser, creado_por, creado_en, reemplaza_fuente_geografica_id, motivo_correccion, NULL::uuid AS establecimiento_actual_id, NULL::uuid AS confirmado_por, NULL::timestamptz AS confirmado_en
    "#).bind(organization_id).bind(id).bind(request.source_type.as_str()).bind(original_geojson).bind(Json(request.geometry.as_geojson())).bind(TERRITORIAL_SRID).bind(request.fingerprint.as_bytes().as_slice()).bind(GEOJSON_INPUT_VERSION).bind(actor_id).fetch_one(&mut **transaction).await.map_err(map_source_database_error)?;
    let source = geographic_source_from_row(row)?;
    sqlx::query("INSERT INTO public.fuentes_geograficas_contribuciones_canonicas (organizacion_id, establecimiento_id, fuente_geografica_id, confirmado_por) VALUES ($1, $2, $3, $4)")
        .bind(organization_id).bind(id).bind(source.id).bind(actor_id).execute(&mut **transaction).await.map_err(map_source_database_error)?;
    persist_canonical_geometry_version(transaction, organization_id, id, actor_id)
        .await
        .map_err(map_source_database_error)?;
    audit::record(transaction, &NewAuditEvent { organization_id, actor: AuditActor::Usuario(actor_id), action: "establecimiento.creado", entity_type: "establecimiento", entity_id: Some(id), reference: Some(request.codigo.as_str()), before_state: None, after_state: Some(json!({"establecimiento_id": id, "codigo": request.codigo.as_str(), "nombre": request.nombre.as_str(), "origen_geometria": request.source_type.as_str(), "activo": true})) }).await.map_err(|_| TerritorySourceStoreError::Unavailable)?;
    audit::record(transaction, &NewAuditEvent { organization_id, actor: AuditActor::Usuario(actor_id), action: "fuente_geografica.alternativa_confirmada", entity_type: "fuente_geografica", entity_id: Some(source.id), reference: Some(request.codigo.as_str()), before_state: None, after_state: Some(json!({"fuente_geografica_id": source.id, "establecimiento_id": id, "tipo_origen": request.source_type.as_str(), "huella_sha256": source.fingerprint.to_hex(), "version_parser": source.parser_version})) }).await.map_err(|_| TerritorySourceStoreError::Unavailable)?;
    idempotency::complete(transaction, pending, SafeIdempotencyResult::new(json!({"establecimiento_id": id.to_string(), "fuente_geografica_id": source.id.to_string()})).expect("result is small")).await.map_err(map_idempotency_error)?;
    Ok(CreatedAlternativeEstablecimiento {
        establecimiento,
        source,
        created: true,
    })
}

async fn create_geojson_lote_base_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    actor_id: Uuid,
    request: NewGeoJsonLoteBase,
) -> Result<CreatedGeoJsonLoteBase, TerritorySourceStoreError> {
    let operation = OperationCode::new("territorio.crear_lote_base_geojson")
        .expect("operation code is canonical");
    let decision = idempotency::begin(
        transaction,
        IdempotencyRequest {
            organization_id: Some(organization_id),
            operation,
            key: request.idempotency_key.clone(),
            request_fingerprint: request.idempotency_request_fingerprint,
        },
    )
    .await
    .map_err(map_idempotency_error)?;
    if let IdempotencyDecision::Replay(replay) = decision {
        let id = replay
            .result
            .as_value()
            .get("lote_base_id")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or(TerritorySourceStoreError::Unavailable)?;
        return load_lote_base_in_transaction(transaction, organization_id, id)
            .await
            .map(|lote_base| CreatedGeoJsonLoteBase {
                lote_base,
                created: false,
            });
    }
    let IdempotencyDecision::Proceed(pending) = decision else {
        unreachable!()
    };
    let parent_exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM public.establecimientos WHERE id = $1 AND organizacion_id = $2",
    )
    .bind(request.establecimiento_id)
    .bind(organization_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(|_| TerritorySourceStoreError::Unavailable)?;
    if parent_exists.is_none() {
        return Err(TerritorySourceStoreError::NotFound);
    }
    validate_normalized_geometry(&mut **transaction, &request.geometry)
        .await
        .map_err(TerritorySourceStoreError::Validation)?;
    let id = Uuid::new_v4();
    let row: (Uuid, Uuid, Uuid, String, String, String, bool, Uuid, OffsetDateTime) = sqlx::query_as(r#"
        INSERT INTO public.lotes_base (id, organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por)
        VALUES ($1, $2, $3, $4, $5, ST_Multi(ST_SetSRID(ST_GeomFromGeoJSON($6::jsonb), $7)), $8)
        RETURNING id, organizacion_id, establecimiento_id, codigo, nombre, ST_AsEWKT(geometria), activo, creado_por, creado_en
    "#).bind(id).bind(organization_id).bind(request.establecimiento_id).bind(request.codigo.as_str()).bind(request.nombre.as_str()).bind(Json(request.geometry.as_geojson())).bind(TERRITORIAL_SRID).bind(actor_id).fetch_one(&mut **transaction).await.map_err(map_source_database_error)?;
    let lote_base = lote_base_from_row(row).map_err(|_| TerritorySourceStoreError::Unavailable)?;
    audit::record(transaction, &NewAuditEvent { organization_id, actor: AuditActor::Usuario(actor_id), action: "lote_base.creado", entity_type: "lote_base", entity_id: Some(id), reference: Some(request.codigo.as_str()), before_state: None, after_state: Some(json!({"lote_base_id": id, "establecimiento_id": request.establecimiento_id, "codigo": request.codigo.as_str(), "nombre": request.nombre.as_str(), "srid": TERRITORIAL_SRID, "activo": true})) }).await.map_err(|_| TerritorySourceStoreError::Unavailable)?;
    idempotency::complete(
        transaction,
        pending,
        SafeIdempotencyResult::new(json!({"lote_base_id": id.to_string()}))
            .expect("result is small"),
    )
    .await
    .map_err(map_idempotency_error)?;
    Ok(CreatedGeoJsonLoteBase {
        lote_base,
        created: true,
    })
}

async fn load_establecimiento_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    id: Uuid,
) -> Result<Establecimiento, TerritorySourceStoreError> {
    let row = sqlx::query_as("SELECT id, organizacion_id, codigo, nombre, ST_AsEWKT(geometria), origen_geometria, activo, creado_por, creado_en FROM public.establecimientos WHERE id = $1 AND organizacion_id = $2")
        .bind(id).bind(organization_id).fetch_optional(&mut **transaction).await.map_err(|_| TerritorySourceStoreError::Unavailable)?;
    row.map(establecimiento_from_row)
        .transpose()
        .map_err(|_| TerritorySourceStoreError::Unavailable)?
        .ok_or(TerritorySourceStoreError::NotFound)
}

async fn load_lote_base_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    id: Uuid,
) -> Result<LoteBase, TerritorySourceStoreError> {
    let row = sqlx::query_as("SELECT id, organizacion_id, establecimiento_id, codigo, nombre, ST_AsEWKT(geometria), activo, creado_por, creado_en FROM public.lotes_base WHERE id = $1 AND organizacion_id = $2")
        .bind(id).bind(organization_id).fetch_optional(&mut **transaction).await.map_err(|_| TerritorySourceStoreError::Unavailable)?;
    row.map(lote_base_from_row)
        .transpose()
        .map_err(|_| TerritorySourceStoreError::Unavailable)?
        .ok_or(TerritorySourceStoreError::NotFound)
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
               fuente.texto_fuente_original, ST_AsGeoJSON(fuente.geometria, 9, 0)::jsonb AS geometria_geojson,
               fuente.huella_sha256, fuente.version_parser, fuente.creado_por, fuente.creado_en,
               fuente.reemplaza_fuente_geografica_id, fuente.motivo_correccion,
               contribucion.establecimiento_id AS establecimiento_actual_id,
               contribucion.confirmado_por, contribucion.confirmado_en
        FROM public.fuentes_geograficas AS fuente
        LEFT JOIN public.external_references AS referencia ON referencia.id = fuente.external_reference_id
        LEFT JOIN public.fuentes_geograficas_contribuciones_canonicas AS contribucion
          ON contribucion.fuente_geografica_id = fuente.id
         AND contribucion.vigente_hasta IS NULL
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
               fuente.texto_fuente_original, ST_AsGeoJSON(fuente.geometria, 9, 0)::jsonb AS geometria_geojson,
               fuente.huella_sha256, fuente.version_parser, fuente.creado_por, fuente.creado_en,
               fuente.reemplaza_fuente_geografica_id, fuente.motivo_correccion,
               contribucion.establecimiento_id AS establecimiento_actual_id,
               contribucion.confirmado_por, contribucion.confirmado_en
        FROM public.fuentes_geograficas AS fuente
        LEFT JOIN public.external_references AS referencia ON referencia.id = fuente.external_reference_id
        LEFT JOIN public.fuentes_geograficas_contribuciones_canonicas AS contribucion
          ON contribucion.fuente_geografica_id = fuente.id
         AND contribucion.vigente_hasta IS NULL
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
    row: GeographicSourceRow,
) -> Result<GeographicSourceView, TerritorySourceStoreError> {
    let fingerprint: [u8; 32] = row
        .huella_sha256
        .try_into()
        .map_err(|_| TerritorySourceStoreError::Unavailable)?;
    let source_type = match row.tipo_origen.as_str() {
        "senasa_renspa"
            if matches!(
                row.version_parser.as_str(),
                SENASA_PARSER_VERSION | CORRECTION_PARSER_VERSION
            ) && row.external_reference_id.is_some()
                && row.external_id.is_some() =>
        {
            GeographicSourceType::SenasaRenspa
        }
        "manual"
            if matches!(
                row.version_parser.as_str(),
                GEOJSON_INPUT_VERSION | CORRECTION_PARSER_VERSION
            ) && row.external_reference_id.is_none()
                && row.external_id.is_none() =>
        {
            GeographicSourceType::Manual
        }
        "importada"
            if matches!(
                row.version_parser.as_str(),
                GEOJSON_INPUT_VERSION | CORRECTION_PARSER_VERSION
            ) && row.external_reference_id.is_none()
                && row.external_id.is_none() =>
        {
            GeographicSourceType::Importada
        }
        _ => return Err(TerritorySourceStoreError::Unavailable),
    };
    Ok(GeographicSourceView {
        id: row.id,
        establecimiento_id: row.establecimiento_id,
        external_reference_id: row.external_reference_id,
        external_id: row.external_id,
        source_type,
        external_name: row.nombre_externo,
        original_source_text: row.texto_fuente_original,
        geometry: multipolygon_from_geojson(row.geometria_geojson.0)
            .map_err(|_| TerritorySourceStoreError::Unavailable)?,
        fingerprint: GeographicSourceFingerprint::from_bytes(fingerprint),
        parser_version: row.version_parser,
        created_by: row.creado_por,
        created_at: row.creado_en,
        replaces_source_id: row.reemplaza_fuente_geografica_id,
        correction_reason: row.motivo_correccion,
        current_establishment_id: row.establecimiento_actual_id,
        confirmed_for_canonical_geometry: row.confirmado_por.is_some(),
        confirmed_by: row.confirmado_por,
        confirmed_at: row.confirmado_en,
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

fn map_source_database_error(error: sqlx::Error) -> TerritorySourceStoreError {
    match &error {
        sqlx::Error::Database(database_error)
            if matches!(
                database_error.code().as_deref(),
                Some("23505" | "23503" | "P0001")
            ) =>
        {
            TerritorySourceStoreError::Conflict
        }
        _ => TerritorySourceStoreError::Unavailable,
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

        let rows: Vec<(Uuid, Uuid, Uuid, Uuid, String, String, bool, Json<Value>)> =
            sqlx::query_as(
                r#"
            SELECT id, campana_id, establecimiento_id, establecimiento_geometria_version_id,
                   codigo, nombre, activa,
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
        for (
            id,
            campana_id,
            establecimiento_id,
            establishment_geometry_version_id,
            codigo,
            nombre,
            activa,
            Json(geometria),
        ) in rows
        {
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
                establishment_geometry_version_id,
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
        let row: Option<(Uuid, Uuid, String, String, bool, Json<Value>)> = sqlx::query_as(
            r#"
            SELECT id, geometria_version_actual_id, codigo, nombre, activo,
                   ST_AsGeoJSON(geometria, 9, 0)::jsonb
            FROM public.establecimientos
            WHERE id = $1 AND organizacion_id = $2
            "#,
        )
        .bind(establecimiento_id)
        .bind(organization_id)
        .fetch_optional(&self.db)
        .await
        .map_err(map_read_database_error)?;
        let (id, geometry_version_id, codigo, nombre, activo, Json(geometria)) =
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
            geometry_version_id,
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
