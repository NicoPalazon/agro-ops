use sqlx::{AssertSqlSafe, PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use std::sync::OnceLock;
use uuid::Uuid;

const EXCLUSION: &str = "23P01";
const FK: &str = "23503";
const RAISE: &str = "P0001";
const UNIQUE: &str = "23505";
const ORG_MISMATCH: &str = "agro_ops_usuario_rol_organizacion_invalida";
const IDENTITY_IMMUTABLE: &str = "agro_ops_identidad_auth_historia_inmutable";
const ROLE_PERMISSION_DELETE: &str = "agro_ops_rol_permiso_historia_eliminacion_prohibida";
const ROLE_PERMISSION_IMMUTABLE: &str = "agro_ops_rol_permiso_historia_inmutable";
const TRUNCATE_FORBIDDEN: &str = "agro_ops_historia_autorizacion_truncate_prohibido";
const USER_ROLE_DELETE: &str = "agro_ops_usuario_rol_historia_eliminacion_prohibida";
const USER_ROLE_IMMUTABLE: &str = "agro_ops_usuario_rol_historia_inmutable";

fn database_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn assert_database_error(error: sqlx::Error, sqlstate: &str, message: Option<&str>) {
    let database_error = error
        .as_database_error()
        .expect("expected a PostgreSQL database error");
    assert_eq!(database_error.code().as_deref(), Some(sqlstate));
    if let Some(message) = message {
        assert_eq!(database_error.message(), message);
    }
}

macro_rules! assert_statement_fails {
    ($tx:ident, $operation:expr, $sqlstate:expr, $message:expr) => {{
        sqlx::query("SAVEPOINT expected_failure")
            .execute(&mut *$tx)
            .await
            .expect("savepoint must be created");
        let error = $operation
            .await
            .expect_err("database statement unexpectedly succeeded");
        assert_database_error(error, $sqlstate, $message);
        sqlx::query("ROLLBACK TO SAVEPOINT expected_failure")
            .execute(&mut *$tx)
            .await
            .expect("failed statement must be rolled back");
        sqlx::query("RELEASE SAVEPOINT expected_failure")
            .execute(&mut *$tx)
            .await
            .expect("savepoint must be released");
    }};
}

fn nuevo_uuid() -> String {
    Uuid::new_v4().to_string()
}

async fn test_pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("PostgreSQL with migrations must be available")
}

async fn organizar(tx: &mut Transaction<'_, Postgres>, nombre: &str) -> String {
    let id = nuevo_uuid();
    sqlx::query("INSERT INTO public.organizaciones (id, nombre) VALUES ($1::uuid, $2)")
        .bind(&id)
        .bind(nombre)
        .execute(&mut **tx)
        .await
        .expect("organization must insert");
    id
}

async fn usuario(
    tx: &mut Transaction<'_, Postgres>,
    organizacion_id: &str,
    nombre: &str,
) -> String {
    let id = nuevo_uuid();
    sqlx::query(
        "INSERT INTO public.usuarios (id, organizacion_id, nombre_completo) VALUES ($1::uuid, $2::uuid, $3)",
    )
    .bind(&id)
    .bind(organizacion_id)
    .bind(nombre)
    .execute(&mut **tx)
    .await
    .expect("user must insert");
    id
}

async fn rol(tx: &mut Transaction<'_, Postgres>, organizacion_id: &str, nombre: &str) -> String {
    let id = nuevo_uuid();
    sqlx::query(
        "INSERT INTO public.roles (id, organizacion_id, nombre) VALUES ($1::uuid, $2::uuid, $3)",
    )
    .bind(&id)
    .bind(organizacion_id)
    .bind(nombre)
    .execute(&mut **tx)
    .await
    .expect("role must insert");
    id
}

async fn permiso(tx: &mut Transaction<'_, Postgres>, codigo: &str) -> String {
    sqlx::query_scalar("SELECT id::text FROM public.permisos WHERE codigo = $1")
        .bind(codigo)
        .fetch_one(&mut **tx)
        .await
        .expect("canonical permission must exist")
}

