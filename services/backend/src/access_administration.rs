use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use tracing::warn;
use uuid::Uuid;

use crate::{
    authorization::{
        AuthorizationContext, SUPABASE_PROVIDER, permission_codes::CONFIGURACION_ADMINISTRAR,
    },
    supabase_admin::{ExternalIdentityAdmin, ExternalIdentityAdminError},
};

type UserWithCurrentRoleRow = (
    Uuid,
    String,
    bool,
    Option<Uuid>,
    Option<Uuid>,
    Option<String>,
);
type RoleWithCurrentPermissionRow = (Uuid, String, Option<String>, bool, Option<String>);

#[derive(Clone, Debug, Serialize)]
pub struct RoleReference {
    pub id: Uuid,
    pub nombre: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct UserSummary {
    pub id: Uuid,
    pub nombre_completo: String,
    pub correo_electronico: Option<String>,
    pub activo: bool,
    pub roles: Vec<RoleReference>,
}

#[derive(Clone, Debug, Serialize)]
pub struct UsersResponse {
    pub usuarios: Vec<UserSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PermissionSummary {
    pub codigo: String,
    pub nombre: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RoleSummary {
    pub id: Uuid,
    pub nombre: String,
    pub descripcion: Option<String>,
    pub activo: bool,
    pub permisos: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RolesResponse {
    pub roles: Vec<RoleSummary>,
    pub permisos: Vec<PermissionSummary>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub correo_electronico: String,
    pub nombre_completo: String,
    #[serde(default)]
    pub roles_ids: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub nombre_completo: Option<String>,
    pub activo: Option<bool>,
    pub roles_ids: Option<Vec<Uuid>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub nombre: String,
    pub descripcion: Option<String>,
    #[serde(default)]
    pub permisos: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub nombre: Option<String>,
    pub descripcion: Option<Option<String>>,
    pub activo: Option<bool>,
    pub permisos: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessAdministrationError {
    InvalidInput,
    NotFound,
    Conflict,
    ExternalRejected,
    ExternalUnavailable,
    DatabaseUnavailable,
}

pub async fn list_users(
    db: &PgPool,
    external_admin: &dyn ExternalIdentityAdmin,
    context: &AuthorizationContext,
) -> Result<UsersResponse, AccessAdministrationError> {
    list_users_for_organization(db, external_admin, context.organization_id).await
}

async fn list_users_for_organization(
    db: &PgPool,
    external_admin: &dyn ExternalIdentityAdmin,
    organization_id: Uuid,
) -> Result<UsersResponse, AccessAdministrationError> {
    let rows: Vec<UserWithCurrentRoleRow> = sqlx::query_as(
        r#"
        SELECT usuario.id, usuario.nombre_completo, usuario.activo,
               identidad.sujeto_proveedor, rol.id, rol.nombre
        FROM public.usuarios AS usuario
        LEFT JOIN public.identidades_autenticacion_externas AS identidad
          ON identidad.usuario_id = usuario.id
         AND identidad.proveedor = $2
         AND tstzrange(identidad.vinculada_en, identidad.desvinculada_en, '[)') @> statement_timestamp()
        LEFT JOIN public.usuarios_roles AS usuario_rol
          ON usuario_rol.usuario_id = usuario.id
         AND tstzrange(usuario_rol.vigente_desde, usuario_rol.vigente_hasta, '[)') @> statement_timestamp()
        LEFT JOIN public.roles AS rol
          ON rol.id = usuario_rol.rol_id
         AND rol.organizacion_id = usuario.organizacion_id
         AND rol.activo
        WHERE usuario.organizacion_id = $1
        ORDER BY usuario.nombre_completo, usuario.id, rol.nombre
        "#,
    )
    .bind(organization_id)
    .bind(SUPABASE_PROVIDER)
    .fetch_all(db)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;

    let mut subjects: Vec<Uuid> = rows.iter().filter_map(|row| row.3).collect();
    subjects.sort_unstable();
    subjects.dedup();
    let (correos, enrichment_available) = match external_admin
        .correos_electronicos_por_sujeto(&subjects)
        .await
    {
        Ok(correos) => (correos, true),
        Err(_) => {
            warn_email_enrichment_unavailable(subjects.len());
            (HashMap::new(), false)
        }
    };

    let mut users: Vec<UserSummary> = Vec::new();
    let mut users_without_external_identity = 0usize;
    let mut users_without_resolved_email = 0usize;
    for (id, nombre_completo, activo, subject, role_id, role_name) in rows {
        if users.last().is_none_or(|user| user.id != id) {
            let correo_electronico = correo_electronico_actual(&correos, subject);
            if subject.is_none() {
                users_without_external_identity += 1;
            } else if enrichment_available && correo_electronico.is_none() {
                users_without_resolved_email += 1;
            }
            users.push(UserSummary {
                id,
                nombre_completo,
                correo_electronico,
                activo,
                roles: Vec::new(),
            });
        }
        if let (Some(role_id), Some(role_name)) = (role_id, role_name) {
            users
                .last_mut()
                .expect("the current user was inserted")
                .roles
                .push(RoleReference {
                    id: role_id,
                    nombre: role_name,
                });
        }
    }

    if users_without_external_identity > 0 || users_without_resolved_email > 0 {
        warn_email_enrichment_incomplete(
            users_without_external_identity,
            users_without_resolved_email,
        );
    }

    Ok(UsersResponse { usuarios: users })
}

pub async fn list_roles(
    db: &PgPool,
    context: &AuthorizationContext,
) -> Result<RolesResponse, AccessAdministrationError> {
    list_roles_for_organization(db, context.organization_id).await
}

async fn list_roles_for_organization(
    db: &PgPool,
    organization_id: Uuid,
) -> Result<RolesResponse, AccessAdministrationError> {
    let role_rows: Vec<RoleWithCurrentPermissionRow> = sqlx::query_as(
        r#"
        SELECT rol.id, rol.nombre, rol.descripcion, rol.activo, permiso.codigo
        FROM public.roles AS rol
        LEFT JOIN public.roles_permisos AS rol_permiso
          ON rol_permiso.rol_id = rol.id
         AND tstzrange(rol_permiso.vigente_desde, rol_permiso.vigente_hasta, '[)') @> statement_timestamp()
        LEFT JOIN public.permisos AS permiso
          ON permiso.id = rol_permiso.permiso_id
         AND permiso.activo
        WHERE rol.organizacion_id = $1
        ORDER BY rol.nombre, rol.id, permiso.codigo
        "#,
    )
    .bind(organization_id)
    .fetch_all(db)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;

    let mut roles: Vec<RoleSummary> = Vec::new();
    for (id, nombre, descripcion, activo, permission) in role_rows {
        if roles.last().is_none_or(|role| role.id != id) {
            roles.push(RoleSummary {
                id,
                nombre,
                descripcion,
                activo,
                permisos: Vec::new(),
            });
        }
        if let Some(permission) = permission {
            roles
                .last_mut()
                .expect("the current role was inserted")
                .permisos
                .push(permission);
        }
    }

    let permisos = sqlx::query_as::<_, (String, String)>(
        "SELECT codigo, nombre FROM public.permisos WHERE activo ORDER BY codigo",
    )
    .fetch_all(db)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?
    .into_iter()
    .map(|(codigo, nombre)| PermissionSummary { codigo, nombre })
    .collect();

    Ok(RolesResponse { roles, permisos })
}

pub async fn create_or_enable_user(
    db: &PgPool,
    external_admin: &dyn ExternalIdentityAdmin,
    context: &AuthorizationContext,
    request: &CreateUserRequest,
) -> Result<UserSummary, AccessAdministrationError> {
    let email = normalized_email(&request.correo_electronico)?;
    let full_name = normalized_required(&request.nombre_completo)?;
    let roles = distinct_ids(&request.roles_ids);
    let external_user = external_admin
        .resolve_or_invite(email, full_name)
        .await
        .map_err(map_external_error)?;

    let mut transaction = db
        .begin()
        .await
        .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    validate_roles(&mut transaction, context.organization_id, &roles).await?;

    let existing: Option<Uuid> = sqlx::query_scalar(
        r#"
        SELECT identidad.id
        FROM public.identidades_autenticacion_externas AS identidad
        WHERE identidad.proveedor = $1 AND identidad.sujeto_proveedor = $2
        FOR UPDATE
        "#,
    )
    .bind(SUPABASE_PROVIDER)
    .bind(external_user.subject)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;

    if existing.is_some() {
        return Err(AccessAdministrationError::Conflict);
    }

    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
    )
    .bind(context.organization_id)
    .bind(full_name)
    .fetch_one(&mut *transaction)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    sqlx::query(
        "INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(SUPABASE_PROVIDER)
    .bind(external_user.subject)
    .execute(&mut *transaction)
    .await
    .map_err(map_write_error)?;

    synchronize_user_roles(&mut transaction, user_id, &roles).await?;
    transaction
        .commit()
        .await
        .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    load_user(db, external_admin, context.organization_id, user_id).await
}

pub async fn update_user(
    db: &PgPool,
    external_admin: &dyn ExternalIdentityAdmin,
    context: &AuthorizationContext,
    user_id: Uuid,
    request: &UpdateUserRequest,
) -> Result<UserSummary, AccessAdministrationError> {
    if request.nombre_completo.is_none() && request.activo.is_none() && request.roles_ids.is_none()
    {
        return Err(AccessAdministrationError::InvalidInput);
    }
    let full_name = request
        .nombre_completo
        .as_deref()
        .map(normalized_required)
        .transpose()?;
    let roles = request.roles_ids.as_ref().map(|roles| distinct_ids(roles));
    let mut transaction = db
        .begin()
        .await
        .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    let may_reduce_administrators = request.activo == Some(false) || roles.is_some();
    if may_reduce_administrators {
        lock_organization_administration(&mut transaction, context.organization_id)
            .await
            .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    }

    let user_exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM public.usuarios WHERE id = $1 AND organizacion_id = $2 FOR UPDATE",
    )
    .bind(user_id)
    .bind(context.organization_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    if user_exists.is_none() {
        return Err(AccessAdministrationError::NotFound);
    }

    if let Some(full_name) = full_name {
        sqlx::query("UPDATE public.usuarios SET nombre_completo = $1 WHERE id = $2")
            .bind(full_name)
            .bind(user_id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    }
    if let Some(active) = request.activo {
        sqlx::query("UPDATE public.usuarios SET activo = $1 WHERE id = $2")
            .bind(active)
            .bind(user_id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    }
    if let Some(roles) = roles.as_ref() {
        validate_roles(&mut transaction, context.organization_id, roles).await?;
        synchronize_user_roles(&mut transaction, user_id, roles).await?;
    }
    if may_reduce_administrators {
        ensure_organization_has_effective_administrator(&mut transaction, context.organization_id)
            .await?;
    }

    transaction
        .commit()
        .await
        .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    load_user(db, external_admin, context.organization_id, user_id).await
}

pub async fn create_role(
    db: &PgPool,
    context: &AuthorizationContext,
    request: &CreateRoleRequest,
) -> Result<RoleSummary, AccessAdministrationError> {
    let name = normalized_required(&request.nombre)?;
    let description = normalized_optional(request.descripcion.as_deref())?;
    let permissions = distinct_strings(&request.permisos);
    let mut transaction = db
        .begin()
        .await
        .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    let permission_ids = validate_permissions(&mut transaction, &permissions).await?;
    let role_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.roles (organizacion_id, nombre, descripcion) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(context.organization_id)
    .bind(name)
    .bind(description)
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_write_error)?;
    synchronize_role_permissions(&mut transaction, role_id, &permission_ids).await?;
    transaction
        .commit()
        .await
        .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    load_role(db, context.organization_id, role_id).await
}

pub async fn update_role(
    db: &PgPool,
    context: &AuthorizationContext,
    role_id: Uuid,
    request: &UpdateRoleRequest,
) -> Result<RoleSummary, AccessAdministrationError> {
    if request.nombre.is_none()
        && request.descripcion.is_none()
        && request.activo.is_none()
        && request.permisos.is_none()
    {
        return Err(AccessAdministrationError::InvalidInput);
    }
    let name = request
        .nombre
        .as_deref()
        .map(normalized_required)
        .transpose()?;
    let description = request
        .descripcion
        .as_ref()
        .map(|value| normalized_optional(value.as_deref()))
        .transpose()?;
    let permissions = request
        .permisos
        .as_ref()
        .map(|permissions| distinct_strings(permissions));
    let mut transaction = db
        .begin()
        .await
        .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    let may_reduce_administrators = request.activo == Some(false) || permissions.is_some();
    if may_reduce_administrators {
        lock_organization_administration(&mut transaction, context.organization_id)
            .await
            .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    }

    let role_exists: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM public.roles WHERE id = $1 AND organizacion_id = $2 FOR UPDATE",
    )
    .bind(role_id)
    .bind(context.organization_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    if role_exists.is_none() {
        return Err(AccessAdministrationError::NotFound);
    }

    if let Some(name) = name {
        sqlx::query("UPDATE public.roles SET nombre = $1 WHERE id = $2")
            .bind(name)
            .bind(role_id)
            .execute(&mut *transaction)
            .await
            .map_err(map_write_error)?;
    }
    if let Some(description) = description {
        sqlx::query("UPDATE public.roles SET descripcion = $1 WHERE id = $2")
            .bind(description)
            .bind(role_id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    }
    if let Some(active) = request.activo {
        sqlx::query("UPDATE public.roles SET activo = $1 WHERE id = $2")
            .bind(active)
            .bind(role_id)
            .execute(&mut *transaction)
            .await
            .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    }
    if let Some(permissions) = permissions.as_ref() {
        let permission_ids = validate_permissions(&mut transaction, permissions).await?;
        synchronize_role_permissions(&mut transaction, role_id, &permission_ids).await?;
    }
    if may_reduce_administrators {
        ensure_organization_has_effective_administrator(&mut transaction, context.organization_id)
            .await?;
    }

    transaction
        .commit()
        .await
        .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    load_role(db, context.organization_id, role_id).await
}

fn normalized_required(value: &str) -> Result<&str, AccessAdministrationError> {
    let value = value.trim();
    (!value.is_empty())
        .then_some(value)
        .ok_or(AccessAdministrationError::InvalidInput)
}

fn normalized_optional(value: Option<&str>) -> Result<Option<&str>, AccessAdministrationError> {
    value.map(normalized_required).transpose()
}

fn normalized_email(value: &str) -> Result<&str, AccessAdministrationError> {
    let value = normalized_required(value)?;
    (value.contains('@') && !value.chars().any(char::is_whitespace))
        .then_some(value)
        .ok_or(AccessAdministrationError::InvalidInput)
}

fn distinct_ids(values: &[Uuid]) -> Vec<Uuid> {
    let mut values = values.to_vec();
    values.sort_unstable();
    values.dedup();
    values
}

fn distinct_strings(values: &[String]) -> Vec<String> {
    let mut values: Vec<String> = values.iter().map(|value| value.trim().to_owned()).collect();
    values.sort();
    values.dedup();
    values
}

fn map_external_error(error: ExternalIdentityAdminError) -> AccessAdministrationError {
    match error {
        ExternalIdentityAdminError::Rejected => AccessAdministrationError::ExternalRejected,
        ExternalIdentityAdminError::AmbiguousIdentity => AccessAdministrationError::Conflict,
        ExternalIdentityAdminError::Unavailable => AccessAdministrationError::ExternalUnavailable,
    }
}

fn correo_electronico_actual(
    correos: &HashMap<Uuid, String>,
    subject: Option<Uuid>,
) -> Option<String> {
    subject
        .and_then(|subject| correos.get(&subject))
        .filter(|correo_electronico| !correo_electronico.trim().is_empty())
        .cloned()
}

fn warn_email_enrichment_unavailable(requested_subject_count: usize) {
    warn!(
        category = "supabase_email_enrichment_unavailable",
        requested_subject_count,
        "Supabase email enrichment is unavailable; returning users without unresolved emails"
    );
}

fn warn_email_enrichment_incomplete(
    users_without_external_identity: usize,
    users_without_resolved_email: usize,
) {
    warn!(
        category = "supabase_email_enrichment_incomplete",
        users_without_external_identity,
        users_without_resolved_email,
        "Some users do not have a display email available"
    );
}

fn map_write_error(error: sqlx::Error) -> AccessAdministrationError {
    match error.as_database_error().and_then(|error| error.code()) {
        Some(code) if matches!(code.as_ref(), "23505" | "23P01" | "P0001") => {
            AccessAdministrationError::Conflict
        }
        _ => AccessAdministrationError::DatabaseUnavailable,
    }
}

async fn validate_roles(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    role_ids: &[Uuid],
) -> Result<(), AccessAdministrationError> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.roles WHERE organizacion_id = $1 AND activo AND id = ANY($2)",
    )
    .bind(organization_id)
    .bind(role_ids)
    .fetch_one(&mut **transaction)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    if count != role_ids.len() as i64 {
        return Err(AccessAdministrationError::Conflict);
    }
    Ok(())
}

async fn validate_permissions(
    transaction: &mut Transaction<'_, Postgres>,
    permission_codes: &[String],
) -> Result<Vec<Uuid>, AccessAdministrationError> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM public.permisos WHERE activo AND codigo = ANY($1) ORDER BY id",
    )
    .bind(permission_codes)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    if ids.len() != permission_codes.len() {
        return Err(AccessAdministrationError::Conflict);
    }
    Ok(ids)
}

async fn synchronize_user_roles(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    desired: &[Uuid],
) -> Result<(), AccessAdministrationError> {
    let current: Vec<Uuid> = sqlx::query_scalar(
        "SELECT rol_id FROM public.usuarios_roles WHERE usuario_id = $1 AND tstzrange(vigente_desde, vigente_hasta, '[)') @> statement_timestamp() FOR UPDATE",
    )
    .bind(user_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;

    sqlx::query(
        "UPDATE public.usuarios_roles SET vigente_hasta = statement_timestamp() WHERE usuario_id = $1 AND tstzrange(vigente_desde, vigente_hasta, '[)') @> statement_timestamp() AND NOT (rol_id = ANY($2))",
    )
    .bind(user_id)
    .bind(desired)
    .execute(&mut **transaction)
    .await
    .map_err(map_write_error)?;

    for role_id in desired.iter().filter(|role_id| !current.contains(role_id)) {
        sqlx::query("INSERT INTO public.usuarios_roles (usuario_id, rol_id) VALUES ($1, $2)")
            .bind(user_id)
            .bind(role_id)
            .execute(&mut **transaction)
            .await
            .map_err(map_write_error)?;
    }
    Ok(())
}

async fn synchronize_role_permissions(
    transaction: &mut Transaction<'_, Postgres>,
    role_id: Uuid,
    desired: &[Uuid],
) -> Result<(), AccessAdministrationError> {
    let current: Vec<Uuid> = sqlx::query_scalar(
        "SELECT permiso_id FROM public.roles_permisos WHERE rol_id = $1 AND tstzrange(vigente_desde, vigente_hasta, '[)') @> statement_timestamp() FOR UPDATE",
    )
    .bind(role_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;

    sqlx::query(
        "UPDATE public.roles_permisos SET vigente_hasta = statement_timestamp() WHERE rol_id = $1 AND tstzrange(vigente_desde, vigente_hasta, '[)') @> statement_timestamp() AND NOT (permiso_id = ANY($2))",
    )
    .bind(role_id)
    .bind(desired)
    .execute(&mut **transaction)
    .await
    .map_err(map_write_error)?;

    for permission_id in desired
        .iter()
        .filter(|permission_id| !current.contains(permission_id))
    {
        sqlx::query("INSERT INTO public.roles_permisos (rol_id, permiso_id) VALUES ($1, $2)")
            .bind(role_id)
            .bind(permission_id)
            .execute(&mut **transaction)
            .await
            .map_err(map_write_error)?;
    }
    Ok(())
}

pub(crate) async fn lock_organization_administration(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "SELECT pg_advisory_xact_lock(hashtextextended('agro_ops:access-administration:' || $1::text, 0::bigint))",
    )
    .bind(organization_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn organization_has_effective_administrator(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM public.usuarios AS usuario
            JOIN public.organizaciones AS organizacion
              ON organizacion.id = usuario.organizacion_id
             AND organizacion.activa
            JOIN public.identidades_autenticacion_externas AS identidad
              ON identidad.usuario_id = usuario.id
             AND identidad.proveedor = $3
             AND tstzrange(identidad.vinculada_en, identidad.desvinculada_en, '[)')
                 @> statement_timestamp()
            JOIN public.usuarios_roles AS usuario_rol
              ON usuario_rol.usuario_id = usuario.id
             AND tstzrange(usuario_rol.vigente_desde, usuario_rol.vigente_hasta, '[)')
                 @> statement_timestamp()
            JOIN public.roles AS rol
              ON rol.id = usuario_rol.rol_id
             AND rol.organizacion_id = usuario.organizacion_id
             AND rol.activo
            JOIN public.roles_permisos AS rol_permiso
              ON rol_permiso.rol_id = rol.id
             AND tstzrange(rol_permiso.vigente_desde, rol_permiso.vigente_hasta, '[)')
                 @> statement_timestamp()
            JOIN public.permisos AS permiso
              ON permiso.id = rol_permiso.permiso_id
             AND permiso.activo
             AND permiso.codigo = $2
            WHERE usuario.organizacion_id = $1
              AND usuario.activo
        )
        "#,
    )
    .bind(organization_id)
    .bind(CONFIGURACION_ADMINISTRAR)
    .bind(SUPABASE_PROVIDER)
    .fetch_one(&mut **transaction)
    .await
}

async fn ensure_organization_has_effective_administrator(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
) -> Result<(), AccessAdministrationError> {
    let remains = organization_has_effective_administrator(transaction, organization_id)
        .await
        .map_err(|_| AccessAdministrationError::DatabaseUnavailable)?;
    remains
        .then_some(())
        .ok_or(AccessAdministrationError::Conflict)
}

async fn load_user(
    db: &PgPool,
    external_admin: &dyn ExternalIdentityAdmin,
    organization_id: Uuid,
    user_id: Uuid,
) -> Result<UserSummary, AccessAdministrationError> {
    list_users_for_organization(db, external_admin, organization_id)
        .await?
        .usuarios
        .into_iter()
        .find(|user| user.id == user_id)
        .ok_or(AccessAdministrationError::NotFound)
}

async fn load_role(
    db: &PgPool,
    organization_id: Uuid,
    role_id: Uuid,
) -> Result<RoleSummary, AccessAdministrationError> {
    list_roles_for_organization(db, organization_id)
        .await?
        .roles
        .into_iter()
        .find(|role| role.id == role_id)
        .ok_or(AccessAdministrationError::NotFound)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };

    use super::{warn_email_enrichment_incomplete, warn_email_enrichment_unavailable};

    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("diagnostic buffer lock must be available")
                .extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn email_enrichment_diagnostics_only_include_sanitized_categories_and_counts() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let writer_output = Arc::clone(&output);
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_target(false)
            .with_writer(move || SharedWriter(Arc::clone(&writer_output)))
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        warn_email_enrichment_unavailable(3);
        warn_email_enrichment_incomplete(1, 2);

        let diagnostics = String::from_utf8(
            output
                .lock()
                .expect("diagnostic buffer lock must be available")
                .clone(),
        )
        .expect("diagnostics must be UTF-8");
        assert!(diagnostics.contains("supabase_email_enrichment_unavailable"));
        assert!(diagnostics.contains("supabase_email_enrichment_incomplete"));
        assert!(diagnostics.contains("requested_subject_count=3"));
        assert!(diagnostics.contains("users_without_external_identity=1"));
        assert!(diagnostics.contains("users_without_resolved_email=2"));
        for forbidden in [
            "user@example.com",
            "00000000-0000-4000-8000-000000000001",
            "bearer-token",
            "secret-key",
            "{\"users\":[]}",
        ] {
            assert!(!diagnostics.contains(forbidden));
        }
    }
}
