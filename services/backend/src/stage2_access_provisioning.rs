use std::{env, fmt};

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::authorization::{
    SUPABASE_PROVIDER,
    permission_codes::{CONFIGURACION_ADMINISTRAR, CONSOLA_TECNICA_VER},
};

pub const TECHNICAL_ROLE_NAME: &str = "Tecnico";
pub const INITIAL_ADMIN_ROLE_NAME: &str = "Administrador inicial";

/// Runtime values for the one-shot Stage 2 access bootstrap.
///
/// These are deliberately limited to values represented in the Slice 2.1
/// schema. In particular, an external subject, rather than an email address,
/// identifies the authenticated principal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvisionStage2AccessRequest {
    pub supabase_subject: Uuid,
    pub organization_name: String,
    pub user_full_name: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProvisionStage2AccessConfig {
    database_url: String,
    request: ProvisionStage2AccessRequest,
}

impl ProvisionStage2AccessConfig {
    pub fn from_env() -> Result<Self, ProvisionStage2AccessConfigError> {
        let database_url = required_env("DATABASE_URL")?;
        let subject = required_env("AGRO_OPS_PROVISION_SUPABASE_SUBJECT")?;
        let supabase_subject = Uuid::parse_str(&subject).map_err(|_| {
            ProvisionStage2AccessConfigError::InvalidSupabaseSubject(
                "AGRO_OPS_PROVISION_SUPABASE_SUBJECT must be a UUID".to_owned(),
            )
        })?;

        Ok(Self {
            database_url,
            request: ProvisionStage2AccessRequest {
                supabase_subject,
                organization_name: required_env("AGRO_OPS_PROVISION_ORGANIZATION_NAME")?,
                user_full_name: required_env("AGRO_OPS_PROVISION_USER_FULL_NAME")?,
            },
        })
    }

    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    pub fn request(&self) -> &ProvisionStage2AccessRequest {
        &self.request
    }
}

impl fmt::Debug for ProvisionStage2AccessConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProvisionStage2AccessConfig")
            .field("database_url", &"[REDACTED]")
            .field("request", &self.request)
            .finish()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProvisionStage2AccessConfigError {
    MissingEnvironmentVariable(String),
    InvalidSupabaseSubject(String),
}

impl fmt::Display for ProvisionStage2AccessConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnvironmentVariable(name) => {
                write!(
                    formatter,
                    "invalid provisioning configuration: {name} is required"
                )
            }
            Self::InvalidSupabaseSubject(message) => {
                write!(formatter, "invalid provisioning configuration: {message}")
            }
        }
    }
}

impl std::error::Error for ProvisionStage2AccessConfigError {}

fn required_env(name: &str) -> Result<String, ProvisionStage2AccessConfigError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            ProvisionStage2AccessConfigError::MissingEnvironmentVariable(name.to_owned())
        })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvisionStage2AccessResult {
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub supabase_subject: Uuid,
    pub role_name: &'static str,
    pub permission_code: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootstrapStage2AdministratorResult {
    pub organization_id: Uuid,
    pub user_id: Uuid,
    pub supabase_subject: Uuid,
    pub role_name: &'static str,
    pub permission_codes: [&'static str; 2],
}

#[derive(Debug)]
pub enum ProvisionStage2AccessError {
    Operational(String),
    Database(sqlx::Error),
}

impl fmt::Display for ProvisionStage2AccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operational(message) => formatter.write_str(message),
            Self::Database(error) => write!(formatter, "database provisioning failed: {error}"),
        }
    }
}

impl std::error::Error for ProvisionStage2AccessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Operational(_) => None,
            Self::Database(error) => Some(error),
        }
    }
}

impl From<sqlx::Error> for ProvisionStage2AccessError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Materialize the minimum Stage 2 authorization path in one transaction.
pub async fn provision_stage2_access(
    db: &PgPool,
    request: &ProvisionStage2AccessRequest,
) -> Result<ProvisionStage2AccessResult, ProvisionStage2AccessError> {
    let mut transaction = db.begin().await?;
    let result = provision_in_transaction(&mut transaction, request).await?;
    transaction.commit().await?;
    Ok(result)
}

