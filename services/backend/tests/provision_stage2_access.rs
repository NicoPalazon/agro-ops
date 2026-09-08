use std::sync::OnceLock;

use agro_ops_backend::{
    authorization::{self, permission_codes::CONSOLA_TECNICA_VER},
    stage2_access_provisioning::{
        ProvisionStage2AccessError, ProvisionStage2AccessRequest, TECHNICAL_ROLE_NAME,
        provision_stage2_access,
    },
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

fn database_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for PostgreSQL tests");
    PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("PostgreSQL must be available with migrations applied")
}

fn request(label: &str) -> ProvisionStage2AccessRequest {
    let tag = Uuid::new_v4().simple().to_string();
    ProvisionStage2AccessRequest {
        supabase_subject: Uuid::new_v4(),
        organization_name: format!("Organizacion provision {label} {tag}"),
        user_full_name: format!("Operador provision {label} {tag}"),
    }
}

fn operational_message(error: ProvisionStage2AccessError) -> String {
    match error {
        ProvisionStage2AccessError::Operational(message) => message,
        ProvisionStage2AccessError::Database(error) => {
            panic!("expected operational provisioning error, got database error: {error}")
        }
    }
}

async fn table_count(db: &PgPool, table: &str, column: &str, value: Uuid) -> i64 {
    match (table, column) {
        ("organizaciones", "id") => {
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM public.organizaciones WHERE id = $1")
                .bind(value)
                .fetch_one(db)
                .await
                .expect("organization count must be queryable")
        }
        ("usuarios", "organizacion_id") => {
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM public.usuarios WHERE organizacion_id = $1")
                .bind(value)
                .fetch_one(db)
                .await
                .expect("user count must be queryable")
        }
        ("identidades_autenticacion_externas", "sujeto_proveedor") => {
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM public.identidades_autenticacion_externas WHERE proveedor = 'supabase' AND sujeto_proveedor = $1")
                .bind(value)
                .fetch_one(db)
                .await
                .expect("identity count must be queryable")
        }
        ("roles", "organizacion_id") => {
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM public.roles WHERE organizacion_id = $1 AND nombre = 'Tecnico'")
                .bind(value)
                .fetch_one(db)
                .await
                .expect("role count must be queryable")
        }
        ("usuarios_roles", "usuario_id") => {
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM public.usuarios_roles WHERE usuario_id = $1")
                .bind(value)
                .fetch_one(db)
                .await
                .expect("user-role count must be queryable")
        }
        ("roles_permisos", "rol_id") => {
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM public.roles_permisos WHERE rol_id = $1")
                .bind(value)
                .fetch_one(db)
                .await
                .expect("role-permission count must be queryable")
        }
        _ => unreachable!("test helper has a fixed catalog"),
    }
}

