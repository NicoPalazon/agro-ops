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
    administrator_user_id: Uuid,
    administrator_role_id: Uuid,
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
        administrator_user_id: user_id,
        administrator_role_id: role_id,
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

async fn effective_administrator_count(fixture: &AdminFixture) -> i64 {
    sqlx::query_scalar(
        r#"
        SELECT COUNT(DISTINCT usuario.id)::bigint
        FROM public.usuarios AS usuario
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
          AND EXISTS (
              SELECT 1
              FROM public.identidades_autenticacion_externas AS identidad
              WHERE identidad.usuario_id = usuario.id
                AND identidad.proveedor = 'supabase'
                AND tstzrange(identidad.vinculada_en, identidad.desvinculada_en, '[)')
                    @> statement_timestamp()
          )
        "#,
    )
    .bind(fixture.organization_id)
    .bind(permission_codes::CONFIGURACION_ADMINISTRAR)
    .fetch_one(&fixture.db)
    .await
    .expect("effective administrator count must be queryable")
}

async fn create_user_with_roles(
    fixture: &AdminFixture,
    subject: Uuid,
    roles_ids: Vec<Uuid>,
) -> access_administration::UserSummary {
    access_administration::create_or_enable_user(
        &fixture.db,
        successful_external_admin(subject).as_ref(),
        &fixture.context,
        &CreateUserRequest {
            correo_electronico: format!("{subject}@example.com"),
            nombre_completo: format!("Usuario {subject}"),
            roles_ids,
        },
    )
    .await
    .expect("test user must be provisioned")
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

    let retried = access_administration::create_or_enable_user(
        &fixture.db,
        successful_external_admin(subject).as_ref(),
        &fixture.context,
        &CreateUserRequest {
            correo_electronico: "sin-acceso@example.com".to_owned(),
            nombre_completo: "Sin acceso".to_owned(),
            roles_ids: vec![],
        },
    )
    .await
    .expect("retry after the external account was created must provision internally");
    let internal_state: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*)::bigint FROM public.usuarios WHERE id = $1),
            (SELECT COUNT(*)::bigint FROM public.identidades_autenticacion_externas WHERE usuario_id = $1),
            (SELECT COUNT(*)::bigint FROM public.usuarios_roles WHERE usuario_id = $1)
        "#,
    )
    .bind(retried.id)
    .fetch_one(&fixture.db)
    .await
    .expect("retried internal state must be queryable");
    assert_eq!(internal_state, (1, 1, 0));
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
async fn posting_an_existing_same_organization_subject_conflicts_without_mutation() {
    let fixture = admin_fixture().await;
    let subject = Uuid::new_v4();
    let external = successful_external_admin(subject);

    let first = access_administration::create_or_enable_user(
        &fixture.db,
        external.as_ref(),
        &fixture.context,
        &CreateUserRequest {
            correo_electronico: "existente@example.com".to_owned(),
            nombre_completo: "Nombre original".to_owned(),
            roles_ids: vec![],
        },
    )
    .await
    .expect("the initially unlinked subject must be provisioned");
    access_administration::update_user(
        &fixture.db,
        &fixture.context,
        first.id,
        &UpdateUserRequest {
            nombre_completo: None,
            activo: Some(false),
            roles_ids: None,
        },
    )
    .await
    .expect("the non-administrator user may be disabled");

    let error = access_administration::create_or_enable_user(
        &fixture.db,
        external.as_ref(),
        &fixture.context,
        &CreateUserRequest {
            correo_electronico: "existente@example.com".to_owned(),
            nombre_completo: "Nombre reemplazado".to_owned(),
            roles_ids: vec![fixture.administrator_role_id],
        },
    )
    .await
    .expect_err("POST must never mutate a subject that is already linked");
    assert_eq!(error, AccessAdministrationError::Conflict);

    let stored: (String, bool) =
        sqlx::query_as("SELECT nombre_completo, activo FROM public.usuarios WHERE id = $1")
            .bind(first.id)
            .fetch_one(&fixture.db)
            .await
            .expect("existing user must remain queryable");
    assert_eq!(stored, ("Nombre original".to_owned(), false));
    let role_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.usuarios_roles WHERE usuario_id = $1",
    )
    .bind(first.id)
    .fetch_one(&fixture.db)
    .await
    .expect("existing role history must remain queryable");
    assert_eq!(role_count, 0);
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

