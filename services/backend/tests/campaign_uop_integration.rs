use std::sync::OnceLock;

use sqlx::{AssertSqlSafe, PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use uuid::Uuid;

const CHECK: &str = "23514";
const FOREIGN_KEY: &str = "23503";
const RAISE: &str = "P0001";
const UOP_OUTSIDE_BASE_PLOTS: &str = "agro_ops_uop_fuera_lotes_base";
const UOP_INTERIOR_OVERLAP: &str = "agro_ops_uop_solapamiento_interior";

const FIELD: &str = "POLYGON((-60 -34, -60 -34.1, -59.9 -34.1, -59.9 -34, -60 -34))";
const BASE_A: &str = "POLYGON((-60 -34, -60 -34.1, -59.95 -34.1, -59.95 -34, -60 -34))";
const BASE_B: &str = "POLYGON((-59.95 -34, -59.95 -34.1, -59.9 -34.1, -59.9 -34, -59.95 -34))";
const BASE_A_LEFT: &str = "POLYGON((-60 -34, -60 -34.1, -59.975 -34.1, -59.975 -34, -60 -34))";
const BASE_A_RIGHT: &str =
    "POLYGON((-59.975 -34, -59.975 -34.1, -59.95 -34.1, -59.95 -34, -59.975 -34))";

#[derive(Clone, Copy)]
struct TerritorialFixture {
    organization_id: Uuid,
    user_id: Uuid,
    establishment_id: Uuid,
    base_plot_a_id: Uuid,
    base_plot_b_id: Uuid,
}

fn database_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn test_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(6)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
        .await
        .expect("PostgreSQL/PostGIS with migrations must be available")
}

async fn create_fixture(db: &PgPool) -> TerritorialFixture {
    let organization_id: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(format!("Organización campañas {}", Uuid::new_v4()))
            .fetch_one(db)
            .await
            .expect("organization must insert");
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
    )
    .bind(organization_id)
    .bind(format!("Usuario campañas {}", Uuid::new_v4()))
    .fetch_one(db)
    .await
    .expect("user must insert");
    let establishment_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.establecimientos (organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, 'CAMPO', 'Campo de prueba', ST_Multi(ST_GeomFromText($2, 4326)), 'manual', $3) RETURNING id",
    )
    .bind(organization_id)
    .bind(FIELD)
    .bind(user_id)
    .fetch_one(db)
    .await
    .expect("establishment must insert");
    let base_plot_a_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'A', 'Lote A', ST_Multi(ST_GeomFromText($3, 4326)), $4) RETURNING id",
    )
    .bind(organization_id)
    .bind(establishment_id)
    .bind(BASE_A)
    .bind(user_id)
    .fetch_one(db)
    .await
    .expect("base plot A must insert");
    let base_plot_b_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'B', 'Lote B', ST_Multi(ST_GeomFromText($3, 4326)), $4) RETURNING id",
    )
    .bind(organization_id)
    .bind(establishment_id)
    .bind(BASE_B)
    .bind(user_id)
    .fetch_one(db)
    .await
    .expect("base plot B must insert");

    TerritorialFixture {
        organization_id,
        user_id,
        establishment_id,
        base_plot_a_id,
        base_plot_b_id,
    }
}

async fn another_establishment(db: &PgPool, fixture: TerritorialFixture) -> (Uuid, Uuid) {
    let establishment_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.establecimientos (organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, 'OTRO_CAMPO', 'Otro campo', ST_Multi(ST_GeomFromText($2, 4326)), 'manual', $3) RETURNING id",
    )
    .bind(fixture.organization_id)
    .bind(FIELD)
    .bind(fixture.user_id)
    .fetch_one(db)
    .await
    .expect("second establishment must insert");
    let base_plot_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'A', 'Otro lote A', ST_Multi(ST_GeomFromText($3, 4326)), $4) RETURNING id",
    )
    .bind(fixture.organization_id)
    .bind(establishment_id)
    .bind(BASE_A)
    .bind(fixture.user_id)
    .fetch_one(db)
    .await
    .expect("second establishment base plot must insert");
    (establishment_id, base_plot_id)
}

async fn campaign(db: &PgPool, fixture: TerritorialFixture, code: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO public.campanas (organizacion_id, codigo, nombre, fecha_inicio, fecha_fin, creado_por) VALUES ($1, $2, $3, DATE '2026-07-01', DATE '2027-06-30', $4) RETURNING id",
    )
    .bind(fixture.organization_id)
    .bind(code)
    .bind(format!("Campaña {code}"))
    .bind(fixture.user_id)
    .fetch_one(db)
    .await
    .expect("campaign must insert")
}