#[tokio::test]
async fn first_provisioning_materializes_the_exact_authorization_path() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let request = request("first");

    let result = provision_stage2_access(&db, &request)
        .await
        .expect("first provisioning must succeed");
    assert_eq!(result.supabase_subject, request.supabase_subject);
    assert_eq!(result.role_name, TECHNICAL_ROLE_NAME);
    assert_eq!(result.permission_code, CONSOLA_TECNICA_VER);

    let organization: (String, bool) =
        sqlx::query_as("SELECT nombre, activa FROM public.organizaciones WHERE id = $1")
            .bind(result.organization_id)
            .fetch_one(&db)
            .await
            .expect("organization must exist");
    assert_eq!(organization, (request.organization_name.clone(), true));

    let user: (String, bool, Uuid) = sqlx::query_as(
        "SELECT nombre_completo, activo, organizacion_id FROM public.usuarios WHERE id = $1",
    )
    .bind(result.user_id)
    .fetch_one(&db)
    .await
    .expect("user must exist");
    assert_eq!(
        user,
        (request.user_full_name.clone(), true, result.organization_id)
    );

    let identity_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.identidades_autenticacion_externas WHERE usuario_id = $1 AND proveedor = 'supabase' AND sujeto_proveedor = $2 AND desvinculada_en IS NULL",
    )
    .bind(result.user_id)
    .bind(request.supabase_subject)
    .fetch_one(&db)
    .await
    .expect("identity must be queryable");
    assert_eq!(identity_count, 1);

    let permissions: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT permiso.codigo
        FROM public.roles_permisos AS rol_permiso
        JOIN public.permisos AS permiso ON permiso.id = rol_permiso.permiso_id
        JOIN public.roles AS rol ON rol.id = rol_permiso.rol_id
        WHERE rol.organizacion_id = $1
          AND rol.nombre = 'Tecnico'
          AND tstzrange(rol_permiso.vigente_desde, rol_permiso.vigente_hasta, '[)') @> transaction_timestamp()
        ORDER BY permiso.codigo
        "#,
    )
    .bind(result.organization_id)
    .fetch_all(&db)
    .await
    .expect("technical role permissions must be queryable");
    assert_eq!(permissions, vec![CONSOLA_TECNICA_VER.to_owned()]);

    let context = authorization::resolve_context(&db, request.supabase_subject)
        .await
        .expect("provisioned authorization context must resolve");
    assert_eq!(context.user_id, result.user_id);
    assert_eq!(context.organization_id, result.organization_id);
    assert!(context.has_permission(CONSOLA_TECNICA_VER));
}

#[tokio::test]
async fn identical_provisioning_is_idempotent_without_new_history() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let request = request("idempotent");
    let first = provision_stage2_access(&db, &request)
        .await
        .expect("first provisioning must succeed");
    let role_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM public.roles WHERE organizacion_id = $1 AND nombre = 'Tecnico'",
    )
    .bind(first.organization_id)
    .fetch_one(&db)
    .await
    .expect("technical role must exist");
    let before = [
        table_count(&db, "organizaciones", "id", first.organization_id).await,
        table_count(&db, "usuarios", "organizacion_id", first.organization_id).await,
        table_count(
            &db,
            "identidades_autenticacion_externas",
            "sujeto_proveedor",
            request.supabase_subject,
        )
        .await,
        table_count(&db, "roles", "organizacion_id", first.organization_id).await,
        table_count(&db, "usuarios_roles", "usuario_id", first.user_id).await,
        table_count(&db, "roles_permisos", "rol_id", role_id).await,
    ];

    let second = provision_stage2_access(&db, &request)
        .await
        .expect("second identical provisioning must succeed");
    let after = [
        table_count(&db, "organizaciones", "id", second.organization_id).await,
        table_count(&db, "usuarios", "organizacion_id", second.organization_id).await,
        table_count(
            &db,
            "identidades_autenticacion_externas",
            "sujeto_proveedor",
            request.supabase_subject,
        )
        .await,
        table_count(&db, "roles", "organizacion_id", second.organization_id).await,
        table_count(&db, "usuarios_roles", "usuario_id", second.user_id).await,
        table_count(&db, "roles_permisos", "rol_id", role_id).await,
    ];
    assert_eq!(second, first);
    assert_eq!(after, before);
}

#[tokio::test]
async fn concurrent_identical_provisioning_creates_one_logical_state() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let request = request("concurrent");

    let (first, second) = tokio::join!(
        provision_stage2_access(&db, &request),
        provision_stage2_access(&db, &request),
    );
    let first = first.expect("first concurrent provisioning must succeed");
    let second = second.expect("second concurrent provisioning must succeed");
    assert_eq!(first, second);
    let role_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM public.roles WHERE organizacion_id = $1 AND nombre = 'Tecnico'",
    )
    .bind(first.organization_id)
    .fetch_one(&db)
    .await
    .expect("technical role must exist");
    assert_eq!(
        table_count(&db, "usuarios", "organizacion_id", first.organization_id).await,
        1
    );
    assert_eq!(
        table_count(
            &db,
            "identidades_autenticacion_externas",
            "sujeto_proveedor",
            request.supabase_subject,
        )
        .await,
        1
    );
    assert_eq!(
        table_count(&db, "usuarios_roles", "usuario_id", first.user_id).await,
        1
    );
    assert_eq!(
        table_count(&db, "roles_permisos", "rol_id", role_id).await,
        1
    );
}