async fn identidad(
    tx: &mut Transaction<'_, Postgres>,
    usuario_id: &str,
    sujeto: &str,
    desde: &str,
    hasta: Option<&str>,
) -> Result<String, sqlx::Error> {
    let id = nuevo_uuid();
    sqlx::query(
        "INSERT INTO public.identidades_autenticacion_externas (id, usuario_id, proveedor, sujeto_proveedor, vinculada_en, desvinculada_en) VALUES ($1::uuid, $2::uuid, 'supabase', $3::uuid, $4::timestamptz, $5::timestamptz)",
    )
    .bind(&id)
    .bind(usuario_id)
    .bind(sujeto)
    .bind(desde)
    .bind(hasta)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

async fn usuario_rol(
    tx: &mut Transaction<'_, Postgres>,
    usuario_id: &str,
    rol_id: &str,
    desde: &str,
    hasta: Option<&str>,
) -> Result<String, sqlx::Error> {
    let id = nuevo_uuid();
    sqlx::query(
        "INSERT INTO public.usuarios_roles (id, usuario_id, rol_id, vigente_desde, vigente_hasta) VALUES ($1::uuid, $2::uuid, $3::uuid, $4::timestamptz, $5::timestamptz)",
    )
    .bind(&id)
    .bind(usuario_id)
    .bind(rol_id)
    .bind(desde)
    .bind(hasta)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

async fn rol_permiso(
    tx: &mut Transaction<'_, Postgres>,
    rol_id: &str,
    permiso_id: &str,
    desde: &str,
    hasta: Option<&str>,
) -> Result<String, sqlx::Error> {
    let id = nuevo_uuid();
    sqlx::query(
        "INSERT INTO public.roles_permisos (id, rol_id, permiso_id, vigente_desde, vigente_hasta) VALUES ($1::uuid, $2::uuid, $3::uuid, $4::timestamptz, $5::timestamptz)",
    )
    .bind(&id)
    .bind(rol_id)
    .bind(permiso_id)
    .bind(desde)
    .bind(hasta)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

#[tokio::test]
async fn materializes_relations_catalog_and_approved_primary_key_names() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let relation_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM pg_tables WHERE schemaname = 'public' AND tablename IN ('organizaciones', 'usuarios', 'identidades_autenticacion_externas', 'roles', 'permisos', 'usuarios_roles', 'roles_permisos')",
    )
    .fetch_one(&db)
    .await
    .expect("relations must be queryable");
    assert_eq!(relation_count, 7);

    let codes: Vec<String> =
        sqlx::query_scalar("SELECT codigo FROM public.permisos ORDER BY codigo")
            .fetch_all(&db)
            .await
            .expect("permissions must be queryable");
    let mut expected_codes = vec![
        "panel:ver",
        "agricultura:ver",
        "agricultura:crear",
        "agricultura:editar",
        "inventario:ver",
        "inventario:ajustar",
        "ganaderia:ver",
        "ganaderia:editar",
        "maquinaria:ver",
        "maquinaria:editar",
        "comercial:ver",
        "comercial:editar",
        "configuracion:ver",
        "configuracion:administrar",
        "consola_tecnica:ver",
        "consola_tecnica:administrar",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    expected_codes.sort();
    assert_eq!(codes, expected_codes);
    let distinct_codes: i64 =
        sqlx::query_scalar("SELECT COUNT(DISTINCT codigo)::bigint FROM public.permisos")
            .fetch_one(&db)
            .await
            .expect("permission uniqueness must be queryable");
    assert_eq!(distinct_codes, 16);

    let primary_keys: Vec<String> = sqlx::query_scalar(
        "SELECT constraint_name FROM information_schema.table_constraints WHERE table_schema = 'public' AND constraint_type = 'PRIMARY KEY' AND table_name IN ('organizaciones', 'usuarios', 'identidades_autenticacion_externas', 'roles', 'permisos', 'usuarios_roles', 'roles_permisos') ORDER BY constraint_name",
    )
    .fetch_all(&db)
    .await
    .expect("primary keys must be queryable");
    assert_eq!(
        primary_keys,
        vec![
            "pk_identidades_autenticacion_externas",
            "pk_organizaciones",
            "pk_permisos",
            "pk_roles",
            "pk_roles_permisos",
            "pk_usuarios",
            "pk_usuarios_roles",
        ]
    );
    let email_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM information_schema.columns WHERE table_schema = 'public' AND table_name IN ('usuarios', 'identidades_autenticacion_externas') AND column_name = 'email'",
    )
    .fetch_one(&db)
    .await
    .expect("columns must be queryable");
    assert_eq!(email_columns, 0);
}

#[tokio::test]
async fn authorization_protection_trigger_functions_use_a_fixed_catalog_search_path() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let search_paths: Vec<(String, String)> = sqlx::query_as(
        "SELECT p.proname, array_to_string(p.proconfig, ',') FROM pg_proc AS p INNER JOIN pg_namespace AS n ON n.oid = p.pronamespace WHERE n.nspname = 'public' AND p.proname IN ('impedir_cambio_organizacion_usuario', 'impedir_cambio_organizacion_rol', 'impedir_cambio_codigo_permiso', 'proteger_historia_identidad_auth', 'proteger_historia_usuario_rol', 'proteger_historia_rol_permiso', 'impedir_truncate_historia_autorizacion') ORDER BY p.proname",
    )
    .fetch_all(&db)
    .await
    .expect("authorization protection function settings must be queryable");

    assert_eq!(
        search_paths,
        vec![
            (
                "impedir_cambio_codigo_permiso".to_owned(),
                "search_path=pg_catalog".to_owned()
            ),
            (
                "impedir_cambio_organizacion_rol".to_owned(),
                "search_path=pg_catalog".to_owned()
            ),
            (
                "impedir_cambio_organizacion_usuario".to_owned(),
                "search_path=pg_catalog".to_owned()
            ),
            (
                "impedir_truncate_historia_autorizacion".to_owned(),
                "search_path=pg_catalog".to_owned()
            ),
            (
                "proteger_historia_identidad_auth".to_owned(),
                "search_path=pg_catalog".to_owned()
            ),
            (
                "proteger_historia_rol_permiso".to_owned(),
                "search_path=pg_catalog".to_owned()
            ),
            (
                "proteger_historia_usuario_rol".to_owned(),
                "search_path=pg_catalog".to_owned()
            ),
        ]
    );
}

