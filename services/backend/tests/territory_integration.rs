use std::sync::OnceLock;

use agro_ops_backend::{
    authorization::{
        self,
        permission_codes::{AGRICULTURA_CREAR, TERRITORIO_CREAR},
    },
    external_references::{self, ExternalEntityType, ExternalId},
    territory::{
        application::{self, TerritoryApplicationError},
        domain::{
            CanonicalTerritorialCode, FunctionalName, GeometryProvenance, GeometryWkt,
            NewEstablecimiento, NewLoteBase,
        },
        infrastructure::PostgresTerritoryStore,
    },
};
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use uuid::Uuid;

const CHECK: &str = "23514";
const FOREIGN_KEY: &str = "23503";
const NOT_NULL: &str = "23502";
const RAISE: &str = "P0001";
const UNIQUE: &str = "23505";

fn database_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn test_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(4)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
        .await
        .expect("PostgreSQL with migrations must be available")
}

async fn principal(db: &PgPool, permission_codes: &[&str]) -> (Uuid, Uuid, Uuid) {
    let organization_id: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(format!("Organización territorio {}", Uuid::new_v4()))
            .fetch_one(db)
            .await
            .expect("organization must insert");
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
    )
    .bind(organization_id)
    .bind(format!("Usuario territorio {}", Uuid::new_v4()))
    .fetch_one(db)
    .await
    .expect("user must insert");
    let subject = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, 'supabase', $2)",
    )
    .bind(user_id)
    .bind(subject)
    .execute(db)
    .await
    .expect("identity must insert");

    if !permission_codes.is_empty() {
        let role_id: Uuid = sqlx::query_scalar(
            "INSERT INTO public.roles (organizacion_id, nombre) VALUES ($1, $2) RETURNING id",
        )
        .bind(organization_id)
        .bind(format!("Operador territorial {}", Uuid::new_v4()))
        .fetch_one(db)
        .await
        .expect("role must insert");
        sqlx::query("INSERT INTO public.usuarios_roles (usuario_id, rol_id) VALUES ($1, $2)")
            .bind(user_id)
            .bind(role_id)
            .execute(db)
            .await
            .expect("role must be assigned");
        for permission_code in permission_codes {
            sqlx::query(
                "INSERT INTO public.roles_permisos (rol_id, permiso_id) SELECT $1, id FROM public.permisos WHERE codigo = $2",
            )
            .bind(role_id)
            .bind(permission_code)
            .execute(db)
            .await
            .expect("permission must be granted");
        }
    }
    (organization_id, user_id, subject)
}

const ESTABLECIMIENTO_GEOMETRY: &str =
    "POLYGON((-60 -34, -60 -34.1, -59.9 -34.1, -59.9 -34, -60 -34))";

fn new_establecimiento(codigo: &str) -> NewEstablecimiento {
    NewEstablecimiento {
        codigo: CanonicalTerritorialCode::new(codigo).expect("code must be valid"),
        nombre: FunctionalName::new("Establecimiento Las Tunas").expect("name must be valid"),
        geometria_wkt: GeometryWkt::new(ESTABLECIMIENTO_GEOMETRY)
            .expect("establishment WKT boundary value must be valid"),
        origen_geometria: GeometryProvenance::Manual,
        renspa: ExternalId::new(format!("RENSPA-{codigo}")).expect("RENSPA must be opaque text"),
    }
}

fn new_lote_base(establecimiento_id: Uuid, codigo: &str, geometry: &str) -> NewLoteBase {
    NewLoteBase {
        establecimiento_id,
        codigo: CanonicalTerritorialCode::new(codigo).expect("code must be valid"),
        nombre: FunctionalName::new("Lote Norte").expect("name must be valid"),
        geometria_wkt: GeometryWkt::new(geometry).expect("WKT boundary value must be valid"),
    }
}

async fn context(db: &PgPool, subject: Uuid) -> authorization::AuthorizationContext {
    authorization::resolve_context(db, subject)
        .await
        .expect("principal must resolve")
}