#[tokio::test]
async fn closed_grants_receive_new_current_episodes_without_rewriting_history() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let request = request("historical-regrant");
    let first = provision_stage2_access(&db, &request)
        .await
        .expect("first provisioning must succeed");
    let role_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM public.roles WHERE organizacion_id = $1 AND nombre = 'Tecnico'",
    )
    .bind(first.organization_id)
    .fetch_one(&db)
    .await
    .expect("technical role must exist");
    let permission_id: Uuid =
        sqlx::query_scalar("SELECT id FROM public.permisos WHERE codigo = $1")
            .bind(CONSOLA_TECNICA_VER)
            .fetch_one(&db)
            .await
            .expect("canonical permission must exist");
    let old_user_role_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM public.usuarios_roles WHERE usuario_id = $1 AND rol_id = $2 AND vigente_hasta IS NULL",
    )
    .bind(first.user_id)
    .bind(role_id)
    .fetch_one(&db)
    .await
    .expect("current user-role episode must exist");
    let old_role_permission_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM public.roles_permisos WHERE rol_id = $1 AND permiso_id = $2 AND vigente_hasta IS NULL",
    )
    .bind(role_id)
    .bind(permission_id)
    .fetch_one(&db)
    .await
    .expect("current role-permission episode must exist");
    sqlx::query("UPDATE public.usuarios_roles SET vigente_hasta = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(old_user_role_id)
        .execute(&db)
        .await
        .expect("current user-role episode must close");
    sqlx::query("UPDATE public.roles_permisos SET vigente_hasta = CURRENT_TIMESTAMP WHERE id = $1")
        .bind(old_role_permission_id)
        .execute(&db)
        .await
        .expect("current role-permission episode must close");

    provision_stage2_access(&db, &request)
        .await
        .expect("provisioning must re-grant closed episodes");
    let old_closed: (bool, bool) = sqlx::query_as(
        r#"
        SELECT
            (SELECT vigente_hasta IS NOT NULL FROM public.usuarios_roles WHERE id = $1),
            (SELECT vigente_hasta IS NOT NULL FROM public.roles_permisos WHERE id = $2)
        "#,
    )
    .bind(old_user_role_id)
    .bind(old_role_permission_id)
    .fetch_one(&db)
    .await
    .expect("historical episodes must remain queryable");
    assert_eq!(old_closed, (true, true));
    assert_eq!(
        table_count(&db, "usuarios_roles", "usuario_id", first.user_id).await,
        2
    );
    assert_eq!(
        table_count(&db, "roles_permisos", "rol_id", role_id).await,
        2
    );
    let current_counts: (i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*)::bigint FROM public.usuarios_roles WHERE usuario_id = $1 AND rol_id = $2 AND tstzrange(vigente_desde, vigente_hasta, '[)') @> transaction_timestamp()),
            (SELECT COUNT(*)::bigint FROM public.roles_permisos WHERE rol_id = $2 AND permiso_id = $3 AND tstzrange(vigente_desde, vigente_hasta, '[)') @> transaction_timestamp())
        "#,
    )
    .bind(first.user_id)
    .bind(role_id)
    .bind(permission_id)
    .fetch_one(&db)
    .await
    .expect("current episode counts must be queryable");
    assert_eq!(current_counts, (1, 1));
}