#[tokio::test]
async fn enforces_foreign_keys_organization_consistency_and_restrictive_deletion() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let mut tx = db.begin().await.expect("test transaction must begin");
    assert_statement_fails!(
        tx,
        sqlx::query("INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1::uuid, 'Sin organizacion')")
            .bind(nuevo_uuid())
            .execute(&mut *tx),
        FK,
        None
    );
    let org_a = organizar(&mut tx, "Organizacion A").await;
    let org_b = organizar(&mut tx, "Organizacion B").await;
    let user_a = usuario(&mut tx, &org_a, "Usuario A").await;
    let role_a = rol(&mut tx, &org_a, "Rol A").await;
    let role_b = rol(&mut tx, &org_b, "Rol B").await;
    let permission = permiso(&mut tx, "panel:ver").await;
    usuario_rol(&mut tx, &user_a, &role_a, "2026-01-01T00:00:00Z", None)
        .await
        .expect("same organization assignment must work");
    assert_statement_fails!(
        tx,
        usuario_rol(&mut tx, &user_a, &role_b, "2026-01-01T00:00:00Z", None),
        RAISE,
        Some(ORG_MISMATCH)
    );
    rol_permiso(&mut tx, &role_a, &permission, "2026-01-01T00:00:00Z", None)
        .await
        .expect("role permission must work");
    assert_statement_fails!(
        tx,
        sqlx::query("UPDATE public.usuarios SET organizacion_id = $1::uuid WHERE id = $2::uuid")
            .bind(&org_b)
            .bind(&user_a)
            .execute(&mut *tx),
        RAISE,
        None
    );
    assert_statement_fails!(
        tx,
        sqlx::query("UPDATE public.roles SET organizacion_id = $1::uuid WHERE id = $2::uuid")
            .bind(&org_b)
            .bind(&role_a)
            .execute(&mut *tx),
        RAISE,
        None
    );
    for query in [
        "DELETE FROM public.usuarios WHERE id = $1::uuid",
        "DELETE FROM public.roles WHERE id = $1::uuid",
        "DELETE FROM public.permisos WHERE id = $1::uuid",
        "DELETE FROM public.organizaciones WHERE id = $1::uuid",
    ] {
        let id = if query.contains("usuarios") {
            &user_a
        } else if query.contains("roles") {
            &role_a
        } else if query.contains("permisos") {
            &permission
        } else {
            &org_a
        };
        assert_statement_fails!(tx, sqlx::query(query).bind(id).execute(&mut *tx), FK, None);
    }
    tx.rollback()
        .await
        .expect("test transaction must roll back");
}