#[tokio::test]
async fn sole_administrator_cannot_be_disabled() {
    let fixture = admin_fixture().await;

    let error = access_administration::update_user(
        &fixture.db,
        &fixture.context,
        fixture.administrator_user_id,
        &UpdateUserRequest {
            nombre_completo: None,
            activo: Some(false),
            roles_ids: None,
        },
    )
    .await
    .expect_err("disabling the sole administrator must conflict");

    assert_eq!(error, AccessAdministrationError::Conflict);
    assert_eq!(effective_administrator_count(&fixture).await, 1);
    let active: bool = sqlx::query_scalar("SELECT activo FROM public.usuarios WHERE id = $1")
        .bind(fixture.administrator_user_id)
        .fetch_one(&fixture.db)
        .await
        .expect("administrator state must be queryable");
    assert!(active);
}

#[tokio::test]
async fn sole_administrator_cannot_remove_their_last_administrative_role() {
    let fixture = admin_fixture().await;

    let error = access_administration::update_user(
        &fixture.db,
        &fixture.context,
        fixture.administrator_user_id,
        &UpdateUserRequest {
            nombre_completo: None,
            activo: None,
            roles_ids: Some(vec![]),
        },
    )
    .await
    .expect_err("removing the sole administrator role must conflict");

    assert_eq!(error, AccessAdministrationError::Conflict);
    assert_eq!(effective_administrator_count(&fixture).await, 1);
    let current_assignment: bool = sqlx::query_scalar(
        "SELECT vigente_hasta IS NULL FROM public.usuarios_roles WHERE usuario_id = $1 AND rol_id = $2",
    )
    .bind(fixture.administrator_user_id)
    .bind(fixture.administrator_role_id)
    .fetch_one(&fixture.db)
    .await
    .expect("administrator role assignment must be queryable");
    assert!(current_assignment);
}

#[tokio::test]
async fn sole_administrator_permission_source_cannot_be_disabled_or_removed() {
    let fixture = admin_fixture().await;

    let deactivate_error = access_administration::update_role(
        &fixture.db,
        &fixture.context,
        fixture.administrator_role_id,
        &UpdateRoleRequest {
            nombre: None,
            descripcion: None,
            activo: Some(false),
            permisos: None,
        },
    )
    .await
    .expect_err("deactivating the sole administrator role must conflict");
    assert_eq!(deactivate_error, AccessAdministrationError::Conflict);

    let remove_error = access_administration::update_role(
        &fixture.db,
        &fixture.context,
        fixture.administrator_role_id,
        &UpdateRoleRequest {
            nombre: None,
            descripcion: None,
            activo: None,
            permisos: Some(vec![]),
        },
    )
    .await
    .expect_err("removing the sole administrator permission must conflict");
    assert_eq!(remove_error, AccessAdministrationError::Conflict);

    assert_eq!(effective_administrator_count(&fixture).await, 1);
    let role_state: (bool, i64) = sqlx::query_as(
        r#"
        SELECT rol.activo,
               COUNT(rol_permiso.id)::bigint
        FROM public.roles AS rol
        LEFT JOIN public.roles_permisos AS rol_permiso
          ON rol_permiso.rol_id = rol.id
         AND tstzrange(rol_permiso.vigente_desde, rol_permiso.vigente_hasta, '[)')
             @> statement_timestamp()
        WHERE rol.id = $1
        GROUP BY rol.activo
        "#,
    )
    .bind(fixture.administrator_role_id)
    .fetch_one(&fixture.db)
    .await
    .expect("administrator role state must be queryable");
    assert_eq!(role_state, (true, 1));
}