async fn insert_uop(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: TerritorialFixture,
    campaign_id: Uuid,
    code: &str,
    geometry: &str,
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO public.unidades_operativas (organizacion_id, campana_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, $3, $4, $5, ST_Multi(ST_GeomFromText($6, 4326)), $7) RETURNING id",
    )
    .bind(fixture.organization_id)
    .bind(campaign_id)
    .bind(fixture.establishment_id)
    .bind(code)
    .bind(format!("UOP {code}"))
    .bind(geometry)
    .bind(fixture.user_id)
    .fetch_one(&mut **transaction)
    .await
    .expect("UOP must insert before deferred validation")
}

async fn link_base_plot(
    transaction: &mut Transaction<'_, Postgres>,
    fixture: TerritorialFixture,
    uop_id: Uuid,
    base_plot_id: Uuid,
) {
    sqlx::query(
        "INSERT INTO public.unidades_operativas_lotes_base (organizacion_id, establecimiento_id, unidad_operativa_id, lote_base_id, creado_por) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(fixture.organization_id)
    .bind(fixture.establishment_id)
    .bind(uop_id)
    .bind(base_plot_id)
    .bind(fixture.user_id)
    .execute(&mut **transaction)
    .await
    .expect("base plot link must insert before deferred validation");
}

async fn create_uop(
    db: &PgPool,
    fixture: TerritorialFixture,
    campaign_id: Uuid,
    code: &str,
    geometry: &str,
    base_plot_ids: &[Uuid],
) -> Uuid {
    let mut transaction = db.begin().await.expect("transaction must begin");
    let uop_id = insert_uop(&mut transaction, fixture, campaign_id, code, geometry).await;
    for base_plot_id in base_plot_ids {
        link_base_plot(&mut transaction, fixture, uop_id, *base_plot_id).await;
    }
    transaction
        .commit()
        .await
        .expect("valid UOP and all sequential links must commit");
    uop_id
}

fn assert_database_error(error: sqlx::Error, expected_sqlstate: &str, message: Option<&str>) {
    let database_error = error
        .as_database_error()
        .expect("expected PostgreSQL database error");
    assert_eq!(database_error.code().as_deref(), Some(expected_sqlstate));
    if let Some(message) = message {
        assert_eq!(database_error.message(), message);
    }
}

async fn assert_deferred_failure(transaction: Transaction<'_, Postgres>, message: &str) {
    let error = transaction
        .commit()
        .await
        .expect_err("invalid deferred territorial state must not commit");
    assert_database_error(error, RAISE, Some(message));
}

#[tokio::test]
async fn campaign_and_valid_boundary_touching_subdivision_persist_with_optional_coverage() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let fixture = create_fixture(&db).await;
    let campaign_id = campaign(&db, fixture, "CAMPANA_2026").await;

    let initial_uop_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.unidades_operativas WHERE campana_id = $1",
    )
    .bind(campaign_id)
    .fetch_one(&db)
    .await
    .expect("campaign UOP count must be queryable");
    assert_eq!(initial_uop_count, 0);

    let left_id = create_uop(
        &db,
        fixture,
        campaign_id,
        "A1",
        BASE_A_LEFT,
        &[fixture.base_plot_a_id],
    )
    .await;
    let right_id = create_uop(
        &db,
        fixture,
        campaign_id,
        "A2",
        BASE_A_RIGHT,
        &[fixture.base_plot_a_id],
    )
    .await;

    let persisted: Vec<(Uuid, String, i32, bool, bool)> = sqlx::query_as(
        "SELECT id, GeometryType(geometria), ST_SRID(geometria), ST_IsValid(geometria), ST_IsEmpty(geometria) FROM public.unidades_operativas WHERE id IN ($1, $2) ORDER BY codigo",
    )
    .bind(left_id)
    .bind(right_id)
    .fetch_all(&db)
    .await
    .expect("persisted UOP geometry must be queryable");
    assert_eq!(persisted.len(), 2);
    assert!(persisted.iter().all(|row| row.1 == "MULTIPOLYGON"));
    assert!(persisted.iter().all(|row| row.2 == 4326 && row.3 && !row.4));

    let unused_base_plot: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.unidades_operativas_lotes_base WHERE lote_base_id = $1",
    )
    .bind(fixture.base_plot_b_id)
    .fetch_one(&db)
    .await
    .expect("unused base plot state must be queryable");
    assert_eq!(unused_base_plot, 0);
}