fn assert_database_error(
    error: sqlx::Error,
    expected_sqlstate: &str,
    expected_message: Option<&str>,
) {
    let database_error = error
        .as_database_error()
        .expect("expected PostgreSQL database error");
    assert_eq!(database_error.code().as_deref(), Some(expected_sqlstate));
    if let Some(expected_message) = expected_message {
        assert_eq!(database_error.message(), expected_message);
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

#[tokio::test]
async fn creates_persists_and_retrieves_establecimiento_and_lote_base_with_postgis_geometry() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let (organization_id, user_id, subject) = principal(&db, &[TERRITORIO_CREAR]).await;
    let territory_context = context(&db, subject).await;
    let store = PostgresTerritoryStore::new(db.clone());

    let establecimiento = application::create_establecimiento(
        &store,
        &territory_context,
        new_establecimiento("LAS_TUNAS"),
    )
    .await
    .expect("establecimiento must be created");
    assert_eq!(establecimiento.organization_id, organization_id);
    assert_eq!(establecimiento.creado_por, user_id);
    assert!(establecimiento.activo);
    assert_eq!(establecimiento.origen_geometria, GeometryProvenance::Manual);
    assert!(
        establecimiento
            .geometria_ewkt
            .starts_with("SRID=4326;MULTIPOLYGON")
    );

    let persisted_establecimiento: (String, i32, bool, bool, String) = sqlx::query_as(
        "SELECT GeometryType(geometria), ST_SRID(geometria), ST_IsValid(geometria), ST_IsEmpty(geometria), origen_geometria FROM public.establecimientos WHERE id = $1",
    )
    .bind(establecimiento.id)
    .fetch_one(&db)
    .await
    .expect("persisted establishment perimeter must be queryable");
    assert_eq!(
        persisted_establecimiento,
        (
            "MULTIPOLYGON".to_owned(),
            4326,
            true,
            false,
            "manual".to_owned()
        )
    );
    let lotes_iniciales: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.lotes_base WHERE establecimiento_id = $1",
    )
    .bind(establecimiento.id)
    .fetch_one(&db)
    .await
    .expect("base plot count must be queryable");
    assert_eq!(lotes_iniciales, 0);
    let referencias_renspa = external_references::list_for_entity(
        &db,
        Some(organization_id),
        &ExternalEntityType::new("establecimiento").expect("entity type must be valid"),
        establecimiento.id,
    )
    .await
    .expect("RENSPA reference must be retrievable");
    assert_eq!(referencias_renspa.len(), 1);
    assert_eq!(referencias_renspa[0].entity_id, establecimiento.id);
    assert_eq!(referencias_renspa[0].system.as_str(), "senasa");
    assert_eq!(
        referencias_renspa[0].external_id.as_str(),
        "RENSPA-LAS_TUNAS"
    );

    let lote_base = application::create_lote_base(
        &store,
        &territory_context,
        new_lote_base(
            establecimiento.id,
            "NORTE",
            "POLYGON((-60 -34, -60 -34.01, -59.99 -34.01, -59.99 -34, -60 -34))",
        ),
    )
    .await
    .expect("lote base must be created");
    assert_eq!(lote_base.organization_id, organization_id);
    assert_eq!(lote_base.establecimiento_id, establecimiento.id);
    assert_eq!(lote_base.creado_por, user_id);
    assert!(
        lote_base
            .geometria_ewkt
            .starts_with("SRID=4326;MULTIPOLYGON")
    );

    let persisted: (String, i32, bool, bool) = sqlx::query_as(
        "SELECT GeometryType(geometria), ST_SRID(geometria), ST_IsValid(geometria), ST_IsEmpty(geometria) FROM public.lotes_base WHERE id = $1",
    )
    .bind(lote_base.id)
    .fetch_one(&db)
    .await
    .expect("persisted geometry must be queryable");
    assert_eq!(persisted, ("MULTIPOLYGON".to_owned(), 4326, true, false));

    let lote_en_limite = application::create_lote_base(
        &store,
        &territory_context,
        new_lote_base(
            establecimiento.id,
            "LIMITE",
            "POLYGON((-60 -34.02, -60 -34.03, -59.99 -34.03, -59.99 -34.02, -60 -34.02))",
        ),
    )
    .await
    .expect("a base plot touching the establishment boundary must be valid");
    assert_eq!(lote_en_limite.establecimiento_id, establecimiento.id);

    let audit_actions: Vec<String> = sqlx::query_scalar(
        "SELECT accion FROM public.audit_events WHERE organizacion_id = $1 AND entidad_id IN ($2, $3, $4) ORDER BY accion",
    )
    .bind(organization_id)
    .bind(establecimiento.id)
    .bind(lote_base.id)
    .bind(lote_en_limite.id)
    .fetch_all(&db)
    .await
    .expect("territorial creation audit must be queryable");
    assert_eq!(
        audit_actions,
        vec![
            "establecimiento.creado",
            "lote_base.creado",
            "lote_base.creado"
        ]
    );
}

