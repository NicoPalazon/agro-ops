use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction, types::Json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    audit::{self, AuditActor, NewAuditEvent},
    external_references::{
        self, ExternalEntityType, ExternalReferenceStoreError, ExternalSystem, LastSyncStatus,
        NewExternalReference,
    },
    territory::{
        application::{
            BasePlotView, CampaignView, EstablishmentView, ExternalReferenceView,
            GeoJsonMultiPolygon, OperationalUnitView, TerritorialUseAssignmentView,
            TerritoryReadStore, TerritoryReadStoreError, TerritoryStore, TerritoryStoreError,
        },
        domain::{
            CanonicalTerritorialCode, Establecimiento, FunctionalName, GeometryProvenance,
            LoteBase, NewEstablecimiento, NewLoteBase, TERRITORIAL_SRID,
        },
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