#[tokio::test]
async fn inactive_organization_user_or_role_fails_closed() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;

    let inactive_organization = request("inactive-organization");
    sqlx::query("INSERT INTO public.organizaciones (nombre, activa) VALUES ($1, false)")
        .bind(&inactive_organization.organization_name)
        .execute(&db)
        .await
        .expect("inactive organization fixture must insert");
    assert!(
        operational_message(
            provision_stage2_access(&db, &inactive_organization)
                .await
                .expect_err("inactive organization must fail"),
        )
        .contains("inactive")
    );

    let inactive_user = request("inactive-user");
    let organization_id: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(&inactive_user.organization_name)
            .fetch_one(&db)
            .await
            .expect("organization fixture must insert");
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo, activo) VALUES ($1, $2, false) RETURNING id",
    )
    .bind(organization_id)
    .bind(&inactive_user.user_full_name)
    .fetch_one(&db)
    .await
    .expect("inactive user fixture must insert");
    sqlx::query(
        "INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, 'supabase', $2)",
    )
    .bind(user_id)
    .bind(inactive_user.supabase_subject)
    .execute(&db)
    .await
    .expect("inactive user identity fixture must insert");
    assert!(
        operational_message(
            provision_stage2_access(&db, &inactive_user)
                .await
                .expect_err("inactive user must fail"),
        )
        .contains("inactive user")
    );
    let user_active: bool = sqlx::query_scalar("SELECT activo FROM public.usuarios WHERE id = $1")
        .bind(user_id)
        .fetch_one(&db)
        .await
        .expect("inactive user must remain queryable");
    assert!(!user_active);

    let inactive_role = request("inactive-role");
    let role_organization_id: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(&inactive_role.organization_name)
            .fetch_one(&db)
            .await
            .expect("role organization fixture must insert");
    let role_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.roles (organizacion_id, nombre, activo) VALUES ($1, 'Tecnico', false) RETURNING id",
    )
    .bind(role_organization_id)
    .fetch_one(&db)
    .await
    .expect("inactive role fixture must insert");
    assert!(
        operational_message(
            provision_stage2_access(&db, &inactive_role)
                .await
                .expect_err("inactive role must fail"),
        )
        .contains("inactive")
    );
    let role_active: bool = sqlx::query_scalar("SELECT activo FROM public.roles WHERE id = $1")
        .bind(role_id)
        .fetch_one(&db)
        .await
        .expect("inactive role must remain queryable");
    assert!(!role_active);
}

#[tokio::test]
async fn incompatible_external_identity_fails_without_relinking() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let request = request("incompatible-identity");
    let other_organization_id: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(format!("Other organization {}", Uuid::new_v4()))
            .fetch_one(&db)
            .await
            .expect("other organization must insert");
    let other_user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
    )
    .bind(other_organization_id)
    .bind(format!("Other user {}", Uuid::new_v4()))
    .fetch_one(&db)
    .await
    .expect("other user must insert");
    sqlx::query(
        "INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, 'supabase', $2)",
    )
    .bind(other_user_id)
    .bind(request.supabase_subject)
    .execute(&db)
    .await
    .expect("other identity must insert");

    assert!(
        operational_message(
            provision_stage2_access(&db, &request)
                .await
                .expect_err("incompatible identity must fail"),
        )
        .contains("different organization")
    );
    let linked_user_id: Uuid = sqlx::query_scalar(
        "SELECT usuario_id FROM public.identidades_autenticacion_externas WHERE proveedor = 'supabase' AND sujeto_proveedor = $1",
    )
    .bind(request.supabase_subject)
    .fetch_one(&db)
    .await
    .expect("existing identity must remain queryable");
    assert_eq!(linked_user_id, other_user_id);
}

#[tokio::test]
async fn closed_external_identity_fails_without_reopening_history() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let request = request("closed-identity");
    let organization_id: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(&request.organization_name)
            .fetch_one(&db)
            .await
            .expect("organization fixture must insert");
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
    )
    .bind(organization_id)
    .bind(&request.user_full_name)
    .fetch_one(&db)
    .await
    .expect("user fixture must insert");
    sqlx::query(
        "INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor, vinculada_en, desvinculada_en) VALUES ($1, 'supabase', $2, CURRENT_TIMESTAMP - INTERVAL '2 hours', CURRENT_TIMESTAMP - INTERVAL '1 hour')",
    )
    .bind(user_id)
    .bind(request.supabase_subject)
    .execute(&db)
    .await
    .expect("closed identity fixture must insert");

    assert!(
        operational_message(
            provision_stage2_access(&db, &request)
                .await
                .expect_err("closed external identity must fail"),
        )
        .contains("historical identity links are not reopened")
    );
    let identity: (Uuid, bool) = sqlx::query_as(
        "SELECT usuario_id, desvinculada_en IS NOT NULL FROM public.identidades_autenticacion_externas WHERE proveedor = 'supabase' AND sujeto_proveedor = $1",
    )
    .bind(request.supabase_subject)
    .fetch_one(&db)
    .await
    .expect("historical identity must remain queryable");
    assert_eq!(identity, (user_id, true));
}