#[tokio::test]
async fn concurrent_independent_removals_leave_one_effective_administrator() {
    let fixture = admin_fixture().await;
    let second = create_user_with_roles(
        &fixture,
        Uuid::new_v4(),
        vec![fixture.administrator_role_id],
    )
    .await;
    assert_eq!(effective_administrator_count(&fixture).await, 2);

    let first_request = UpdateUserRequest {
        nombre_completo: None,
        activo: None,
        roles_ids: Some(vec![]),
    };
    let second_request = UpdateUserRequest {
        nombre_completo: None,
        activo: None,
        roles_ids: Some(vec![]),
    };
    let (first_result, second_result) = tokio::join!(
        access_administration::update_user(
            &fixture.db,
            &fixture.context,
            fixture.administrator_user_id,
            &first_request,
        ),
        access_administration::update_user(
            &fixture.db,
            &fixture.context,
            second.id,
            &second_request,
        ),
    );

    let outcomes = [first_result, second_result];
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(AccessAdministrationError::Conflict)))
            .count(),
        1
    );
    assert_eq!(effective_administrator_count(&fixture).await, 1);
}

#[tokio::test]
async fn administrative_reductions_succeed_when_another_administrator_remains() {
    let fixture = admin_fixture().await;
    let second = create_user_with_roles(
        &fixture,
        Uuid::new_v4(),
        vec![fixture.administrator_role_id],
    )
    .await;

    access_administration::update_user(
        &fixture.db,
        &fixture.context,
        second.id,
        &UpdateUserRequest {
            nombre_completo: None,
            activo: Some(false),
            roles_ids: None,
        },
    )
    .await
    .expect("a redundant administrator may be disabled");

    assert_eq!(effective_administrator_count(&fixture).await, 1);
}