#[tokio::test]
async fn rejects_cross_organization_assignment_under_hostile_search_path() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let mut tx = db.begin().await.expect("test transaction must begin");
    let org_a = organizar(&mut tx, "Organizacion publica A").await;
    let org_b = organizar(&mut tx, "Organizacion publica B").await;
    let user_id = usuario(&mut tx, &org_a, "Usuario publico").await;
    let role_id = rol(&mut tx, &org_b, "Rol publico").await;
    let hostile = format!("hostile_{}", Uuid::new_v4().simple());
    sqlx::query(AssertSqlSafe(format!("CREATE SCHEMA {hostile}")))
        .execute(&mut *tx)
        .await
        .expect("hostile schema must be created");
    for relation in ["usuarios", "roles"] {
        sqlx::query(AssertSqlSafe(format!(
            "CREATE TABLE {hostile}.{relation} (id UUID PRIMARY KEY, organizacion_id UUID NOT NULL)"
        )))
        .execute(&mut *tx)
        .await
        .expect("hostile shadow relation must be created");
    }
    sqlx::query(AssertSqlSafe(format!(
        "INSERT INTO {hostile}.usuarios (id, organizacion_id) VALUES ($1::uuid, $2::uuid)"
    )))
    .bind(&user_id)
    .bind(&org_a)
    .execute(&mut *tx)
    .await
    .expect("hostile shadow user must insert");
    sqlx::query(AssertSqlSafe(format!(
        "INSERT INTO {hostile}.roles (id, organizacion_id) VALUES ($1::uuid, $2::uuid)"
    )))
    .bind(&role_id)
    .bind(&org_a)
    .execute(&mut *tx)
    .await
    .expect("hostile shadow role must insert");
    sqlx::query(AssertSqlSafe(format!(
        "SET LOCAL search_path TO {hostile}, public"
    )))
    .execute(&mut *tx)
    .await
    .expect("hostile search path must be set");
    assert_statement_fails!(
        tx,
        sqlx::query("INSERT INTO public.usuarios_roles (usuario_id, rol_id, vigente_desde) VALUES ($1::uuid, $2::uuid, '2026-01-01T00:00:00Z'::timestamptz)")
            .bind(&user_id)
            .bind(&role_id)
            .execute(&mut *tx),
        RAISE,
        Some(ORG_MISMATCH)
    );
    tx.rollback()
        .await
        .expect("test transaction must roll back");
}

#[tokio::test]
async fn protects_history_from_truncate_and_enforces_external_identity_temporal_integrity() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let mut tx = db.begin().await.expect("test transaction must begin");
    let org = organizar(&mut tx, "Organizacion identidades").await;
    let user_a = usuario(&mut tx, &org, "Usuario identidad A").await;
    let user_b = usuario(&mut tx, &org, "Usuario identidad B").await;
    let role = rol(&mut tx, &org, "Rol identidad").await;
    let permission = permiso(&mut tx, "panel:ver").await;
    let subject_a = nuevo_uuid();
    let subject_b = nuevo_uuid();
    let identity_a = identidad(&mut tx, &user_a, &subject_a, "2026-01-10T00:00:00Z", None)
        .await
        .expect("initial identity must work");
    assert_statement_fails!(
        tx,
        identidad(
            &mut tx,
            &user_a,
            &subject_b,
            "2026-01-15T00:00:00Z",
            Some("2026-01-25T00:00:00Z")
        ),
        EXCLUSION,
        None
    );
    assert_statement_fails!(
        tx,
        identidad(&mut tx, &user_b, &subject_a, "2026-01-10T00:00:00Z", None),
        UNIQUE,
        None
    );
    sqlx::query("UPDATE public.identidades_autenticacion_externas SET desvinculada_en = '2026-02-01T00:00:00Z'::timestamptz WHERE id = $1::uuid")
        .bind(&identity_a)
        .execute(&mut *tx)
        .await
        .expect("identity closure must work");
    assert_statement_fails!(
        tx,
        sqlx::query("UPDATE public.identidades_autenticacion_externas SET desvinculada_en = NULL WHERE id = $1::uuid")
            .bind(&identity_a)
            .execute(&mut *tx),
        RAISE,
        Some(IDENTITY_IMMUTABLE)
    );
    identidad(&mut tx, &user_a, &subject_b, "2026-02-01T00:00:00Z", None)
        .await
        .expect("adjacent identity replacement must work");
    let user_role = usuario_rol(&mut tx, &user_b, &role, "2026-01-01T00:00:00Z", None)
        .await
        .expect("assignment for truncate guard must work");
    let role_permission = rol_permiso(&mut tx, &role, &permission, "2026-01-01T00:00:00Z", None)
        .await
        .expect("grant for truncate guard must work");
    for relation in [
        "public.identidades_autenticacion_externas",
        "public.usuarios_roles",
        "public.roles_permisos",
    ] {
        assert_statement_fails!(
            tx,
            sqlx::query(AssertSqlSafe(format!("TRUNCATE {relation}"))).execute(&mut *tx),
            RAISE,
            Some(TRUNCATE_FORBIDDEN)
        );
    }
    let identity_rows: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM public.identidades_autenticacion_externas WHERE usuario_id = $1::uuid")
        .bind(&user_a)
        .fetch_one(&mut *tx)
        .await
        .expect("identity rows must remain");
    let user_role_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.usuarios_roles WHERE id = $1::uuid",
    )
    .bind(&user_role)
    .fetch_one(&mut *tx)
    .await
    .expect("assignment row must remain");
    let role_permission_rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.roles_permisos WHERE id = $1::uuid",
    )
    .bind(&role_permission)
    .fetch_one(&mut *tx)
    .await
    .expect("grant row must remain");
    assert_eq!(
        (identity_rows, user_role_rows, role_permission_rows),
        (2, 1, 1)
    );
    tx.rollback()
        .await
        .expect("test transaction must roll back");
}