#[tokio::test]
async fn territorial_creation_requires_territorio_crear_and_not_agricultura_crear() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let (organization_id, _, subject) = principal(&db, &[]).await;
    let unprivileged_context = context(&db, subject).await;
    let store = PostgresTerritoryStore::new(db.clone());

    let error = application::create_establecimiento(
        &store,
        &unprivileged_context,
        new_establecimiento("SIN_PERMISO"),
    )
    .await
    .expect_err("creation without the capability must fail");
    assert_eq!(error, TerritoryApplicationError::PermissionDenied);
    let persisted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.establecimientos WHERE organizacion_id = $1",
    )
    .bind(organization_id)
    .fetch_one(&db)
    .await
    .expect("persistence must be queryable");
    assert_eq!(persisted, 0);

    let (agriculture_organization_id, _, agriculture_subject) =
        principal(&db, &[AGRICULTURA_CREAR]).await;
    let agriculture_context = context(&db, agriculture_subject).await;
    let agriculture_error = application::create_establecimiento(
        &store,
        &agriculture_context,
        new_establecimiento("SOLO_AGRICULTURA"),
    )
    .await
    .expect_err("agriculture creation alone must not authorize territory creation");
    assert_eq!(
        agriculture_error,
        TerritoryApplicationError::PermissionDenied
    );
    let agriculture_persisted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.establecimientos WHERE organizacion_id = $1",
    )
    .bind(agriculture_organization_id)
    .fetch_one(&db)
    .await
    .expect("persistence must be queryable");
    assert_eq!(agriculture_persisted, 0);
}

