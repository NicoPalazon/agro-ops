use std::{collections::HashSet, sync::Arc};

use agro_ops_backend::{
    access_administration::{
        self, AccessAdministrationError, CreateRoleRequest, CreateUserRequest, UpdateRoleRequest,
        UpdateUserRequest,
    },
    authorization::{self, AuthorizationContext, permission_codes},
    supabase_admin::{ExternalAuthUser, ExternalIdentityAdmin, ExternalIdentityAdminError},
};
use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{AssertSqlSafe, PgPool, postgres::PgPoolOptions, types::Json};
use uuid::Uuid;

type StoredAuditEvent = (
    Uuid,
    String,
    Option<Uuid>,
    String,
    String,
    Option<Uuid>,
    Option<Json<Value>>,
    Option<Json<Value>>,
);

const CONFIGURATION_VIEW_PERMISSION: &str = "configuracion:ver";

#[derive(Clone)]
struct MockExternalAdmin {
    result: Result<ExternalAuthUser, ExternalIdentityAdminError>,
    email_lookup_error: Option<ExternalIdentityAdminError>,
    unresolved_email_subjects: HashSet<Uuid>,
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

    async fn correos_electronicos_por_sujeto(
        &self,
        subjects: &[Uuid],
    ) -> Result<std::collections::HashMap<Uuid, String>, ExternalIdentityAdminError> {
        if let Some(error) = self.email_lookup_error {
            return Err(error);
        }
        Ok(subjects
            .iter()
            .filter(|subject| !self.unresolved_email_subjects.contains(subject))
            .map(|subject| (*subject, format!("{subject}@example.com")))
            .collect())
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
    administrator_subject: Uuid,
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
        administrator_subject: subject,
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
        email_lookup_error: None,
        unresolved_email_subjects: HashSet::new(),
    })
}

async fn audit_events_for_entity(fixture: &AdminFixture, entity_id: Uuid) -> Vec<StoredAuditEvent> {
    sqlx::query_as(
        r#"
        SELECT
            organizacion_id,
            actor_tipo,
            actor_usuario_id,
            accion,
            entidad_tipo,
            entidad_id,
            estado_anterior,
            estado_posterior
        FROM public.audit_events
        WHERE organizacion_id = $1 AND entidad_id = $2
        ORDER BY ocurrido_en, id
        "#,
    )
    .bind(fixture.organization_id)
    .bind(entity_id)
    .fetch_all(&fixture.db)
    .await
    .expect("audit events must be queryable")
}

fn audit_event_by_action<'a>(events: &'a [StoredAuditEvent], action: &str) -> &'a StoredAuditEvent {
    events
        .iter()
        .find(|event| event.3 == action)
        .expect("expected audit action must exist")
}

#[tokio::test]
async fn administrative_user_list_includes_current_supabase_email_for_duplicate_names() {
    let fixture = admin_fixture().await;
    let second_subject = Uuid::new_v4();
    let second_user: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
    )
    .bind(fixture.organization_id)
    .bind("Administrador de prueba")
    .fetch_one(&fixture.db)
    .await
    .expect("second user must insert");
    sqlx::query(
        "INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, 'supabase', $2)",
    )
    .bind(second_user)
    .bind(second_subject)
    .execute(&fixture.db)
    .await
    .expect("second Supabase identity must insert");

    let users = access_administration::list_users(
        &fixture.db,
        successful_external_admin(Uuid::new_v4()).as_ref(),
        &fixture.context,
    )
    .await
    .expect("administrative user list must load");

    let duplicate_names: Vec<_> = users
        .usuarios
        .iter()
        .filter(|user| user.nombre_completo == "Administrador de prueba")
        .collect();
    assert_eq!(duplicate_names.len(), 2);
    assert!(
        duplicate_names
            .iter()
            .all(|user| user.correo_electronico.is_some())
    );
    assert_ne!(
        duplicate_names[0].correo_electronico,
        duplicate_names[1].correo_electronico
    );
    let response = serde_json::to_value(&users).expect("user response must serialize");
    assert!(
        response["usuarios"]
            .as_array()
            .expect("users must be an array")
            .iter()
            .all(|user| user.get("correo_electronico").is_some())
    );
}

