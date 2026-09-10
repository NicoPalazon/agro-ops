use std::collections::BTreeSet;

use sqlx::PgPool;
use uuid::Uuid;

pub const SUPABASE_PROVIDER: &str = "supabase";

pub mod permission_codes {
    pub const CONFIGURACION_ADMINISTRAR: &str = "configuracion:administrar";
    pub const CONSOLA_TECNICA_VER: &str = "consola_tecnica:ver";
}

const RESOLVE_CONTEXT_SQL: &str = r#"
WITH instante AS MATERIALIZED (
    SELECT statement_timestamp() AS ahora
),
actor AS (
    SELECT
        identidad.id AS identidad_id,
        usuario.id AS usuario_id,
        organizacion.id AS organizacion_id,
        instante.ahora AS autorizado_en
    FROM instante
    JOIN public.identidades_autenticacion_externas AS identidad
      ON tstzrange(
             identidad.vinculada_en,
             identidad.desvinculada_en,
             '[)'
         ) @> instante.ahora
    JOIN public.usuarios AS usuario
      ON usuario.id = identidad.usuario_id
     AND usuario.activo
    JOIN public.organizaciones AS organizacion
      ON organizacion.id = usuario.organizacion_id
     AND organizacion.activa
    WHERE identidad.proveedor = $1
      AND identidad.sujeto_proveedor = $2
)
SELECT
    actor.identidad_id,
    actor.usuario_id,
    actor.organizacion_id,
    COALESCE(permisos.codigos, ARRAY[]::text[]) AS codigos_permisos
FROM actor
LEFT JOIN LATERAL (
    SELECT array_agg(DISTINCT permiso.codigo ORDER BY permiso.codigo) AS codigos
    FROM public.usuarios_roles AS usuario_rol
    JOIN public.roles AS rol
      ON rol.id = usuario_rol.rol_id
     AND rol.organizacion_id = actor.organizacion_id
     AND rol.activo
    JOIN public.roles_permisos AS rol_permiso
      ON rol_permiso.rol_id = rol.id
     AND tstzrange(
             rol_permiso.vigente_desde,
             rol_permiso.vigente_hasta,
             '[)'
         ) @> actor.autorizado_en
    JOIN public.permisos AS permiso
      ON permiso.id = rol_permiso.permiso_id
     AND permiso.activo
    WHERE usuario_rol.usuario_id = actor.usuario_id
      AND tstzrange(
              usuario_rol.vigente_desde,
              usuario_rol.vigente_hasta,
              '[)'
          ) @> actor.autorizado_en
) AS permisos ON TRUE;
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizationContext {
    pub user_id: Uuid,
    pub organization_id: Uuid,
    permission_codes: BTreeSet<String>,
}

impl AuthorizationContext {
    pub fn permission_codes(&self) -> &BTreeSet<String> {
        &self.permission_codes
    }

    pub fn has_permission(&self, required: &str) -> bool {
        self.permission_codes.contains(required)
    }

    pub fn require_permission(&self, required: &str) -> Result<(), PermissionDenied> {
        self.has_permission(required)
            .then_some(())
            .ok_or(PermissionDenied)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermissionDenied;

#[derive(Debug)]
pub enum ResolveAuthorizationError {
    PrincipalUnavailable,
    InvariantViolation,
    DatabaseUnavailable(sqlx::Error),
}

pub async fn resolve_context(
    db: &PgPool,
    supabase_subject: Uuid,
) -> Result<AuthorizationContext, ResolveAuthorizationError> {
    let rows: Vec<(Uuid, Uuid, Uuid, Vec<String>)> = sqlx::query_as(RESOLVE_CONTEXT_SQL)
        .bind(SUPABASE_PROVIDER)
        .bind(supabase_subject)
        .fetch_all(db)
        .await
        .map_err(ResolveAuthorizationError::DatabaseUnavailable)?;

    match rows.as_slice() {
        [] => Err(ResolveAuthorizationError::PrincipalUnavailable),
        [(_, user_id, organization_id, permission_codes)] => Ok(AuthorizationContext {
            user_id: *user_id,
            organization_id: *organization_id,
            permission_codes: permission_codes.iter().cloned().collect(),
        }),
        _ => Err(ResolveAuthorizationError::InvariantViolation),
    }
}
