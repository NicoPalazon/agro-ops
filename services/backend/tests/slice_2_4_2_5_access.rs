use std::sync::Arc;

use agro_ops_backend::{
    access_administration::{
        self, AccessAdministrationError, CreateRoleRequest, CreateUserRequest, UpdateRoleRequest,
        UpdateUserRequest,
    },
    authorization::{self, AuthorizationContext, permission_codes},
    supabase_admin::{ExternalAuthUser, ExternalIdentityAdmin, ExternalIdentityAdminError},
};
use async_trait::async_trait;
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

#[derive(Clone)]
struct MockExternalAdmin {
    result: Result<ExternalAuthUser, ExternalIdentityAdminError>,
}

#[async_trait]
impl ExternalIdentityAdmin for MockExternalAdmin {
    async fn resolve_or_invite(
        &self,
        _email: &str,
        _full_name: &str,
    ) -> Result<ExternalAuthUser, ExternalIdentityAdminError> {
        self.result.clone()
    }
}

async fn database() -> PgPool {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests"))
        .await
        .expect("PostgreSQL must be available for integration tests")
}

struct AdminFixture {
    db: PgPool,
    context: AuthorizationContext,
    organization_id: Uuid,
}

async fn admin_fixture() -> AdminFixture {
    let db = database().await;
    let organization_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let subject = Uuid::new_v4();
    let role_id = Uuid::new_v4();
    let tag = Uuid::new_v4();

    sqlx::query("INSERT INTO public.organizaciones (id, nombre) VALUES ($1, $2)")
        .bind(organization_id)
        .bind(format!("Organización administración {tag}"))
        .execute(&db)
        .await
        .expect("organization fixture must insert");
    sqlx::query(
        "INSERT INTO public.usuarios (id, organizacion_id, nombre_completo) VALUES ($1, $2, 'Administrador de prueba')",
    )
    .bind(user_id)
    .bind(organization_id)
    .execute(&db)
    .await
    .expect("admin fixture must insert");
    sqlx::query(
        "INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, 'supabase', $2)",
    )
    .bind(user_id)
    .bind(subject)
    .execute(&db)
    .await
    .expect("identity fixture must insert");
    sqlx::query("INSERT INTO public.roles (id, organizacion_id, nombre) VALUES ($1, $2, $3)")
        .bind(role_id)
        .bind(organization_id)
        .bind(format!("Administrador {tag}"))
        .execute(&db)
        .await
        .expect("admin role fixture must insert");
    sqlx::query("INSERT INTO public.usuarios_roles (usuario_id, rol_id) VALUES ($1, $2)")
        .bind(user_id)
        .bind(role_id)
        .execute(&db)
        .await
        .expect("admin role assignment fixture must insert");
    sqlx::query(
        "INSERT INTO public.roles_permisos (rol_id, permiso_id) SELECT $1, id FROM public.permisos WHERE codigo = $2",
    )
    .bind(role_id)
    .bind(permission_codes::CONFIGURACION_ADMINISTRAR)
    .execute(&db)
    .await
    .expect("admin permission fixture must insert");

    let context = authorization::resolve_context(&db, subject)
        .await
        .expect("admin context must resolve");
    AdminFixture {
        db,
        context,
        organization_id,
    }
}

async fn role_with_permissions(fixture: &AdminFixture, name: &str, permissions: &[&str]) -> Uuid {
    let role_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.roles (organizacion_id, nombre) VALUES ($1, $2) RETURNING id",
    )
    .bind(fixture.organization_id)
    .bind(format!("{name} {}", Uuid::new_v4()))
    .fetch_one(&fixture.db)
    .await
    .expect("role fixture must insert");
    for permission in permissions {
        sqlx::query(
            "INSERT INTO public.roles_permisos (rol_id, permiso_id) SELECT $1, id FROM public.permisos WHERE codigo = $2",
        )
        .bind(role_id)
        .bind(permission)
        .execute(&fixture.db)
        .await
        .expect("role permission fixture must insert");
    }
    role_id
}

fn successful_external_admin(subject: Uuid) -> Arc<MockExternalAdmin> {
    Arc::new(MockExternalAdmin {
        result: Ok(ExternalAuthUser { subject }),
    })
}