#[tokio::test]
async fn deleted_supabase_subject_keeps_user_state_and_roles_with_null_email() {
    let fixture = admin_fixture().await;
    let external = MockExternalAdmin {
        result: Ok(ExternalAuthUser {
            subject: fixture.administrator_subject,
        }),
        email_lookup_error: None,
        unresolved_email_subjects: HashSet::from([fixture.administrator_subject]),
    };

    let users = access_administration::list_users(&fixture.db, &external, &fixture.context)
        .await
        .expect("a deleted Supabase subject must not fail the PostgreSQL user list");
    let administrator = users
        .usuarios
        .iter()
        .find(|user| user.id == fixture.administrator_user_id)
        .expect("the Agro Ops administrator must remain listed");

    assert_eq!(administrator.correo_electronico, None);
    assert!(administrator.activo);
    assert_eq!(administrator.roles.len(), 1);
    assert_eq!(administrator.roles[0].id, fixture.administrator_role_id);
}

#[tokio::test]
async fn user_without_external_identity_keeps_inactive_state_and_roles_with_null_email() {
    let fixture = admin_fixture().await;
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo, activo) VALUES ($1, 'Usuario sin identidad', false) RETURNING id",
    )
    .bind(fixture.organization_id)
    .fetch_one(&fixture.db)
    .await
    .expect("user without external identity must insert");
    sqlx::query("INSERT INTO public.usuarios_roles (usuario_id, rol_id) VALUES ($1, $2)")
        .bind(user_id)
        .bind(fixture.administrator_role_id)
        .execute(&fixture.db)
        .await
        .expect("test role assignment must insert");

    let users = access_administration::list_users(
        &fixture.db,
        successful_external_admin(Uuid::new_v4()).as_ref(),
        &fixture.context,
    )
    .await
    .expect("a missing external identity must not fail the PostgreSQL user list");
    let user = users
        .usuarios
        .iter()
        .find(|user| user.id == user_id)
        .expect("the user without an external identity must remain listed");

    assert_eq!(user.correo_electronico, None);
    assert!(!user.activo);
    assert_eq!(user.roles.len(), 1);
    assert_eq!(user.roles[0].id, fixture.administrator_role_id);
}

#[tokio::test]
async fn unavailable_email_enrichment_keeps_authoritative_postgresql_users() {
    let fixture = admin_fixture().await;
    let external = MockExternalAdmin {
        result: Ok(ExternalAuthUser {
            subject: fixture.administrator_subject,
        }),
        email_lookup_error: Some(ExternalIdentityAdminError::Unavailable),
        unresolved_email_subjects: HashSet::new(),
    };

    let users = access_administration::list_users(&fixture.db, &external, &fixture.context)
        .await
        .expect("unavailable email enrichment must not fail the PostgreSQL user list");
    let administrator = users
        .usuarios
        .iter()
        .find(|user| user.id == fixture.administrator_user_id)
        .expect("the Agro Ops administrator must remain listed");

    assert_eq!(administrator.correo_electronico, None);
    assert!(administrator.activo);
    assert_eq!(administrator.roles.len(), 1);
    assert_eq!(administrator.roles[0].id, fixture.administrator_role_id);
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
async fn user_creation_activity_and_role_changes_append_safe_administrative_audit_events() {
    let fixture = admin_fixture().await;
    let first_role = role_with_permissions(
        &fixture,
        "Operador inicial",
        &[permission_codes::CONSOLA_TECNICA_VER],
    )
    .await;
    let replacement_role = role_with_permissions(&fixture, "Operador reemplazo", &[]).await;
    let user = create_user_with_roles(&fixture, Uuid::new_v4(), vec![first_role]).await;

    access_administration::update_user(
        &fixture.db,
        successful_external_admin(Uuid::new_v4()).as_ref(),
        &fixture.context,
        user.id,
        &UpdateUserRequest {
            nombre_completo: None,
            activo: Some(false),
            roles_ids: Some(vec![replacement_role]),
        },
    )
    .await
    .expect("user activity and roles must update");
    access_administration::update_user(
        &fixture.db,
        successful_external_admin(Uuid::new_v4()).as_ref(),
        &fixture.context,
        user.id,
        &UpdateUserRequest {
            nombre_completo: None,
            activo: Some(true),
            roles_ids: None,
        },
    )
    .await
    .expect("user must re-enable");

    let events = audit_events_for_entity(&fixture, user.id).await;
    assert_eq!(events.len(), 4);
    for event in &events {
        assert_eq!(event.0, fixture.organization_id);
        assert_eq!(event.1, "usuario");
        assert_eq!(event.2, Some(fixture.administrator_user_id));
        assert_eq!(event.4, "usuario");
        assert_eq!(event.5, Some(user.id));
    }
    let created = audit_event_by_action(&events, "usuario.creado");
    let disabled = audit_event_by_action(&events, "usuario.deshabilitado");
    let roles_changed = audit_event_by_action(&events, "usuario.roles_actualizados");
    let enabled = audit_event_by_action(&events, "usuario.habilitado");
    assert_eq!(created.6, None);
    assert_eq!(
        created.7.as_ref().map(|value| &value.0),
        Some(&json!({
            "usuario_id": user.id,
            "activo": true,
            "roles_ids": [first_role],
        }))
    );
    assert_eq!(
        disabled.6.as_ref().map(|value| &value.0),
        Some(&json!({"usuario_id": user.id, "activo": true}))
    );
    assert_eq!(
        disabled.7.as_ref().map(|value| &value.0),
        Some(&json!({"usuario_id": user.id, "activo": false}))
    );
    assert_eq!(
        roles_changed.6.as_ref().map(|value| &value.0),
        Some(&json!({"usuario_id": user.id, "roles_ids": [first_role]}))
    );
    assert_eq!(
        roles_changed.7.as_ref().map(|value| &value.0),
        Some(&json!({"usuario_id": user.id, "roles_ids": [replacement_role]}))
    );
    assert_eq!(
        enabled.6.as_ref().map(|value| &value.0),
        Some(&json!({"usuario_id": user.id, "activo": false}))
    );
    assert_eq!(
        enabled.7.as_ref().map(|value| &value.0),
        Some(&json!({"usuario_id": user.id, "activo": true}))
    );
    assert!(created.7.as_ref().is_some_and(|value| {
        value.0.get("correo_electronico").is_none()
            && value.0.get("sujeto_proveedor").is_none()
            && value.0.get("token").is_none()
    }));

    let role_history: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COUNT(vigente_hasta)::bigint FROM public.usuarios_roles WHERE usuario_id = $1",
    )
    .bind(user.id)
    .fetch_one(&fixture.db)
    .await
    .expect("user role history must be queryable");
    assert_eq!(role_history, (2, 1));
}