#[tokio::test]
async fn rejects_overlapping_usuario_rol_periods_and_preserves_closed_history() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let mut tx = db.begin().await.expect("test transaction must begin");
    let org = organizar(&mut tx, "Organizacion temporal usuario rol").await;
    let user = usuario(&mut tx, &org, "Usuario temporal").await;
    let user_alt = usuario(&mut tx, &org, "Usuario alterno").await;
    let role = rol(&mut tx, &org, "Rol temporal").await;
    let role_alt = rol(&mut tx, &org, "Rol alterno").await;
    let first = usuario_rol(&mut tx, &user, &role, "2026-01-10T00:00:00Z", None)
        .await
        .expect("initial assignment must work");
    sqlx::query("UPDATE public.usuarios_roles SET vigente_hasta = '2026-01-20T00:00:00Z'::timestamptz WHERE id = $1::uuid")
        .bind(&first)
        .execute(&mut *tx)
        .await
        .expect("closing an open assignment must work");
    assert_statement_fails!(
        tx,
        sqlx::query("UPDATE public.usuarios_roles SET vigente_hasta = NULL WHERE id = $1::uuid")
            .bind(&first)
            .execute(&mut *tx),
        RAISE,
        Some(USER_ROLE_IMMUTABLE)
    );
    assert_statement_fails!(tx, sqlx::query("UPDATE public.usuarios_roles SET vigente_desde = '2026-01-09T00:00:00Z'::timestamptz WHERE id = $1::uuid").bind(&first).execute(&mut *tx), RAISE, Some(USER_ROLE_IMMUTABLE));
    assert_statement_fails!(
        tx,
        sqlx::query("UPDATE public.usuarios_roles SET usuario_id = $1::uuid WHERE id = $2::uuid")
            .bind(&user_alt)
            .bind(&first)
            .execute(&mut *tx),
        RAISE,
        Some(USER_ROLE_IMMUTABLE)
    );
    assert_statement_fails!(
        tx,
        sqlx::query("UPDATE public.usuarios_roles SET rol_id = $1::uuid WHERE id = $2::uuid")
            .bind(&role_alt)
            .bind(&first)
            .execute(&mut *tx),
        RAISE,
        Some(USER_ROLE_IMMUTABLE)
    );
    for (desde, hasta) in [
        ("2026-01-10T00:00:00Z", "2026-01-20T00:00:00Z"),
        ("2026-01-12T00:00:00Z", "2026-01-18T00:00:00Z"),
        ("2026-01-15T00:00:00Z", "2026-01-25T00:00:00Z"),
    ] {
        assert_statement_fails!(
            tx,
            usuario_rol(&mut tx, &user, &role, desde, Some(hasta)),
            EXCLUSION,
            None
        );
    }
    usuario_rol(
        &mut tx,
        &user,
        &role,
        "2026-01-20T00:00:00Z",
        Some("2026-03-01T00:00:00Z"),
    )
    .await
    .expect("adjacent assignment must work");
    let current = usuario_rol(&mut tx, &user, &role, "2026-03-01T00:00:00Z", None)
        .await
        .expect("later re-grant must work");
    assert_statement_fails!(
        tx,
        usuario_rol(
            &mut tx,
            &user,
            &role,
            "2026-04-01T00:00:00Z",
            Some("2026-05-01T00:00:00Z")
        ),
        EXCLUSION,
        None
    );
    assert_statement_fails!(
        tx,
        sqlx::query("DELETE FROM public.usuarios_roles WHERE id = $1::uuid")
            .bind(&current)
            .execute(&mut *tx),
        RAISE,
        Some(USER_ROLE_DELETE)
    );
    tx.rollback()
        .await
        .expect("test transaction must roll back");
}