/// Materialize the first trusted administrator without making the technical
/// role administrative. This is intentionally a one-shot operational path.
pub async fn bootstrap_stage2_administrator(
    db: &PgPool,
    request: &ProvisionStage2AccessRequest,
) -> Result<BootstrapStage2AdministratorResult, ProvisionStage2AccessError> {
    let mut transaction = db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0::bigint))")
        .bind(&request.organization_name)
        .execute(&mut *transaction)
        .await?;

    let organization_id =
        resolve_organization(&mut transaction, &request.organization_name).await?;
    let user_id = resolve_user_and_identity(&mut transaction, organization_id, request).await?;
    let role_id = resolve_initial_admin_role(&mut transaction, organization_id).await?;
    let configuration_permission_id =
        resolve_permission(&mut transaction, CONFIGURACION_ADMINISTRAR).await?;
    let console_permission_id = resolve_permission(&mut transaction, CONSOLA_TECNICA_VER).await?;

    ensure_current_user_role(&mut transaction, user_id, role_id).await?;
    ensure_current_role_permission(&mut transaction, role_id, configuration_permission_id).await?;
    ensure_current_role_permission(&mut transaction, role_id, console_permission_id).await?;
    verify_exact_permissions(
        &mut transaction,
        role_id,
        &[CONFIGURACION_ADMINISTRAR, CONSOLA_TECNICA_VER],
    )
    .await?;

    transaction.commit().await?;
    Ok(BootstrapStage2AdministratorResult {
        organization_id,
        user_id,
        supabase_subject: request.supabase_subject,
        role_name: INITIAL_ADMIN_ROLE_NAME,
        permission_codes: [CONFIGURACION_ADMINISTRAR, CONSOLA_TECNICA_VER],
    })
}

async fn provision_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    request: &ProvisionStage2AccessRequest,
) -> Result<ProvisionStage2AccessResult, ProvisionStage2AccessError> {
    // `organizaciones.nombre` is the only supplied identifying field in the
    // approved physical schema and is not unique. This narrow transaction lock
    // prevents two executions of this command from creating the same logical
    // organization before either can observe the other.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0::bigint))")
        .bind(&request.organization_name)
        .execute(&mut **transaction)
        .await?;

    let organization_id = resolve_organization(transaction, &request.organization_name).await?;
    let permission_id = resolve_canonical_permission(transaction).await?;
    let role_id = resolve_technical_role(transaction, organization_id).await?;
    let user_id = resolve_user_and_identity(transaction, organization_id, request).await?;

    ensure_current_user_role(transaction, user_id, role_id).await?;
    ensure_current_role_permission(transaction, role_id, permission_id).await?;
    verify_exact_current_role_permissions(transaction, role_id).await?;

    Ok(ProvisionStage2AccessResult {
        organization_id,
        user_id,
        supabase_subject: request.supabase_subject,
        role_name: TECHNICAL_ROLE_NAME,
        permission_code: CONSOLA_TECNICA_VER,
    })
}

async fn resolve_organization(
    transaction: &mut Transaction<'_, Postgres>,
    organization_name: &str,
) -> Result<Uuid, ProvisionStage2AccessError> {
    let organizations: Vec<(Uuid, bool)> =
        sqlx::query_as("SELECT id, activa FROM public.organizaciones WHERE nombre = $1 FOR UPDATE")
            .bind(organization_name)
            .fetch_all(&mut **transaction)
            .await?;

    match organizations.as_slice() {
        [] => Ok(sqlx::query_scalar(
            "INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id",
        )
        .bind(organization_name)
        .fetch_one(&mut **transaction)
        .await?),
        [(organization_id, true)] => Ok(*organization_id),
        [(_, false)] => Err(ProvisionStage2AccessError::Operational(
            "matching organization is inactive; refusing to reactivate it".to_owned(),
        )),
        _ => Err(ProvisionStage2AccessError::Operational(
            "multiple organizations match AGRO_OPS_PROVISION_ORGANIZATION_NAME; manual investigation is required"
                .to_owned(),
        )),
    }
}