#[tokio::test]
async fn database_rejects_invalid_uop_geometry_and_same_campaign_interior_overlap() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let fixture = create_fixture(&db).await;
    let campaign_id = campaign(&db, fixture, "SOLAPAMIENTOS").await;

    let invalid_geometry_cases = [
        (
            "VACIA",
            "ST_GeomFromText('MULTIPOLYGON EMPTY', 4326)",
            CHECK,
        ),
        (
            "INVALIDA",
            "ST_GeomFromText('MULTIPOLYGON(((-60 -34, -59.96 -34.08, -59.96 -34, -60 -34.08, -60 -34)))', 4326)",
            CHECK,
        ),
        (
            "SRID_INCORRECTO",
            "ST_GeomFromText('MULTIPOLYGON(((-60 -34, -60 -34.01, -59.99 -34.01, -59.99 -34, -60 -34)))', 3857)",
            "22023",
        ),
    ];
    for (code, geometry_expression, sqlstate) in invalid_geometry_cases {
        let statement = format!(
            "INSERT INTO public.unidades_operativas (organizacion_id, campana_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, $3, $4, 'UOP inválida', {geometry_expression}, $5)"
        );
        let error = sqlx::query(AssertSqlSafe(statement))
            .bind(fixture.organization_id)
            .bind(campaign_id)
            .bind(fixture.establishment_id)
            .bind(code)
            .bind(fixture.user_id)
            .execute(&db)
            .await
            .expect_err("invalid geometry must fail immediately");
        assert_database_error(error, sqlstate, None);
    }
    let null_error = sqlx::query(
        "INSERT INTO public.unidades_operativas (organizacion_id, campana_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, $3, 'NULA', 'UOP nula', NULL, $4)",
    )
    .bind(fixture.organization_id)
    .bind(campaign_id)
    .bind(fixture.establishment_id)
    .bind(fixture.user_id)
    .execute(&db)
    .await
    .expect_err("null UOP geometry must fail");
    assert_database_error(null_error, "23502", None);

    create_uop(
        &db,
        fixture,
        campaign_id,
        "ORIGINAL",
        BASE_A_LEFT,
        &[fixture.base_plot_a_id],
    )
    .await;
    create_uop(
        &db,
        fixture,
        campaign_id,
        "TOCANTE",
        BASE_A_RIGHT,
        &[fixture.base_plot_a_id],
    )
    .await;

    for (code, geometry) in [
        (
            "SOLAPADA",
            "POLYGON((-59.98 -34.02, -59.98 -34.08, -59.96 -34.08, -59.96 -34.02, -59.98 -34.02))",
        ),
        (
            "CONTENIDA",
            "POLYGON((-59.995 -34.02, -59.995 -34.04, -59.985 -34.04, -59.985 -34.02, -59.995 -34.02))",
        ),
        ("DUPLICADA", BASE_A_LEFT),
    ] {
        let mut transaction = db.begin().await.expect("transaction must begin");
        let uop_id = insert_uop(&mut transaction, fixture, campaign_id, code, geometry).await;
        link_base_plot(&mut transaction, fixture, uop_id, fixture.base_plot_a_id).await;
        assert_deferred_failure(transaction, UOP_INTERIOR_OVERLAP).await;
    }

    let later_campaign = campaign(&db, fixture, "SOLAPAMIENTOS_2027").await;
    create_uop(
        &db,
        fixture,
        later_campaign,
        "MISMA_GEOMETRIA",
        BASE_A_LEFT,
        &[fixture.base_plot_a_id],
    )
    .await;
}