#[tokio::test]
async fn finite_current_grants_are_shortened_and_synchronization_remains_idempotent() {
    let fixture = admin_fixture().await;
    let technical_role = role_with_permissions(
        &fixture,
        "Temporal",
        &[permission_codes::CONSOLA_TECNICA_VER],
    )
    .await;
    let user = create_user_with_roles(&fixture, Uuid::new_v4(), vec![technical_role]).await;

    sqlx::query(
        "UPDATE public.usuarios_roles SET vigente_hasta = transaction_timestamp() + INTERVAL '1 hour' WHERE usuario_id = $1 AND rol_id = $2 AND vigente_hasta IS NULL",
    )
    .bind(user.id)
    .bind(technical_role)
    .execute(&fixture.db)
    .await
    .expect("user-role grant must receive a future end");
    sqlx::query(
        "UPDATE public.roles_permisos SET vigente_hasta = transaction_timestamp() + INTERVAL '1 hour' WHERE rol_id = $1 AND vigente_hasta IS NULL",
    )
    .bind(technical_role)
    .execute(&fixture.db)
    .await
    .expect("role-permission grant must receive a future end");

    let user_extension = sqlx::query(
        "UPDATE public.usuarios_roles SET vigente_hasta = vigente_hasta + INTERVAL '1 hour' WHERE usuario_id = $1 AND rol_id = $2",
    )
    .bind(user.id)
    .bind(technical_role)
    .execute(&fixture.db)
    .await;
    assert!(
        user_extension.is_err(),
        "a finite user-role end cannot be extended"
    );
    let permission_extension = sqlx::query(
        "UPDATE public.roles_permisos SET vigente_hasta = vigente_hasta + INTERVAL '1 hour' WHERE rol_id = $1",
    )
    .bind(technical_role)
    .execute(&fixture.db)
    .await;
    assert!(
        permission_extension.is_err(),
        "a finite role-permission end cannot be extended"
    );

    for _ in 0..2 {
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
        .expect("current finite user-role grant must close early");
        access_administration::update_role(
            &fixture.db,
            &fixture.context,
            technical_role,
            &UpdateRoleRequest {
                nombre: None,
                descripcion: None,
                activo: None,
                permisos: Some(vec![]),
            },
        )
        .await
        .expect("current finite role-permission grant must close early");
    }

    let shortened: (bool, bool) = sqlx::query_as(
        r#"
        SELECT
            (SELECT vigente_hasta <= statement_timestamp() FROM public.usuarios_roles WHERE usuario_id = $1 AND rol_id = $2),
            (SELECT vigente_hasta <= statement_timestamp() FROM public.roles_permisos WHERE rol_id = $2)
        "#,
    )
    .bind(user.id)
    .bind(technical_role)
    .fetch_one(&fixture.db)
    .await
    .expect("shortened periods must be queryable");
    assert_eq!(shortened, (true, true));

    let rewrite_user_history = sqlx::query(
        "UPDATE public.usuarios_roles SET vigente_hasta = vigente_hasta + INTERVAL '1 second' WHERE usuario_id = $1 AND rol_id = $2",
    )
    .bind(user.id)
    .bind(technical_role)
    .execute(&fixture.db)
    .await;
    assert!(
        rewrite_user_history.is_err(),
        "closed user-role history is immutable"
    );
    let rewrite_permission_history = sqlx::query(
        "UPDATE public.roles_permisos SET vigente_hasta = vigente_hasta + INTERVAL '1 second' WHERE rol_id = $1",
    )
    .bind(technical_role)
    .execute(&fixture.db)
    .await;
    assert!(
        rewrite_permission_history.is_err(),
        "closed role-permission history is immutable"
    );

    for _ in 0..2 {
        access_administration::update_user(
            &fixture.db,
            &fixture.context,
            user.id,
            &UpdateUserRequest {
                nombre_completo: None,
                activo: None,
                roles_ids: Some(vec![technical_role]),
            },
        )
        .await
        .expect("user-role grant must be restored idempotently");
        access_administration::update_role(
            &fixture.db,
            &fixture.context,
            technical_role,
            &UpdateRoleRequest {
                nombre: None,
                descripcion: None,
                activo: None,
                permisos: Some(vec![permission_codes::CONSOLA_TECNICA_VER.to_owned()]),
            },
        )
        .await
        .expect("role-permission grant must be restored idempotently");
    }

    let history_counts: (i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*)::bigint FROM public.usuarios_roles WHERE usuario_id = $1 AND rol_id = $2),
            (SELECT COUNT(*)::bigint FROM public.roles_permisos WHERE rol_id = $2),
            (SELECT COUNT(*)::bigint FROM public.usuarios_roles AS left_period JOIN public.usuarios_roles AS right_period ON left_period.id < right_period.id AND left_period.usuario_id = right_period.usuario_id AND left_period.rol_id = right_period.rol_id AND tstzrange(left_period.vigente_desde, left_period.vigente_hasta, '[)') && tstzrange(right_period.vigente_desde, right_period.vigente_hasta, '[)') WHERE left_period.usuario_id = $1 AND left_period.rol_id = $2),
            (SELECT COUNT(*)::bigint FROM public.roles_permisos AS left_period JOIN public.roles_permisos AS right_period ON left_period.id < right_period.id AND left_period.rol_id = right_period.rol_id AND left_period.permiso_id = right_period.permiso_id AND tstzrange(left_period.vigente_desde, left_period.vigente_hasta, '[)') && tstzrange(right_period.vigente_desde, right_period.vigente_hasta, '[)') WHERE left_period.rol_id = $2)
        "#,
    )
    .bind(user.id)
    .bind(technical_role)
    .fetch_one(&fixture.db)
    .await
    .expect("temporal history must be queryable");
    assert_eq!(history_counts, (2, 2, 0, 0));
}