async fn resolve_canonical_permission(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Uuid, ProvisionStage2AccessError> {
    let permission: Option<(Uuid, bool)> =
        sqlx::query_as("SELECT id, activo FROM public.permisos WHERE codigo = $1 FOR UPDATE")
            .bind(CONSOLA_TECNICA_VER)
            .fetch_optional(&mut **transaction)
            .await?;

    match permission {
        Some((permission_id, true)) => Ok(permission_id),
        Some((_, false)) => Err(ProvisionStage2AccessError::Operational(
            "canonical permission consola_tecnica:ver is inactive; refusing to provision".to_owned(),
        )),
        None => Err(ProvisionStage2AccessError::Operational(
            "canonical permission consola_tecnica:ver is missing; apply the approved Slice 2.1 migration"
                .to_owned(),
        )),
    }
}

async fn resolve_permission(
    transaction: &mut Transaction<'_, Postgres>,
    permission_code: &'static str,
) -> Result<Uuid, ProvisionStage2AccessError> {
    let permission: Option<(Uuid, bool)> =
        sqlx::query_as("SELECT id, activo FROM public.permisos WHERE codigo = $1 FOR UPDATE")
            .bind(permission_code)
            .fetch_optional(&mut **transaction)
            .await?;

    match permission {
        Some((permission_id, true)) => Ok(permission_id),
        Some((_, false)) => Err(ProvisionStage2AccessError::Operational(format!(
            "canonical permission {permission_code} is inactive; refusing to provision"
        ))),
        None => Err(ProvisionStage2AccessError::Operational(format!(
            "canonical permission {permission_code} is missing; apply the approved Slice 2.1 migration"
        ))),
    }
}

async fn resolve_technical_role(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
) -> Result<Uuid, ProvisionStage2AccessError> {
    let role: Option<(Uuid, bool)> = sqlx::query_as(
        "SELECT id, activo FROM public.roles WHERE organizacion_id = $1 AND nombre = $2 FOR UPDATE",
    )
    .bind(organization_id)
    .bind(TECHNICAL_ROLE_NAME)
    .fetch_optional(&mut **transaction)
    .await?;

    match role {
        Some((role_id, true)) => Ok(role_id),
        Some((_, false)) => Err(ProvisionStage2AccessError::Operational(
            "matching Tecnico role is inactive; refusing to reactivate it".to_owned(),
        )),
        None => Ok(sqlx::query_scalar(
            "INSERT INTO public.roles (organizacion_id, nombre) VALUES ($1, $2) RETURNING id",
        )
        .bind(organization_id)
        .bind(TECHNICAL_ROLE_NAME)
        .fetch_one(&mut **transaction)
        .await?),
    }
}

async fn resolve_initial_admin_role(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
) -> Result<Uuid, ProvisionStage2AccessError> {
    let role: Option<(Uuid, bool)> = sqlx::query_as(
        "SELECT id, activo FROM public.roles WHERE organizacion_id = $1 AND nombre = $2 FOR UPDATE",
    )
    .bind(organization_id)
    .bind(INITIAL_ADMIN_ROLE_NAME)
    .fetch_optional(&mut **transaction)
    .await?;

    match role {
        Some((role_id, true)) => Ok(role_id),
        Some((_, false)) => Err(ProvisionStage2AccessError::Operational(
            "matching Administrador inicial role is inactive; refusing to reactivate it".to_owned(),
        )),
        None => Ok(sqlx::query_scalar(
            "INSERT INTO public.roles (organizacion_id, nombre, descripcion) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(organization_id)
        .bind(INITIAL_ADMIN_ROLE_NAME)
        .bind("Acceso de emergencia para el primer administrador de Agro Ops")
        .fetch_one(&mut **transaction)
        .await?),
    }
}

async fn resolve_user_and_identity(
    transaction: &mut Transaction<'_, Postgres>,
    organization_id: Uuid,
    request: &ProvisionStage2AccessRequest,
) -> Result<Uuid, ProvisionStage2AccessError> {
    let identity: Option<(Uuid, Uuid, bool, bool, bool)> = sqlx::query_as(
        r#"
        SELECT
            usuario.id,
            usuario.organizacion_id,
            usuario.activo,
            organizacion.activa,
            tstzrange(identidad.vinculada_en, identidad.desvinculada_en, '[)')
                @> transaction_timestamp()
        FROM public.identidades_autenticacion_externas AS identidad
        JOIN public.usuarios AS usuario ON usuario.id = identidad.usuario_id
        JOIN public.organizaciones AS organizacion ON organizacion.id = usuario.organizacion_id
        WHERE identidad.proveedor = $1
          AND identidad.sujeto_proveedor = $2
        FOR UPDATE OF identidad, usuario, organizacion
        "#,
    )
    .bind(SUPABASE_PROVIDER)
    .bind(request.supabase_subject)
    .fetch_optional(&mut **transaction)
    .await?;

    match identity {
        Some((_, _, _, _, false)) => Err(ProvisionStage2AccessError::Operational(
            "Supabase subject has a closed or not-yet-active external identity; historical identity links are not reopened"
                .to_owned(),
        )),
        Some((_, identity_organization_id, _, _, _)) if identity_organization_id != organization_id => {
            Err(ProvisionStage2AccessError::Operational(
                "Supabase subject is already linked to a user in a different organization"
                    .to_owned(),
            ))
        }
        Some((_, _, false, _, _)) => Err(ProvisionStage2AccessError::Operational(
            "Supabase subject is linked to an inactive user; refusing to reactivate it".to_owned(),
        )),
        Some((_, _, _, false, _)) => Err(ProvisionStage2AccessError::Operational(
            "Supabase subject is linked to an inactive organization; refusing to reactivate it"
                .to_owned(),
        )),
        Some((user_id, _, true, true, true)) => Ok(user_id),
        None => {
            let user_id: Uuid = sqlx::query_scalar(
                "INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
            )
            .bind(organization_id)
            .bind(&request.user_full_name)
            .fetch_one(&mut **transaction)
            .await?;
            sqlx::query(
                "INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, $2, $3)",
            )
            .bind(user_id)
            .bind(SUPABASE_PROVIDER)
            .bind(request.supabase_subject)
            .execute(&mut **transaction)
            .await?;
            Ok(user_id)
        }
    }
}

async fn ensure_current_user_role(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    role_id: Uuid,
) -> Result<(), ProvisionStage2AccessError> {
    let current: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM public.usuarios_roles WHERE usuario_id = $1 AND rol_id = $2 AND tstzrange(vigente_desde, vigente_hasta, '[)') @> transaction_timestamp() FOR UPDATE",
    )
    .bind(user_id)
    .bind(role_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if current.is_some() {
        return Ok(());
    }

    let future_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM public.usuarios_roles WHERE usuario_id = $1 AND rol_id = $2 AND vigente_desde > transaction_timestamp())",
    )
    .bind(user_id)
    .bind(role_id)
    .fetch_one(&mut **transaction)
    .await?;
    if future_exists {
        return Err(ProvisionStage2AccessError::Operational(
            "a future UsuarioRol episode exists; manual investigation is required before provisioning"
                .to_owned(),
        ));
    }

    sqlx::query("INSERT INTO public.usuarios_roles (usuario_id, rol_id) VALUES ($1, $2)")
        .bind(user_id)
        .bind(role_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn ensure_current_role_permission(
    transaction: &mut Transaction<'_, Postgres>,
    role_id: Uuid,
    permission_id: Uuid,
) -> Result<(), ProvisionStage2AccessError> {
    let current: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM public.roles_permisos WHERE rol_id = $1 AND permiso_id = $2 AND tstzrange(vigente_desde, vigente_hasta, '[)') @> transaction_timestamp() FOR UPDATE",
    )
    .bind(role_id)
    .bind(permission_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if current.is_some() {
        return Ok(());
    }

    let future_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM public.roles_permisos WHERE rol_id = $1 AND permiso_id = $2 AND vigente_desde > transaction_timestamp())",
    )
    .bind(role_id)
    .bind(permission_id)
    .fetch_one(&mut **transaction)
    .await?;
    if future_exists {
        return Err(ProvisionStage2AccessError::Operational(
            "a future RolPermiso episode exists; manual investigation is required before provisioning"
                .to_owned(),
        ));
    }

    sqlx::query("INSERT INTO public.roles_permisos (rol_id, permiso_id) VALUES ($1, $2)")
        .bind(role_id)
        .bind(permission_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn verify_exact_current_role_permissions(
    transaction: &mut Transaction<'_, Postgres>,
    role_id: Uuid,
) -> Result<(), ProvisionStage2AccessError> {
    let permission_codes: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT permiso.codigo
        FROM public.roles_permisos AS rol_permiso
        JOIN public.permisos AS permiso ON permiso.id = rol_permiso.permiso_id
        WHERE rol_permiso.rol_id = $1
          AND tstzrange(rol_permiso.vigente_desde, rol_permiso.vigente_hasta, '[)') @> transaction_timestamp()
        ORDER BY permiso.codigo
        "#,
    )
    .bind(role_id)
    .fetch_all(&mut **transaction)
    .await?;

    if permission_codes.as_slice() != [CONSOLA_TECNICA_VER] {
        return Err(ProvisionStage2AccessError::Operational(
            "matching Tecnico role has permissions outside consola_tecnica:ver; refusing to alter its authorization profile"
                .to_owned(),
        ));
    }
    Ok(())
}

async fn verify_exact_permissions(
    transaction: &mut Transaction<'_, Postgres>,
    role_id: Uuid,
    expected: &[&str],
) -> Result<(), ProvisionStage2AccessError> {
    let permission_codes: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT permiso.codigo
        FROM public.roles_permisos AS rol_permiso
        JOIN public.permisos AS permiso ON permiso.id = rol_permiso.permiso_id
        WHERE rol_permiso.rol_id = $1
          AND tstzrange(rol_permiso.vigente_desde, rol_permiso.vigente_hasta, '[)') @> transaction_timestamp()
        ORDER BY permiso.codigo
        "#,
    )
    .bind(role_id)
    .fetch_all(&mut **transaction)
    .await?;

    if permission_codes
        != expected
            .iter()
            .map(|permission| (*permission).to_owned())
            .collect::<Vec<_>>()
    {
        return Err(ProvisionStage2AccessError::Operational(
            "matching Administrador inicial role has unexpected permissions; refusing to alter its authorization profile"
                .to_owned(),
        ));
    }
    Ok(())
}