#[tokio::test]
async fn deferred_union_coverage_and_context_constraints_cannot_be_bypassed_by_direct_sql() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let fixture = create_fixture(&db).await;
    let campaign_id = campaign(&db, fixture, "UNION_LOTES").await;

    let grouped_id = create_uop(
        &db,
        fixture,
        campaign_id,
        "AGRUPADA",
        FIELD,
        &[fixture.base_plot_a_id, fixture.base_plot_b_id],
    )
    .await;
    let link_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.unidades_operativas_lotes_base WHERE unidad_operativa_id = $1",
    )
    .bind(grouped_id)
    .fetch_one(&db)
    .await
    .expect("M:N links must be queryable");
    assert_eq!(link_count, 2);

    let mut no_links_transaction = db.begin().await.expect("transaction must begin");
    insert_uop(
        &mut no_links_transaction,
        fixture,
        campaign_id,
        "SIN_LOTES",
        BASE_A_LEFT,
    )
    .await;
    assert_deferred_failure(no_links_transaction, "agro_ops_uop_sin_lotes_base").await;

    let mut outside_transaction = db.begin().await.expect("transaction must begin");
    let outside_id = insert_uop(
        &mut outside_transaction,
        fixture,
        campaign_id,
        "FUERA_UNION",
        "POLYGON((-60 -34, -60 -34.1, -59.89 -34.1, -59.89 -34, -60 -34))",
    )
    .await;
    link_base_plot(
        &mut outside_transaction,
        fixture,
        outside_id,
        fixture.base_plot_a_id,
    )
    .await;
    link_base_plot(
        &mut outside_transaction,
        fixture,
        outside_id,
        fixture.base_plot_b_id,
    )
    .await;
    assert_deferred_failure(outside_transaction, UOP_OUTSIDE_BASE_PLOTS).await;

    let (other_establishment_id, other_base_plot_id) = another_establishment(&db, fixture).await;
    let other_establishment_scope = TerritorialFixture {
        establishment_id: other_establishment_id,
        base_plot_a_id: other_base_plot_id,
        base_plot_b_id: other_base_plot_id,
        ..fixture
    };
    create_uop(
        &db,
        other_establishment_scope,
        campaign_id,
        "OTRO_CAMPO",
        BASE_A,
        &[other_base_plot_id],
    )
    .await;
    let cross_establishment_error = sqlx::query(
        "INSERT INTO public.unidades_operativas_lotes_base (organizacion_id, establecimiento_id, unidad_operativa_id, lote_base_id, creado_por) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(fixture.organization_id)
    .bind(fixture.establishment_id)
    .bind(grouped_id)
    .bind(other_base_plot_id)
    .bind(fixture.user_id)
    .execute(&db)
    .await
    .expect_err("a UOP cannot link a base plot from another establishment");
    assert_database_error(cross_establishment_error, FOREIGN_KEY, None);

    let other_organization = create_fixture(&db).await;
    let cross_organization_error = sqlx::query(
        "INSERT INTO public.unidades_operativas_lotes_base (organizacion_id, establecimiento_id, unidad_operativa_id, lote_base_id, creado_por) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(fixture.organization_id)
    .bind(fixture.establishment_id)
    .bind(grouped_id)
    .bind(other_organization.base_plot_a_id)
    .bind(fixture.user_id)
    .execute(&db)
    .await
    .expect_err("a UOP cannot link a base plot from another organization");
    assert_database_error(cross_organization_error, FOREIGN_KEY, None);

    let cross_campaign_organization_error = sqlx::query(
        "INSERT INTO public.unidades_operativas (organizacion_id, campana_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, $3, 'ORG_CRUZADA', 'Organización cruzada', ST_Multi(ST_GeomFromText($4, 4326)), $5)",
    )
    .bind(other_organization.organization_id)
    .bind(campaign_id)
    .bind(other_organization.establishment_id)
    .bind(BASE_A)
    .bind(other_organization.user_id)
    .execute(&db)
    .await
    .expect_err("campaign and UOP organization must agree");
    assert_database_error(cross_campaign_organization_error, FOREIGN_KEY, None);

    assert_ne!(other_establishment_id, fixture.establishment_id);
}

#[tokio::test]
async fn later_campaign_partition_does_not_rewrite_earlier_uops_or_base_plot_links() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let fixture = create_fixture(&db).await;
    let first_campaign = campaign(&db, fixture, "HISTORIA_2026").await;
    create_uop(
        &db,
        fixture,
        first_campaign,
        "A1",
        BASE_A_LEFT,
        &[fixture.base_plot_a_id],
    )
    .await;
    create_uop(
        &db,
        fixture,
        first_campaign,
        "A2",
        BASE_A_RIGHT,
        &[fixture.base_plot_a_id],
    )
    .await;
    let before = campaign_snapshot(&db, first_campaign).await;

    let second_campaign = campaign(&db, fixture, "HISTORIA_2027").await;
    create_uop(
        &db,
        fixture,
        second_campaign,
        "AB",
        FIELD,
        &[fixture.base_plot_a_id, fixture.base_plot_b_id],
    )
    .await;

    let after = campaign_snapshot(&db, first_campaign).await;
    assert_eq!(after, before);
}

async fn campaign_snapshot(
    db: &PgPool,
    campaign_id: Uuid,
) -> Vec<(Uuid, Uuid, String, String, Vec<Uuid>)> {
    sqlx::query_as(
        "SELECT unidad.id, unidad.campana_id, unidad.codigo, ST_AsEWKT(unidad.geometria), array_agg(vinculo.lote_base_id ORDER BY vinculo.lote_base_id) FROM public.unidades_operativas AS unidad JOIN public.unidades_operativas_lotes_base AS vinculo ON vinculo.unidad_operativa_id = unidad.id WHERE unidad.campana_id = $1 GROUP BY unidad.id ORDER BY unidad.codigo",
    )
    .bind(campaign_id)
    .fetch_all(db)
    .await
    .expect("campaign snapshot must be queryable")
}
