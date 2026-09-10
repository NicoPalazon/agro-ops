use async_trait::async_trait;
use serde_json::json;
use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    audit::{self, AuditActor, NewAuditEvent},
    external_references::{
        self, ExternalEntityType, ExternalReferenceStoreError, ExternalSystem, LastSyncStatus,
        NewExternalReference,
    },
    territory::{
        application::{TerritoryStore, TerritoryStoreError},
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