#[tokio::test]
async fn role_creation_activity_and_permission_changes_append_safe_administrative_audit_events() {
    let fixture = admin_fixture().await;
    let role = access_administration::create_role(
        &fixture.db,
        &fixture.context,
        &CreateRoleRequest {
            nombre: format!("Rol auditado {}", Uuid::new_v4()),
            descripcion: Some("Descripción que no se audita".to_owned()),
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
            activo: Some(false),
            permisos: Some(vec![CONFIGURATION_VIEW_PERMISSION.to_owned()]),
        },
    )
    .await
    .expect("role activity and permissions must update");
    access_administration::update_role(
        &fixture.db,
        &fixture.context,
        role.id,
        &UpdateRoleRequest {
            nombre: None,
            descripcion: None,
            activo: Some(true),
            permisos: None,
        },
    )
    .await
    .expect("role must re-enable");

    let events = audit_events_for_entity(&fixture, role.id).await;
    assert_eq!(events.len(), 4);
    for event in &events {
        assert_eq!(event.0, fixture.organization_id);
        assert_eq!(event.1, "usuario");
        assert_eq!(event.2, Some(fixture.administrator_user_id));
        assert_eq!(event.4, "rol");
        assert_eq!(event.5, Some(role.id));
    }
    let created = audit_event_by_action(&events, "rol.creado");
    let disabled = audit_event_by_action(&events, "rol.deshabilitado");
    let permissions_changed = audit_event_by_action(&events, "rol.permisos_actualizados");
    let enabled = audit_event_by_action(&events, "rol.habilitado");
    assert_eq!(created.6, None);
    assert_eq!(
        created.7.as_ref().map(|value| &value.0),
        Some(&json!({
            "rol_id": role.id,
            "activo": true,
            "permisos": [permission_codes::CONSOLA_TECNICA_VER],
        }))
    );
    assert_eq!(
        disabled.6.as_ref().map(|value| &value.0),
        Some(&json!({"rol_id": role.id, "activo": true}))
    );
    assert_eq!(
        disabled.7.as_ref().map(|value| &value.0),
        Some(&json!({"rol_id": role.id, "activo": false}))
    );
    assert_eq!(
        permissions_changed.6.as_ref().map(|value| &value.0),
        Some(&json!({
            "rol_id": role.id,
            "permisos": [permission_codes::CONSOLA_TECNICA_VER],
        }))
    );
    assert_eq!(
        permissions_changed.7.as_ref().map(|value| &value.0),
        Some(&json!({
            "rol_id": role.id,
            "permisos": [CONFIGURATION_VIEW_PERMISSION],
        }))
    );
    assert_eq!(
        enabled.6.as_ref().map(|value| &value.0),
        Some(&json!({"rol_id": role.id, "activo": false}))
    );
    assert_eq!(
        enabled.7.as_ref().map(|value| &value.0),
        Some(&json!({"rol_id": role.id, "activo": true}))
    );

    let permission_history: (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COUNT(vigente_hasta)::bigint FROM public.roles_permisos WHERE rol_id = $1",
    )
    .bind(role.id)
    .fetch_one(&fixture.db)
    .await
    .expect("role permission history must be queryable");
    assert_eq!(permission_history, (2, 1));
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
    let failed_creation_audits: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.audit_events WHERE organizacion_id = $1 AND accion = 'usuario.creado'",
    )
    .bind(fixture.organization_id)
    .fetch_one(&fixture.db)
    .await
    .expect("failed creation audit count must be queryable");
    assert_eq!(failed_creation_audits, 0);

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
async fn audit_insert_failure_rolls_back_the_internal_user_creation() {
    let fixture = admin_fixture().await;
    sqlx::query(AssertSqlSafe(format!(
        "ALTER TABLE public.audit_events ADD CONSTRAINT ck_audit_events_test_actor_rejected CHECK (actor_usuario_id IS DISTINCT FROM '{}'::uuid)",
        fixture.administrator_user_id,
    )))
    .execute(&fixture.db)
    .await
    .expect("test audit constraint must install");

    let subject = Uuid::new_v4();
    let result = access_administration::create_or_enable_user(
        &fixture.db,
        successful_external_admin(subject).as_ref(),
        &fixture.context,
        &CreateUserRequest {
            correo_electronico: "rollback@example.com".to_owned(),
            nombre_completo: "Usuario revertido".to_owned(),
            roles_ids: vec![],
        },
    )
    .await;

    sqlx::query(
        "ALTER TABLE public.audit_events DROP CONSTRAINT ck_audit_events_test_actor_rejected",
    )
    .execute(&fixture.db)
    .await
    .expect("test audit constraint must remove");

    assert!(matches!(
        result,
        Err(AccessAdministrationError::DatabaseUnavailable)
    ));
    let internal_state: (i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*)::bigint FROM public.usuarios WHERE organizacion_id = $1 AND nombre_completo = 'Usuario revertido'),
            (SELECT COUNT(*)::bigint FROM public.identidades_autenticacion_externas WHERE sujeto_proveedor = $2),
            (SELECT COUNT(*)::bigint FROM public.audit_events WHERE organizacion_id = $1 AND accion = 'usuario.creado')
        "#,
    )
    .bind(fixture.organization_id)
    .bind(subject)
    .fetch_one(&fixture.db)
    .await
    .expect("rolled-back internal state must be queryable");
    assert_eq!(internal_state, (0, 0, 0));
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
        email_lookup_error: None,
        unresolved_email_subjects: HashSet::new(),
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
        successful_external_admin(Uuid::new_v4()).as_ref(),
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
        successful_external_admin(Uuid::new_v4()).as_ref(),
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
        successful_external_admin(Uuid::new_v4()).as_ref(),
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
        successful_external_admin(Uuid::new_v4()).as_ref(),
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
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.audit_events WHERE organizacion_id = $1 AND accion = 'usuario.deshabilitado'",
    )
    .bind(fixture.organization_id)
    .fetch_one(&fixture.db)
    .await
    .expect("last-administrator audit count must be queryable");
    assert_eq!(audit_count, 0);
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
        successful_external_admin(Uuid::new_v4()).as_ref(),
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
    let first_external = successful_external_admin(Uuid::new_v4());
    let second_external = successful_external_admin(Uuid::new_v4());
    let (first_result, second_result) = tokio::join!(
        access_administration::update_user(
            &fixture.db,
            first_external.as_ref(),
            &fixture.context,
            fixture.administrator_user_id,
            &first_request,
        ),
        access_administration::update_user(
            &fixture.db,
            second_external.as_ref(),
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
        successful_external_admin(Uuid::new_v4()).as_ref(),
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
            successful_external_admin(Uuid::new_v4()).as_ref(),
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
            successful_external_admin(Uuid::new_v4()).as_ref(),
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