#[tokio::test]
async fn administrator_links_supabase_subject_and_assigns_roles_transactionally() {
    let fixture = admin_fixture().await;
    let target_role = role_with_permissions(
        &fixture,
        "Técnico",
        &[permission_codes::CONSOLA_TECNICA_VER],
    )
    .await;
    let subject = Uuid::new_v4();

    let user = access_administration::create_or_enable_user(
        &fixture.db,
        successful_external_admin(subject).as_ref(),
        &fixture.context,
        &CreateUserRequest {
            correo_electronico: "operador@example.com".to_owned(),
            nombre_completo: "Operador de prueba".to_owned(),
            roles_ids: vec![target_role],
        },
    )
    .await
    .expect("administrator must create an Agro Ops user");

    assert!(user.activo);
    assert_eq!(user.roles.len(), 1);
    let linked_user: Uuid = sqlx::query_scalar(
        "SELECT usuario_id FROM public.identidades_autenticacion_externas WHERE proveedor = 'supabase' AND sujeto_proveedor = $1 AND desvinculada_en IS NULL",
    )
    .bind(subject)
    .fetch_one(&fixture.db)
    .await
    .expect("stable Supabase subject must be linked");
    assert_eq!(linked_user, user.id);
    let context = authorization::resolve_context(&fixture.db, subject)
        .await
        .expect("newly provisioned user must authorize");
    assert!(context.has_permission(permission_codes::CONSOLA_TECNICA_VER));
}

#[tokio::test]
async fn external_success_followed_by_internal_failure_grants_no_agro_ops_access() {
    let fixture = admin_fixture().await;
    let other_organization: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(format!("Otra organización {}", Uuid::new_v4()))
            .fetch_one(&fixture.db)
            .await
            .expect("other organization must insert");
    let foreign_role: Uuid = sqlx::query_scalar(
        "INSERT INTO public.roles (organizacion_id, nombre) VALUES ($1, 'Rol ajeno') RETURNING id",
    )
    .bind(other_organization)
    .fetch_one(&fixture.db)
    .await
    .expect("foreign role must insert");
    let subject = Uuid::new_v4();

    let error = access_administration::create_or_enable_user(
        &fixture.db,
        successful_external_admin(subject).as_ref(),
        &fixture.context,
        &CreateUserRequest {
            correo_electronico: "sin-acceso@example.com".to_owned(),
            nombre_completo: "Sin acceso".to_owned(),
            roles_ids: vec![foreign_role],
        },
    )
    .await
    .expect_err("cross-organization role must fail the internal transaction");

    assert_eq!(error, AccessAdministrationError::Conflict);
    let identity_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.identidades_autenticacion_externas WHERE sujeto_proveedor = $1",
    )
    .bind(subject)
    .fetch_one(&fixture.db)
    .await
    .expect("identity count must be queryable");
    assert_eq!(identity_count, 0);
    assert!(matches!(
        authorization::resolve_context(&fixture.db, subject).await,
        Err(authorization::ResolveAuthorizationError::PrincipalUnavailable)
    ));
}

#[tokio::test]
async fn supabase_admin_failure_creates_no_internal_user() {
    let fixture = admin_fixture().await;
    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.usuarios WHERE organizacion_id = $1",
    )
    .bind(fixture.organization_id)
    .fetch_one(&fixture.db)
    .await
    .expect("user count must be queryable");
    let external = MockExternalAdmin {
        result: Err(ExternalIdentityAdminError::Unavailable),
    };

    let error = access_administration::create_or_enable_user(
        &fixture.db,
        &external,
        &fixture.context,
        &CreateUserRequest {
            correo_electronico: "fallo@example.com".to_owned(),
            nombre_completo: "Falla externa".to_owned(),
            roles_ids: vec![],
        },
    )
    .await
    .expect_err("Supabase failure must fail closed");
    assert_eq!(error, AccessAdministrationError::ExternalUnavailable);

    let after: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.usuarios WHERE organizacion_id = $1",
    )
    .bind(fixture.organization_id)
    .fetch_one(&fixture.db)
    .await
    .expect("user count must be queryable");
    assert_eq!(after, before);
}

#[tokio::test]
async fn retry_reuses_subject_without_creating_or_relinking_a_second_user() {
    let fixture = admin_fixture().await;
    let subject = Uuid::new_v4();
    let request = CreateUserRequest {
        correo_electronico: "reintento@example.com".to_owned(),
        nombre_completo: "Usuario reintentable".to_owned(),
        roles_ids: vec![],
    };
    let external = successful_external_admin(subject);

    let first = access_administration::create_or_enable_user(
        &fixture.db,
        external.as_ref(),
        &fixture.context,
        &request,
    )
    .await
    .expect("first attempt must succeed");
    let second = access_administration::create_or_enable_user(
        &fixture.db,
        external.as_ref(),
        &fixture.context,
        &request,
    )
    .await
    .expect("retry must succeed");

    assert_eq!(first.id, second.id);
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.identidades_autenticacion_externas WHERE proveedor = 'supabase' AND sujeto_proveedor = $1",
    )
    .bind(subject)
    .fetch_one(&fixture.db)
    .await
    .expect("identity count must be queryable");
    assert_eq!(count, 1);
}

