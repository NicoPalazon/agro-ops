use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Row, Transaction, types::Json};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

const INSERT_AUDIT_EVENT_SQL: &str = r#"
    INSERT INTO public.audit_events (
        organizacion_id,
        actor_tipo,
        actor_usuario_id,
        accion,
        entidad_tipo,
        entidad_id,
        referencia,
        estado_anterior,
        estado_posterior
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
    RETURNING id
"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditActor {
    Usuario(Uuid),
    Sistema,
}

impl AuditActor {
    fn kind(self) -> &'static str {
        match self {
            Self::Usuario(_) => "usuario",
            Self::Sistema => "sistema",
        }
    }

    fn user_id(self) -> Option<Uuid> {
        match self {
            Self::Usuario(user_id) => Some(user_id),
            Self::Sistema => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NewAuditEvent<'a> {
    pub organization_id: Uuid,
    pub actor: AuditActor,
    pub action: &'a str,
    pub entity_type: &'a str,
    pub entity_id: Option<Uuid>,
    pub reference: Option<&'a str>,
    pub before_state: Option<Value>,
    pub after_state: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedAuditEvent {
    pub id: Uuid,
}

const MAX_DIAGNOSTIC_LIMIT: u16 = 100;

#[derive(Clone, Debug, Default, Deserialize, IntoParams)]
pub struct AuditDiagnosticsQuery {
    pub accion: Option<String>,
    pub entidad_tipo: Option<String>,
    pub actor_usuario_id: Option<String>,
    pub limite: Option<u16>,
}

/// Metadata-only view of an immutable audit fact.
///
/// Audit snapshots are intentionally not exposed here. They are extensible
/// domain data, whereas the technical console needs only to establish whether
/// a before and/or after state was recorded.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AuditDiagnostic {
    pub id: String,
    pub ocurrido_en: String,
    pub accion: String,
    pub entidad_tipo: String,
    pub entidad_id: Option<String>,
    pub referencia: Option<String>,
    pub actor_tipo: String,
    pub actor_usuario_id: Option<String>,
    pub tiene_estado_anterior: bool,
    pub tiene_estado_posterior: bool,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct AuditDiagnosticsResponse {
    pub eventos: Vec<AuditDiagnostic>,
}

pub async fn list_diagnostics(
    db: &PgPool,
    organization_id: Uuid,
    query: &AuditDiagnosticsQuery,
) -> Result<AuditDiagnosticsResponse, sqlx::Error> {
    let limit = query.limite.unwrap_or(50).clamp(1, MAX_DIAGNOSTIC_LIMIT);
    let rows = sqlx::query(
        r#"
        SELECT
            id,
            to_char(ocurrido_en AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.US"Z"') AS ocurrido_en,
            accion,
            entidad_tipo,
            entidad_id,
            referencia,
            actor_tipo,
            actor_usuario_id,
            estado_anterior IS NOT NULL AS tiene_estado_anterior,
            estado_posterior IS NOT NULL AS tiene_estado_posterior
        FROM public.audit_events
        WHERE organizacion_id = $1
          AND ($2::text IS NULL OR accion = $2)
          AND ($3::text IS NULL OR entidad_tipo = $3)
          AND ($4::text IS NULL OR actor_usuario_id::text = $4)
        ORDER BY ocurrido_en DESC, id DESC
        LIMIT $5
        "#,
    )
    .bind(organization_id)
    .bind(query.accion.as_deref())
    .bind(query.entidad_tipo.as_deref())
    .bind(query.actor_usuario_id.as_deref())
    .bind(i64::from(limit))
    .fetch_all(db)
    .await?;

    let eventos = rows
        .into_iter()
        .map(|row| {
            Ok(AuditDiagnostic {
                id: row.try_get::<Uuid, _>("id")?.to_string(),
                ocurrido_en: row.try_get("ocurrido_en")?,
                accion: row.try_get("accion")?,
                entidad_tipo: row.try_get("entidad_tipo")?,
                entidad_id: row
                    .try_get::<Option<Uuid>, _>("entidad_id")?
                    .map(|id| id.to_string()),
                referencia: row.try_get("referencia")?,
                actor_tipo: row.try_get("actor_tipo")?,
                actor_usuario_id: row
                    .try_get::<Option<Uuid>, _>("actor_usuario_id")?
                    .map(|id| id.to_string()),
                tiene_estado_anterior: row.try_get("tiene_estado_anterior")?,
                tiene_estado_posterior: row.try_get("tiene_estado_posterior")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(AuditDiagnosticsResponse { eventos })
}

/// Appends one immutable audit fact to a transaction owned by the caller.
///
/// Callers explicitly supply JSON snapshots; this primitive never serializes
/// domain structs or request data on their behalf.
pub async fn record(
    transaction: &mut Transaction<'_, Postgres>,
    event: &NewAuditEvent<'_>,
) -> Result<RecordedAuditEvent, sqlx::Error> {
    let id = sqlx::query_scalar(INSERT_AUDIT_EVENT_SQL)
        .bind(event.organization_id)
        .bind(event.actor.kind())
        .bind(event.actor.user_id())
        .bind(event.action)
        .bind(event.entity_type)
        .bind(event.entity_id)
        .bind(event.reference)
        .bind(event.before_state.clone().map(Json))
        .bind(event.after_state.clone().map(Json))
        .fetch_one(&mut **transaction)
        .await?;

    Ok(RecordedAuditEvent { id })
}