#[tokio::test]
async fn existing_technical_role_with_an_extra_permission_fails_without_altering_it() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let request = request("extra-role-permission");
    let organization_id: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(&request.organization_name)
            .fetch_one(&db)
            .await
            .expect("organization fixture must insert");
    let role_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.roles (organizacion_id, nombre) VALUES ($1, 'Tecnico') RETURNING id",
    )
    .bind(organization_id)
    .fetch_one(&db)
    .await
    .expect("role fixture must insert");
    let other_permission_id: Uuid =
        sqlx::query_scalar("SELECT id FROM public.permisos WHERE codigo = 'panel:ver'")
            .fetch_one(&db)
            .await
            .expect("other canonical permission must exist");
    sqlx::query("INSERT INTO public.roles_permisos (rol_id, permiso_id) VALUES ($1, $2)")
        .bind(role_id)
        .bind(other_permission_id)
        .execute(&db)
        .await
        .expect("extra role permission fixture must insert");

    assert!(
        operational_message(
            provision_stage2_access(&db, &request)
                .await
                .expect_err("an existing technical role with extra permissions must fail"),
        )
        .contains("outside consola_tecnica:ver")
    );
    let permission_codes: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT permiso.codigo
        FROM public.roles_permisos AS rol_permiso
        JOIN public.permisos AS permiso ON permiso.id = rol_permiso.permiso_id
        WHERE rol_permiso.rol_id = $1
        ORDER BY permiso.codigo
        "#,
    )
    .bind(role_id)
    .fetch_all(&db)
    .await
    .expect("pre-existing role permission must remain queryable");
    assert_eq!(permission_codes, vec!["panel:ver".to_owned()]);
}

#[tokio::test]
async fn quote_and_sql_looking_display_values_are_bound_as_data() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let tag = Uuid::new_v4();
    let request = ProvisionStage2AccessRequest {
        supabase_subject: Uuid::new_v4(),
        organization_name: format!("Empresa'); DROP TABLE permisos; -- {tag}"),
        user_full_name: format!("Operador'); DROP TABLE roles; -- {tag}"),
    };

    let result = provision_stage2_access(&db, &request)
        .await
        .expect("bound quote-containing values must provision normally");
    let stored_values: (String, String) = sqlx::query_as(
        r#"
        SELECT organizacion.nombre, usuario.nombre_completo
        FROM public.organizaciones AS organizacion
        JOIN public.usuarios AS usuario ON usuario.organizacion_id = organizacion.id
        WHERE organizacion.id = $1 AND usuario.id = $2
        "#,
    )
    .bind(result.organization_id)
    .bind(result.user_id)
    .fetch_one(&db)
    .await
    .expect("quoted values must remain ordinary stored data");
    assert_eq!(
        stored_values,
        (request.organization_name, request.user_full_name)
    );
    let permissions_relation: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.permisos')::text")
            .fetch_one(&db)
            .await
            .expect("permissions relation lookup must succeed");
    assert_eq!(permissions_relation.as_deref(), Some("permisos"));
    let canonical_permission_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM public.permisos WHERE codigo = $1")
            .bind(CONSOLA_TECNICA_VER)
            .fetch_one(&db)
            .await
            .expect("canonical permission must remain queryable");
    assert_eq!(canonical_permission_count, 1);
}