#[tokio::test]
async fn rejects_overlapping_rol_permiso_periods_and_preserves_closed_history() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let mut tx = db.begin().await.expect("test transaction must begin");
    let org = organizar(&mut tx, "Organizacion temporal rol permiso").await;
    let role = rol(&mut tx, &org, "Rol temporal permiso").await;
    let role_alt = rol(&mut tx, &org, "Rol alterno permiso").await;
    let permission = permiso(&mut tx, "panel:ver").await;
    let permission_alt = permiso(&mut tx, "agricultura:ver").await;
    let first = rol_permiso(&mut tx, &role, &permission, "2026-01-10T00:00:00Z", None)
        .await
        .expect("initial grant must work");
    sqlx::query("UPDATE public.roles_permisos SET vigente_hasta = '2026-01-20T00:00:00Z'::timestamptz WHERE id = $1::uuid")
        .bind(&first)
        .execute(&mut *tx)
        .await
        .expect("closing an open grant must work");
    assert_statement_fails!(
        tx,
        sqlx::query("UPDATE public.roles_permisos SET vigente_hasta = NULL WHERE id = $1::uuid")
            .bind(&first)
            .execute(&mut *tx),
        RAISE,
        Some(ROLE_PERMISSION_IMMUTABLE)
    );
    assert_statement_fails!(tx, sqlx::query("UPDATE public.roles_permisos SET vigente_desde = '2026-01-09T00:00:00Z'::timestamptz WHERE id = $1::uuid").bind(&first).execute(&mut *tx), RAISE, Some(ROLE_PERMISSION_IMMUTABLE));
    assert_statement_fails!(
        tx,
        sqlx::query("UPDATE public.roles_permisos SET rol_id = $1::uuid WHERE id = $2::uuid")
            .bind(&role_alt)
            .bind(&first)
            .execute(&mut *tx),
        RAISE,
        Some(ROLE_PERMISSION_IMMUTABLE)
    );
    assert_statement_fails!(
        tx,
        sqlx::query("UPDATE public.roles_permisos SET permiso_id = $1::uuid WHERE id = $2::uuid")
            .bind(&permission_alt)
            .bind(&first)
            .execute(&mut *tx),
        RAISE,
        Some(ROLE_PERMISSION_IMMUTABLE)
    );
    for (desde, hasta) in [
        ("2026-01-10T00:00:00Z", "2026-01-20T00:00:00Z"),
        ("2026-01-12T00:00:00Z", "2026-01-18T00:00:00Z"),
        ("2026-01-15T00:00:00Z", "2026-01-25T00:00:00Z"),
    ] {
        assert_statement_fails!(
            tx,
            rol_permiso(&mut tx, &role, &permission, desde, Some(hasta)),
            EXCLUSION,
            None
        );
    }
    rol_permiso(
        &mut tx,
        &role,
        &permission,
        "2026-01-20T00:00:00Z",
        Some("2026-03-01T00:00:00Z"),
    )
    .await
    .expect("adjacent grant must work");
    let current = rol_permiso(&mut tx, &role, &permission, "2026-03-01T00:00:00Z", None)
        .await
        .expect("later re-grant must work");
    assert_statement_fails!(
        tx,
        rol_permiso(
            &mut tx,
            &role,
            &permission,
            "2026-04-01T00:00:00Z",
            Some("2026-05-01T00:00:00Z")
        ),
        EXCLUSION,
        None
    );
    assert_statement_fails!(
        tx,
        sqlx::query("DELETE FROM public.roles_permisos WHERE id = $1::uuid")
            .bind(&current)
            .execute(&mut *tx),
        RAISE,
        Some(ROLE_PERMISSION_DELETE)
    );
    tx.rollback()
        .await
        .expect("test transaction must roll back");
}