#[tokio::test]
async fn subject_linked_to_another_organization_is_never_relinked() {
    let fixture = admin_fixture().await;
    let subject = Uuid::new_v4();
    let other_organization: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(format!("Organización propietaria {}", Uuid::new_v4()))
            .fetch_one(&fixture.db)
            .await
            .expect("other organization must insert");
    let owner: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1, 'Dueño original') RETURNING id",
    )
    .bind(other_organization)
    .fetch_one(&fixture.db)
    .await
    .expect("owner user must insert");
    sqlx::query(
        "INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, 'supabase', $2)",
    )
    .bind(owner)
    .bind(subject)
    .execute(&fixture.db)
    .await
    .expect("existing identity must insert");

    let error = access_administration::create_or_enable_user(
        &fixture.db,
        successful_external_admin(subject).as_ref(),
        &fixture.context,
        &CreateUserRequest {
            correo_electronico: "vinculado@example.com".to_owned(),
            nombre_completo: "Otro usuario".to_owned(),
            roles_ids: vec![],
        },
    )
    .await
    .expect_err("an existing subject from another organization must conflict");

    assert_eq!(error, AccessAdministrationError::Conflict);
    let still_linked_to: Uuid = sqlx::query_scalar(
        "SELECT usuario_id FROM public.identidades_autenticacion_externas WHERE proveedor = 'supabase' AND sujeto_proveedor = $1",
    )
    .bind(subject)
    .fetch_one(&fixture.db)
    .await
    .expect("existing identity must remain queryable");
    assert_eq!(still_linked_to, owner);
}

#[tokio::test]
async fn disabling_user_and_changing_roles_affect_the_next_authorization_request() {
    let fixture = admin_fixture().await;
    let technical_role = role_with_permissions(
        &fixture,
        "Técnico",
        &[permission_codes::CONSOLA_TECNICA_VER],
    )
    .await;
    let subject = Uuid::new_v4();
    let user = access_administration::create_or_enable_user(
        &fixture.db,
        successful_external_admin(subject).as_ref(),
        &fixture.context,
        &CreateUserRequest {
            correo_electronico: "cambio@example.com".to_owned(),
            nombre_completo: "Usuario cambiante".to_owned(),
            roles_ids: vec![technical_role],
        },
    )
    .await
    .expect("user must provision");

    access_administration::update_user(
        &fixture.db,
        &fixture.context,
        user.id,
        &UpdateUserRequest {
            nombre_completo: None,
            activo: None,
            roles_ids: Some(vec![]),
        },
    )
    .await
    .expect("role removal must succeed");
    let context = authorization::resolve_context(&fixture.db, subject)
        .await
        .expect("enabled user still resolves");
    assert!(!context.has_permission(permission_codes::CONSOLA_TECNICA_VER));
    let history: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COUNT(vigente_hasta)::bigint FROM public.usuarios_roles WHERE usuario_id = $1 AND rol_id = $2",
    )
    .bind(user.id)
    .bind(technical_role)
    .fetch_one(&fixture.db)
    .await
    .expect("role history must be queryable");
    assert_eq!(history, (1, 1));

    access_administration::update_user(
        &fixture.db,
        &fixture.context,
        user.id,
        &UpdateUserRequest {
            nombre_completo: None,
            activo: Some(false),
            roles_ids: None,
        },
    )
    .await
    .expect("deactivation must succeed");
    assert!(matches!(
        authorization::resolve_context(&fixture.db, subject).await,
        Err(authorization::ResolveAuthorizationError::PrincipalUnavailable)
    ));
}

#[tokio::test]
async fn role_permission_removal_closes_history_without_deleting_it() {
    let fixture = admin_fixture().await;
    let role = access_administration::create_role(
        &fixture.db,
        &fixture.context,
        &CreateRoleRequest {
            nombre: format!("Encargado {}", Uuid::new_v4()),
            descripcion: None,
            permisos: vec![permission_codes::CONSOLA_TECNICA_VER.to_owned()],
        },
    )
    .await
    .expect("role must create");

    access_administration::update_role(
        &fixture.db,
        &fixture.context,
        role.id,
        &UpdateRoleRequest {
            nombre: None,
            descripcion: None,
            activo: None,
            permisos: Some(vec![]),
        },
    )
    .await
    .expect("permission removal must succeed");

    let history: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COUNT(vigente_hasta)::bigint FROM public.roles_permisos WHERE rol_id = $1",
    )
    .bind(role.id)
    .fetch_one(&fixture.db)
    .await
    .expect("permission history must be queryable");
    assert_eq!(history, (1, 1));
}