#[tokio::test]
async fn database_enforces_territorial_organization_identity_and_geometry_invariants() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let (organization_a, user_a, _) = principal(&db, &[]).await;
    let (organization_b, user_b, _) = principal(&db, &[]).await;
    let establecimiento_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO public.establecimientos (id, organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, $2, 'CAMPO_A', 'Campo A', ST_Multi(ST_GeomFromText($3, 4326)), 'manual', $4)",
    )
    .bind(establecimiento_id)
    .bind(organization_a)
    .bind(ESTABLECIMIENTO_GEOMETRY)
    .bind(user_a)
    .execute(&db)
    .await
    .expect("base establishment must insert");
    sqlx::query(
        "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'UNICO', 'Lote único', ST_GeomFromText('MULTIPOLYGON(((-60 -34, -60 -34.01, -59.99 -34.01, -59.99 -34, -60 -34)))', 4326), $3)",
    )
    .bind(organization_a)
    .bind(establecimiento_id)
    .bind(user_a)
    .execute(&db)
    .await
    .expect("base plot must insert");

    let corrected_lote_geometry: String = sqlx::query_scalar(
        "UPDATE public.lotes_base SET geometria = ST_GeomFromText('MULTIPOLYGON(((-59.98 -34.02, -59.98 -34.03, -59.97 -34.03, -59.97 -34.02, -59.98 -34.02)))', 4326) WHERE establecimiento_id = $1 AND codigo = 'UNICO' RETURNING ST_AsEWKT(geometria)",
    )
    .bind(establecimiento_id)
    .fetch_one(&db)
    .await
    .expect("a valid contained base plot correction must remain possible");
    assert!(corrected_lote_geometry.starts_with("SRID=4326;MULTIPOLYGON"));
    let corrected_establecimiento_geometry: String = sqlx::query_scalar(
        "UPDATE public.establecimientos SET geometria = ST_Multi(ST_GeomFromText('POLYGON((-60 -34, -60 -34.11, -59.9 -34.11, -59.9 -34, -60 -34))', 4326)) WHERE id = $1 RETURNING ST_AsEWKT(geometria)",
    )
    .bind(establecimiento_id)
    .fetch_one(&db)
    .await
    .expect("a perimeter correction that still covers every base plot must remain possible");
    assert!(corrected_establecimiento_geometry.starts_with("SRID=4326;MULTIPOLYGON"));

    let mut transaction: Transaction<'_, Postgres> =
        db.begin().await.expect("transaction must begin");
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.establecimientos (organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, 'CAMPO_A', 'Duplicado', ST_Multi(ST_GeomFromText($2, 4326)), 'manual', $3)",
        )
        .bind(organization_a)
        .bind(ESTABLECIMIENTO_GEOMETRY)
        .bind(user_a)
        .execute(&mut *transaction),
        UNIQUE,
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "UPDATE public.establecimientos SET geometria = ST_Multi(ST_GeomFromText('POLYGON((-60 -34, -60 -34.11, -59.975 -34.11, -59.975 -34, -60 -34))', 4326)) WHERE id = $1",
        )
        .bind(establecimiento_id)
        .execute(&mut *transaction),
        RAISE,
        Some("agro_ops_establecimiento_no_cubre_lotes_base")
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.establecimientos (organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, 'EST_SIN_GEOMETRIA', 'Sin geometría', NULL, 'manual', $2)",
        )
        .bind(organization_a)
        .bind(user_a)
        .execute(&mut *transaction),
        NOT_NULL,
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.establecimientos (organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, 'EST_VACIO', 'Vacío', ST_GeomFromText('MULTIPOLYGON EMPTY', 4326), 'manual', $2)",
        )
        .bind(organization_a)
        .bind(user_a)
        .execute(&mut *transaction),
        CHECK,
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.establecimientos (organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, 'EST_INVALIDO', 'Inválido', ST_GeomFromText('MULTIPOLYGON(((-60 -34, -59.99 -34.01, -59.99 -34, -60 -34.01, -60 -34)))', 4326), 'manual', $2)",
        )
        .bind(organization_a)
        .bind(user_a)
        .execute(&mut *transaction),
        CHECK,
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.establecimientos (organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, 'EST_SRID_INCORRECTO', 'SRID incorrecto', ST_GeomFromText('MULTIPOLYGON(((-60 -34, -60 -34.01, -59.99 -34.01, -59.99 -34, -60 -34)))', 3857), 'manual', $2)",
        )
        .bind(organization_a)
        .bind(user_a)
        .execute(&mut *transaction),
        "22023",
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'UNICO', 'Lote duplicado', ST_GeomFromText('MULTIPOLYGON(((-60 -34, -60 -34.01, -59.99 -34.01, -59.99 -34, -60 -34)))', 4326), $3)",
        )
        .bind(organization_a)
        .bind(establecimiento_id)
        .bind(user_a)
        .execute(&mut *transaction),
        UNIQUE,
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'NORTE', 'Lote cruzado', ST_GeomFromText('MULTIPOLYGON(((-60 -34, -60 -34.01, -59.99 -34.01, -59.99 -34, -60 -34)))', 4326), $3)",
        )
        .bind(organization_b)
        .bind(establecimiento_id)
        .bind(user_b)
        .execute(&mut *transaction),
        FOREIGN_KEY,
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'PARCIALMENTE_FUERA', 'Parcialmente fuera', ST_GeomFromText('MULTIPOLYGON(((-60.01 -34.02, -60.01 -34.03, -59.99 -34.03, -59.99 -34.02, -60.01 -34.02)))', 4326), $3)",
        )
        .bind(organization_a)
        .bind(establecimiento_id)
        .bind(user_a)
        .execute(&mut *transaction),
        RAISE,
        Some("agro_ops_lote_base_fuera_establecimiento")
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'TOTALMENTE_FUERA', 'Totalmente fuera', ST_GeomFromText('MULTIPOLYGON(((-60.2 -34.02, -60.2 -34.03, -60.19 -34.03, -60.19 -34.02, -60.2 -34.02)))', 4326), $3)",
        )
        .bind(organization_a)
        .bind(establecimiento_id)
        .bind(user_a)
        .execute(&mut *transaction),
        RAISE,
        Some("agro_ops_lote_base_fuera_establecimiento")
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'SIN_GEOMETRIA', 'Sin geometría', NULL, $3)",
        )
        .bind(organization_a)
        .bind(establecimiento_id)
        .bind(user_a)
        .execute(&mut *transaction),
        NOT_NULL,
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'VACIO', 'Vacío', ST_GeomFromText('MULTIPOLYGON EMPTY', 4326), $3)",
        )
        .bind(organization_a)
        .bind(establecimiento_id)
        .bind(user_a)
        .execute(&mut *transaction),
        CHECK,
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'INVALIDO', 'Inválido', ST_GeomFromText('MULTIPOLYGON(((-60 -34, -59.99 -34.01, -59.99 -34, -60 -34.01, -60 -34)))', 4326), $3)",
        )
        .bind(organization_a)
        .bind(establecimiento_id)
        .bind(user_a)
        .execute(&mut *transaction),
        CHECK,
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query(
            "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'SRID_INCORRECTO', 'SRID incorrecto', ST_GeomFromText('MULTIPOLYGON(((-60 -34, -60 -34.01, -59.99 -34.01, -59.99 -34, -60 -34)))', 3857), $3)",
        )
        .bind(organization_a)
        .bind(establecimiento_id)
        .bind(user_a)
        .execute(&mut *transaction),
        "22023",
        None
    );
    assert_statement_fails!(
        transaction,
        sqlx::query("DELETE FROM public.establecimientos WHERE id = $1")
            .bind(establecimiento_id)
            .execute(&mut *transaction),
        RAISE,
        Some("agro_ops_establecimiento_eliminacion_prohibida")
    );
    transaction.commit().await.expect("transaction must commit");
}
